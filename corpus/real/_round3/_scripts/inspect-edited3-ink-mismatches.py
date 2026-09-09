"""Explain ink shape-count differences using read-only source/edited OOXML."""

import hashlib
import json
import posixpath
import zipfile
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path

from lxml import etree


ROOT = Path('C:/word/real-word-round3-20260907')
EDITED = Path('C:/word/round3-work-20260907/real-word-round3-inputs/edited')
REPO = Path('C:/Users/Administrator/rsWordParser')
NS = {
    'w': 'http://schemas.openxmlformats.org/wordprocessingml/2006/main',
    'wp': 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing',
    'a': 'http://schemas.openxmlformats.org/drawingml/2006/main',
    'r': 'http://schemas.openxmlformats.org/officeDocument/2006/relationships',
    'w14': 'http://schemas.microsoft.com/office/word/2010/wordml',
    'm': 'http://schemas.openxmlformats.org/officeDocument/2006/math',
    'o': 'urn:schemas-microsoft-com:office:office',
}


def sha(raw):
    return hashlib.sha256(raw).hexdigest().upper()


def xp(node, query):
    return node.xpath(query, namespaces=NS)


def first(node, query, default=None):
    data = xp(node, query)
    return data[0] if data else default


def semantic(node):
    return [node.tag, sorted(node.attrib.items()), node.text,
            [semantic(child) for child in node if isinstance(child.tag, str)]]


def inspect(path):
    raw = path.read_bytes()
    with zipfile.ZipFile(path) as archive:
        parts = {name: archive.read(name) for name in archive.namelist()}
        crc_error = archive.testzip()
    doc = etree.fromstring(parts['word/document.xml'])
    rels = {node.get('Id'): node.get('Target') for node in etree.fromstring(parts['word/_rels/document.xml.rels'])}
    anchors = []
    for anchor in xp(doc, '//wp:anchor | //wp:inline'):
        name = first(anchor, 'wp:docPr/@name', '')
        rid = first(anchor, './/a:blip/@r:embed')
        target = rels.get(rid)
        media = posixpath.normpath(posixpath.join('word', target)) if target else None
        anchors.append({'id': first(anchor, 'wp:docPr/@id'), 'name': name,
                        'kind': etree.QName(anchor).localname, 'aidocsInk': name.startswith('aidocs-ink'),
                        'payload': first(anchor, 'wp:docPr/@descr'),
                        'xEmu': first(anchor, 'wp:positionH/wp:posOffset/text()'),
                        'yEmu': first(anchor, 'wp:positionV/wp:posOffset/text()'),
                        'widthEmu': first(anchor, 'wp:extent/@cx'),
                        'heightEmu': first(anchor, 'wp:extent/@cy'),
                        'wrapNone': bool(xp(anchor, 'wp:wrapNone')),
                        'media': media, 'mediaSha256': sha(parts[media]) if media in parts else None,
                        'semanticSha256': sha(json.dumps(semantic(anchor), ensure_ascii=True, sort_keys=True).encode())})
    feature_queries = {'nativeContentPart': '//w14:contentPart', 'math': '//m:oMath', 'ole': '//o:OLEObject'}
    return {'path': str(path), 'sha256': sha(raw), 'zipCrcError': crc_error, 'anchors': anchors,
            'anchorCount': sum(item['kind'] == 'anchor' for item in anchors),
            'inlineCount': sum(item['kind'] == 'inline' for item in anchors),
            'overlayCount': sum(item['aidocsInk'] for item in anchors),
            'docPrIds': xp(doc, '//wp:docPr/@id'),
            'nativeInkPartHashes': {name: sha(data) for name, data in parts.items() if name.startswith('word/ink/')},
            'partHashes': {name: sha(data) for name, data in parts.items()},
            'featureCounts': {key: len(xp(doc, query)) for key, query in feature_queries.items()}}


latest = {}
ignored_lines = 0
log_path = ROOT / 'edited3-results.jsonl'
for line in log_path.read_text(encoding='utf-8-sig').splitlines():
    if not line:
        continue
    try:
        item = json.loads(line)
        latest[item['file']] = item
    except json.JSONDecodeError:
        ignored_lines += 1
selected = [item for item in latest.values() if item.get('op') == 'ink' and item.get('object_model_assessment') == 'mismatch']
report = {'generatedUtc': datetime.now(timezone.utc).isoformat(),
          'method': 'Read-only JSONL snapshot, ZIP/lxml source-vs-edited comparison, plus existing repository source inspection. No Word calls or GUI observations; no DOCX or repository edits. Existing PDF reports are untouched.',
          'jsonlRowsAtSnapshot': len(latest), 'incompleteJsonlLinesIgnored': ignored_lines,
          'codeEvidence': [{'path': str(REPO / relative), 'sha256': sha((REPO / relative).read_bytes()), 'lines': lines, 'meaning': meaning}
                           for relative, lines, meaning in (
                               ('crates/rsword/src/edit/ink_ops.rs', '3-6, 51-63', 'inks is an authoritative list; remove_inks deletes all editor aidocs-ink runs before insertion.'),
                               ('crates/rsword/src/save/options/mod.rs', '99-101, 172-175', 'SaveOptions.inks=Some schedules RemoveInks then InsertInk for each supplied entry.'),
                               ('crates/rsword/tests/ink.rs', '191-194', 'Existing test documents authoritative-list and repeated-save-does-not-accumulate semantics. Test was read, not executed.'))],
          'rows': []}
