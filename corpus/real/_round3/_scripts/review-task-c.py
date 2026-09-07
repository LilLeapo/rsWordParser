"""Independent Task C integrity review. Does not read or rewrite Task B reports."""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import xml.etree.ElementTree as ET
import zipfile

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
INPUTS = Path('C:/word/round3-work-20260907/real-word-round3-inputs')
W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'


def load(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest().upper()


def local(path):
    result = Path(path)
    return result if result.is_absolute() else ROOT / result


VISUAL_REVIEW = [
    ('screenshots/c-strike-twice-font-0.jpg', '9CF14EC7B7455B2F3F78E0598BF92FFFF5E4E21D87CCBD3F76092760DBAA86F4', 'Native Font dialog for strike: single and double strike boxes are unchecked; preview has no strike line.'),
    ('screenshots/c-dstrike-twice-font-0.jpg', '6CB3DEDEC04C3594DE50A2C5B6921B5F82C9981FB81D1FDCB81B4E520A0D79D0', 'Native Font dialog: double strike checked; single strike unchecked; dstrike preview visibly double-struck.'),
    ('screenshots/c-vanish-once-font-0.jpg', '4F52A6E8A7D7DE4498E9ED888907B933E0076A38B42520D9143158293ABC9A97', 'Native Font dialog: Hidden checked. The underlying page shows vanish twice and no vanish once.'),
    ('screenshots/c-convert-mode12-before-0.jpg', 'E28816F14666BD58A9779F93191F13D8926E815F1130128D003692AD81A784D4', 'Working document title explicitly shows compatibility mode before conversion. The screenshot alone does not give the numeric12 value; that value comes from COM.'),
    ('screenshots/c-convert-info-ready-0.jpg', '3FE5EAC9006AE5DA7350B2B479A9A6B4FEFD22FAF14219BE8E4A89DE23400339', 'Word File Info page shows the working filename, Compatibility Mode panel and Convert button.'),
    ('screenshots/c-convert-action-0.jpg', '334F237573B0B5BA933FCA90D96135FC54A4086B6214DDB8162BDB77FBD0ED23', 'Native Word confirmation states the document will be upgraded to the latest file format, with OK and Cancel buttons.'),
    ('screenshots/c-convert-completed-0.jpg', '5AB485DB6D0B25F1B4096FAE3829873AA153004A64478EAA95FCCD264B9D2469', 'After conversion the working document title no longer shows compatibility mode; body appearance remains consistent with the preconversion view.'),
]


def review():
    checks = []

    def check(name, value, actual=None):
        checks.append({'check': name, 'passed': bool(value), 'actual': actual})

    ref_path = ROOT / '_scripts/toggle-round2-reference.json'
    reference = load(ref_path)['rows']
    expected = {(r['file'], r['sentence'], r['property']): r for r in reference}
    check('25 preserved prior points across eight fixtures', len(expected) == 25 and len({key[0] for key in expected}) == 8)
    check('Every prior raw readout hash still matches round2', all(sha(Path(r['priorReadoutPath'])) == r['priorReadoutSha256'] for r in reference))
    raw_paths = sorted((ROOT / '_scripts/toggle15-read').glob('*.json'))
    documents = [load(path) for path in raw_paths]
    primary = [r for d in documents if d['variant'] == 'supplied-compat15' for r in d['rows']]
    converted = [r for d in documents if d['variant'] == 'word-converted' for r in d['rows']]
    actual_keys = [(r['baseFile'], r['sentence'], r['property']) for r in primary]
    check('25 unique supplied-mode15 points exactly match round2 scope', len(primary) == 25 and len(set(actual_keys)) == 25 and set(actual_keys) == set(expected))
    check('Nine readout documents retain current source and reference hashes', len(documents) == 9 and all(sha(Path(d['fullName'])) == d['sourceSha256'] == d['sourceSha256AfterRead'] and d['referenceSha256'] == sha(ref_path) for d in documents))
    summary = load(ROOT / '_scripts/task-c-summary.json')
    summary_map = {(r['baseFile'], r['sentence'], r['property']): r for r in summary['rows']}
    recomputed = []
    for row in primary + converted:
        samples = row.get('samples', [])
        raw = samples[0].get('raw') if samples else None
        value = raw.rstrip('\r\n\x07') if row['property'] == 'HeaderDefault' and isinstance(raw, str) else True if raw == -1 else False if raw == 0 else None
        valid = len(samples) == 2 and samples[0]['raw'] == samples[1]['raw'] and value is not None and row['desktopWord'] == value and all(s['value'] == value for s in samples) and row['compatibilityMode'] == 15 and row['consistent'] and not row.get('error')
        prior = expected[(row['baseFile'], row['sentence'], row['property'])]['priorDesktopWord']
        recomputed.append({'file': row['file'], 'sentence': row['sentence'], 'property': row['property'], 'raw': [s['raw'] for s in samples], 'value': value, 'validRepeatedReading': valid, 'sameAsRound2': valid and value == prior})
    check('All37 points have two independently validated consistent mode15 readings', len(recomputed) == 37 and all(r['validRepeatedReading'] for r in recomputed))
    check('All25 supplied plus12 converted readings match round2', len(recomputed) == 37 and all(r['sameAsRound2'] for r in recomputed))
    check('Converted scope is exactly the12 other-toggle points', len(converted) == 12 and {r['sentence'] for r in converted} == {r['sentence'] for r in reference if r['file'] == 'toggle-other-toggles.docx'})
    check('Summary preserves every raw primary measurement', all(all(summary_map.get(key, {}).get(field) == row.get(field) for field in ('samples', 'desktopWord', 'compatibilityMode', 'consistent', 'error')) for key, row in zip(actual_keys, primary)))
    conversion_path = ROOT / '_readouts/task-c-conversion.json'
    conversion = load(conversion_path)
    source = INPUTS / 'fixtures/toggle-other-toggles.docx'
    target = ROOT / '_resaved/toggle-other-toggles-converted.docx'
    check('Conversion source and initial working copy are byte-identical', sha(source) == conversion['sourceSha256'] == conversion['workingSha256Before'])
    check('Conversion output hash matches actual saved DOCX and source12/output15 COM modes', sha(target) == conversion['convertedSha256'] and conversion['sourceCompatibilityMode'] == 12 and conversion['convertedCompatibilityMode'] == 15)
    with zipfile.ZipFile(target) as archive:
        xml_names = [name for name in archive.namelist() if name.endswith(('.xml', '.rels'))]
        for name in xml_names:
            ET.fromstring(archive.read(name))
        settings = ET.fromstring(archive.read('word/settings.xml'))
        mode = settings.find('.//{' + W + '}compatSetting[@{' + W + '}name="compatibilityMode"]')
        check('Converted DOCX CRC/XML and persisted compatibility15 setting', archive.testzip() is None and mode is not None and mode.get('{' + W + '}val') == '15', {'xmlParts': len(xml_names)})
    evidence = {e['path'] for row in summary['rows'] for e in row['uiEvidence']} | set(conversion['uiEvidence'])
    evidence_rows = []
    for name in sorted(evidence):
        path = local(name)
        with Image.open(path) as image:
            image.verify()
        evidence_rows.append({'file': name, 'sha256': sha(path)})
    check('25 summary rows have actual referenced UI files', all(r['uiVerified'] and r['uiEvidence'] for r in summary['rows']))
    visual_rows = [{'file': name, 'sha256': expected_hash, 'observed': note, 'current': sha(ROOT / name) == expected_hash} for name, expected_hash, note in VISUAL_REVIEW]
    check('Seven independently viewed critical screenshots retain observed bytes', all(row['current'] for row in visual_rows))
    markdown_path = ROOT / 'TOGGLE15.md'
    raw_markdown = markdown_path.read_bytes()
    text = raw_markdown.decode('utf-8-sig')
    primary_lines = [line for line in text.splitlines() if line.startswith('| ') and '-compat15.docx |' in line]
    check('Markdown has25 complete five-column primary rows without embedded bare CR', len(primary_lines) == 25 and all(len(line.split('|')) == 7 for line in primary_lines) and b'\r' not in raw_markdown.replace(b'\r\n', b''))
    check('Report primary and converted agreement summary is accurate', summary['sameAsRound2'] == 25 and summary['differentFromRound2'] == 0 and summary['allThreeAgreementRows'] == 12 and not summary['incomplete'])
    report = {'reviewedAt': datetime.now(timezone.utc).isoformat(), 'method': 'Independent read-only review of prior/current raw JSON, hashes, converted DOCX CRC/XML, Markdown row structure and saved screenshots. No Word actions and no Task B report access.',
              'visualScope': 'Seven critical Font/conversion screenshots independently viewed and hash-bound below. Table, inherited header and converted-page screenshots also visually inspected; remaining page observations are attributed to the root operator.',
              'reportSha256': sha(markdown_path), 'summarySha256': sha(ROOT / '_scripts/task-c-summary.json'), 'conversionRecordSha256': sha(conversion_path),
              'checks': checks, 'measurements': recomputed, 'evidence': evidence_rows, 'independentVisualReview': visual_rows,
              'preservedHistoricalLimit': 'Round2 toggle-para-and-char has no numeric compatibility-mode reading. Its prior titlebar showed compatibility mode; no exact historical number is fabricated.',
              'passed': all(item['passed'] for item in checks)}
    output = ROOT / '_scripts/task-c-independent-review.json'
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'path': str(output), 'passed': report['passed'], 'checks': len(checks), 'failed': [c for c in checks if not c['passed']]}, ensure_ascii=True, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(review())
