"""Read-only input package, fixture and baseline audit. Never controls Word."""
from __future__ import annotations

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import io
import json
from pathlib import Path
import xml.etree.ElementTree as ET
import zipfile

W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
REL = 'http://schemas.openxmlformats.org/package/2006/relationships'
CT = 'http://schemas.openxmlformats.org/package/2006/content-types'


def sha(data):
    return hashlib.sha256(data).hexdigest().upper()


def parts(data):
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        return {name: archive.read(name) for name in archive.namelist() if not name.endswith('/')}


def without_settings_relationship(data):
    root = ET.fromstring(data)
    return sorted(tuple(sorted(e.attrib.items())) for e in root if not e.attrib.get('Type', '').endswith('/settings'))


def without_settings_content_type(data):
    root = ET.fromstring(data)
    return sorted(tuple(sorted(e.attrib.items())) for e in root if e.attrib.get('PartName') != '/word/settings.xml')


def audit(args):
    root = Path(args.input_root)
    fixture_root = root / 'fixtures'
    baseline_roots = [Path(args.round1_root), Path(args.round2_root)]
    with zipfile.ZipFile(args.input_zip, metadata_encoding='utf-8') as archive:
        archive_crc = archive.testzip()
        prefix = 'real-word-round3-inputs/'
        members = [e for e in archive.infolist() if not e.is_dir()]
        extracted_rows = []
        for member in members:
            if not member.filename.startswith(prefix):
                raise ValueError(f'Unexpected input archive prefix: {member.filename}')
            relative = member.filename[len(prefix):]
            path = root / relative
            content = archive.read(member)
            extracted_rows.append({'file': relative, 'filenameUtf8Flag': bool(member.flag_bits & 0x800), 'archiveSha256': sha(content), 'extractedSha256': sha(path.read_bytes()) if path.is_file() else None, 'match': path.is_file() and content == path.read_bytes()})
    manifest = json.loads((root / 'edited' / 'manifest.json').read_text(encoding='utf-8-sig'))
    generated = [row for row in manifest if row['status'] == 'generated']
    expected_files = {row['file'] for row in generated}
    actual_files = {path.name for path in (root / 'edited').glob('*.docx')}
    baseline_rows = []
    for base in sorted({row['base'] for row in manifest}):
        candidates = [candidate / base for candidate in baseline_roots if (candidate / base).is_file()]
        baseline_rows.append({'base': base, 'candidates': [{'path': str(path), 'sha256': sha(path.read_bytes())} for path in candidates], 'resolved': len(candidates) == 1, 'provenanceNote': 'Manifest path resolved to preserved prior delivery. Input package has edited outputs but no source copies, so this is not a byte-level proof of the exact engine input.'})
    fixture_rows = []
    prior_fixture_root = Path(args.round2_input_root) / 'fixtures'
    for old_path in sorted(path for path in fixture_root.glob('*.docx') if not path.stem.endswith('-compat15')):
        old_bytes = old_path.read_bytes()
        new_path = old_path.with_stem(old_path.stem + '-compat15')
        new_bytes = new_path.read_bytes()
        old_parts, new_parts = parts(old_bytes), parts(new_bytes)
        shared = old_parts.keys() & new_parts.keys()
        changed = sorted(name for name in shared if old_parts[name] != new_parts[name])
        added = sorted(new_parts.keys() - old_parts.keys())
        removed = sorted(old_parts.keys() - new_parts.keys())
        compatibility = ET.fromstring(new_parts['word/settings.xml']).find(f'.//{{{W}}}compatSetting[@{{{W}}}name="compatibilityMode"]')
        value = compatibility.get(f'{{{W}}}val') if compatibility is not None else None
        rels_only_settings = without_settings_relationship(old_parts['word/_rels/document.xml.rels']) == without_settings_relationship(new_parts['word/_rels/document.xml.rels'])
        ct_only_settings = without_settings_content_type(old_parts['[Content_Types].xml']) == without_settings_content_type(new_parts['[Content_Types].xml'])
        prior_path = prior_fixture_root / old_path.name
        fixture_rows.append({'file': old_path.name, 'compat15File': new_path.name, 'originalSha256': sha(old_bytes), 'compat15Sha256': sha(new_bytes), 'round2Sha256': sha(prior_path.read_bytes()) if prior_path.is_file() else None, 'byteIdenticalToRound2': prior_path.is_file() and prior_path.read_bytes() == old_bytes, 'addedParts': added, 'removedParts': removed, 'changedExistingParts': changed, 'compatibilityMode': value, 'relationshipChangesOnlySettings': rels_only_settings, 'contentTypeChangesOnlySettings': ct_only_settings, 'unchangedContentAndStyles': old_parts.get('word/document.xml') == new_parts.get('word/document.xml') and old_parts.get('word/styles.xml') == new_parts.get('word/styles.xml'), 'settingsOnlyChangeIncludingRequiredRegistration': added == ['word/settings.xml'] and not removed and set(changed) <= {'[Content_Types].xml', 'word/_rels/document.xml.rels'} and rels_only_settings and ct_only_settings and value == '15'})
    output = {'schemaVersion': 1, 'auditedAt': datetime.now(timezone.utc).isoformat(), 'method': 'Read-only ZIP, SHA256, XML and filesystem inspection; no Word opens or measurements.', 'inputZip': args.input_zip, 'inputZipSha256': sha(Path(args.input_zip).read_bytes()), 'inputRoot': str(root), 'archiveCrcError': archive_crc, 'counts': {'archiveFiles': len(extracted_rows), 'inputDocx': sum(row['file'].endswith('.docx') for row in extracted_rows), 'manifestRows': len(manifest), 'generatedRows': len(generated), 'actualEditedDocx': len(actual_files), 'uniqueBaselinePaths': len(baseline_rows), 'fixtureDocx': len(fixture_rows) * 2, 'byGeneratedOperation': dict(sorted(Counter(row['op'] for row in generated).items())), 'byStatus': dict(Counter(row['status'] for row in manifest))}, 'archiveExtraction': extracted_rows, 'allArchiveFilesMatchExtraction': all(row['match'] for row in extracted_rows), 'generatedFileMissing': sorted(expected_files - actual_files), 'unmanifestedEditedDocx': sorted(actual_files - expected_files), 'duplicateGeneratedFileNames': sorted(name for name, count in Counter(row['file'] for row in generated).items() if count > 1), 'nongeneratedManifestRows': [row for row in manifest if row['status'] != 'generated'], 'baselineMap': baseline_rows, 'unresolvedBaselines': [row['base'] for row in baseline_rows if not row['resolved']], 'fixtures': fixture_rows, 'notes': ['Task text says127 bases, but manifest names180: original110 plus53 Word resaves and17 round2 authored documents.', 'The1544 required measurements correspond to generated rows.11 ChartEx chartdata rows were skipped, and fields-toc-stale--deleteblock failed engine save with FLD_STRAY_END; those12 have no input DOCX.', 'A settings part requires relationship and content-type registration. Changed XML packaging registration is reported separately from unchanged document/styles bytes.']}
    Path(args.output).write_text(json.dumps(output, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    return output


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--input-root', default='C:/word/round3-work-20260907/real-word-round3-inputs')
    parser.add_argument('--input-zip', default='C:/word/real-word-round3-inputs-20260907.zip')
    parser.add_argument('--round1-root', default='C:/word/round2-work-20260907/baseline/real-word-corpus-20260906')
    parser.add_argument('--round2-root', default='C:/word/real-word-round2-20260907')
    parser.add_argument('--round2-input-root', default='C:/word/round2-work-20260907/real-word-round2-inputs')
    parser.add_argument('--output', default=str(Path(__file__).with_name('round3-input-audit.json')))
    result = audit(parser.parse_args())
    print(json.dumps({key: result[key] for key in ('counts', 'allArchiveFilesMatchExtraction', 'generatedFileMissing', 'unmanifestedEditedDocx', 'unresolvedBaselines')}, ensure_ascii=False, indent=2))
    print(json.dumps({'fixturePairs': len(result['fixtures']), 'allOldFixturesMatchRound2': all(row['byteIdenticalToRound2'] for row in result['fixtures']), 'allCompat15FixturesChangeOnlySettingsAndRegistration': all(row['settingsOnlyChangeIncludingRequiredRegistration'] for row in result['fixtures'])}, indent=2))