for item in selected:
    base = inspect(Path(item['baseline']['source_path']))
    edited = inspect(EDITED / item['file'])
    base_non = [shape for shape in base['anchors'] if not shape['aidocsInk']]
    edited_non = [shape for shape in edited['anchors'] if not shape['aidocsInk']]
    non_same = Counter(shape['semanticSha256'] for shape in base_non) == Counter(shape['semanticSha256'] for shape in edited_non)
    ink_same = base['nativeInkPartHashes'] == edited['nativeInkPartHashes']
    before_count = item['baseline']['metrics']['shapes']
    expected_replace = before_count - base['overlayCount'] + 1
    actual = item['metrics']['shapes']
    formula_agrees = actual == expected_replace and edited['overlayCount'] == 1
    changed = sorted(name for name in base['partHashes'].keys() & edited['partHashes'].keys() if base['partHashes'][name] != edited['partHashes'][name])
    old_overlays = [shape for shape in base['anchors'] if shape['aidocsInk']]
    new_overlays = [shape for shape in edited['anchors'] if shape['aidocsInk']]
    overlay_bitmap_same = bool(old_overlays) and bool(new_overlays) and all(
        old['mediaSha256'] == new['mediaSha256'] for old in old_overlays for new in new_overlays)
    row = {'file': item['file'], 'manifestExpectation': item['expect'], 'batchAssessmentPreserved': item['object_model_assessment'],
           'batchShapes': {'baseline': before_count, 'naiveExpectedPlusOne': item['checks']['shape_check']['expected_shapes'], 'actual': actual},
           'baseline': base, 'edited': edited,
           'diagnosis': {'baselineAidocsOverlays': base['overlayCount'], 'editedAidocsOverlays': edited['overlayCount'],
                         'expectedIfAuthoritativeOneOverlayList': expected_replace, 'nativeWordCountAgreesWithReplacement': formula_agrees,
                         'allNonOverlayDrawingSemanticTreesUnchanged': non_same,
                         'nativeInkPartsByteIdentical': ink_same,
                         'otherFeatureCountsUnchanged': base['featureCounts'] == edited['featureCounts'],
                         'editedDocPrIdsUnique': len(edited['docPrIds']) == len(set(edited['docPrIds'])),
                         'replacementOverlayUsesSameBitmapBytes': overlay_bitmap_same,
                         'replacementOverlayMediaPartNamesChanged': {shape['media'] for shape in old_overlays} != {shape['media'] for shape in new_overlays},
                         'onlyDocumentAndItsRelationshipsModifiedAmongRetainedParts': changed == ['word/_rels/document.xml.rels', 'word/document.xml'],
                         'conclusion': 'Old aidocs-ink overlay layer replaced with a single new overlay; no loss of non-overlay drawings or native ink detected.'
                                       if formula_agrees and non_same and ink_same else 'Requires further investigation; one or more replacement-preservation checks do not agree.'},
           'packageChanges': {'changedParts': changed, 'removedParts': sorted(base['partHashes'].keys() - edited['partHashes'].keys()),
                              'addedParts': sorted(edited['partHashes'].keys() - base['partHashes'].keys())}}
    report['rows'].append(row)
report['summary'] = {'mismatchFiles': len(report['rows']),
                     'replacementFormulaExplainsCount': sum(row['diagnosis']['nativeWordCountAgreesWithReplacement'] for row in report['rows']),
                     'nonOverlayDrawingsPreserved': sum(row['diagnosis']['allNonOverlayDrawingSemanticTreesUnchanged'] for row in report['rows']),
                     'nativeInkPartsPreserved': sum(row['diagnosis']['nativeInkPartsByteIdentical'] for row in report['rows']),
                     'advice': 'Do not reinterpret the recorded Word counts or silently overwrite batch mismatches. Add a semantic diagnosis: authoritative ink-list replacement expects baseline shapes minus existing aidocs-ink count plus one. Native ink is separate. If the task intended additive ink, flag the input generator/specification mismatch; the current package behavior matches the repository save API.'}
out = ROOT / '_scripts/edited3-ink-mismatch-audit.json'
out.write_text(json.dumps(report, ensure_ascii=True, indent=2), encoding='utf-8')
print(json.dumps({'output': str(out), 'summary': report['summary'],
                  'diagnoses': [{'file': row['file'], 'batchShapes': row['batchShapes'], 'diagnosis': row['diagnosis']} for row in report['rows']]}, ensure_ascii=True, indent=2))
