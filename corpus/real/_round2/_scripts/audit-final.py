import argparse
import collections
import hashlib
import io
import json
import posixpath
import shutil
import sys
import zipfile
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import unquote, urlsplit
from xml.etree import ElementTree

import pypdfium2 as pdfium
from PIL import Image


DELIVERY = Path('C:/word/real-word-round2-20260907')
WORK = Path('C:/word/round2-work-20260907')
INPUTS = WORK / 'real-word-round2-inputs'
INPUT_ZIP = Path('C:/word/real-word-round2-inputs-20260907.zip')
BASELINE = WORK / 'baseline/real-word-corpus-20260906'
BASELINE_ZIP = Path('C:/word/real-word-corpus-20260906.zip')
D_CASES = (
    'blank/blank-new', 'blank/blank-styles-used',
    'revisions2/rev-insert-delete', 'revisions2/rev-move', 'revisions2/rev-format',
    'revisions2/rev-table', 'revisions2/rev-section', 'revisions2/rev-accept-reject',
    'revisions2/rev-comment-threads', 'fields2/fields-seq-captions',
    'fields2/fields-index', 'fields2/fields-toc-stale', 'fields2/fields-citations',
    'fields2/fields-page-in-footer', 'sections2/sections-breaks-zoo',
    'image2/image-z-order', 'shapes2/textbox-linked',
)


def read_json(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))


def digest(data):
    return hashlib.sha256(data).hexdigest().upper()


def relative(path):
    return path.relative_to(DELIVERY).as_posix()


def evidence_paths(value):
    if isinstance(value, str):
        return [value]
    if isinstance(value, list):
        return [path for item in value for path in evidence_paths(item)]
    return []


def package_check(data, prefix=''):
    result = {'entries': 0, 'xmlParts': 0, 'errors': [], 'duplicateNames': []}
    try:
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            counts = collections.Counter(archive.namelist())
            result['duplicateNames'] = [prefix + name for name, count in counts.items() if count > 1]
            for entry in archive.infolist():
                if entry.is_dir():
                    continue
                result['entries'] += 1
                name = prefix + entry.filename
                try:
                    content = archive.read(entry)
                    if len(content) != entry.file_size:
                        raise ValueError('uncompressed length mismatch')
                    if entry.filename.endswith(('.xml', '.rels')):
                        ElementTree.fromstring(content)
                        result['xmlParts'] += 1
                    if entry.filename.endswith(('.xlsx', '.docx', '.pptx')):
                        child = package_check(content, name + '!')
                        result['entries'] += child['entries']
                        result['xmlParts'] += child['xmlParts']
                        result['errors'].extend(child['errors'])
                        result['duplicateNames'].extend(child['duplicateNames'])
                except Exception as error:
                    result['errors'].append({'part': name, 'error': str(error)})
    except Exception as error:
        result['errors'].append({'part': prefix or '(package)', 'error': str(error)})
    return result


def ink_package_details(path):
    namespaces = {
        'a': 'http://schemas.openxmlformats.org/drawingml/2006/main',
        'wp': 'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing',
        'w14': 'http://schemas.microsoft.com/office/word/2010/wordml',
        'r': 'http://schemas.openxmlformats.org/officeDocument/2006/relationships',
        'mc': 'http://schemas.openxmlformats.org/markup-compatibility/2006',
        'v': 'urn:schemas-microsoft-com:vml',
    }
    with zipfile.ZipFile(path) as archive:
        parts = {item.filename: archive.read(item) for item in archive.infolist() if not item.is_dir()}
    hashes = {name: digest(data) for name, data in parts.items()}
    xml = ElementTree.fromstring(parts['word/document.xml'])
    parents = {child: parent for parent in xml.iter() for child in parent}
    relationships = {}
    for rel in ElementTree.fromstring(parts['word/_rels/document.xml.rels']):
        target = rel.get('Target')
        if rel.get('TargetMode') != 'External':
            target = posixpath.normpath(posixpath.join('word', unquote(urlsplit(target).path)))
        relationships[rel.get('Id')] = {'target': target, 'type': rel.get('Type')}
    native = []
    ink_uri = 'http://schemas.microsoft.com/office/word/2010/wordprocessingInk'
    for graphic in xml.findall('.//a:graphicData', namespaces):
        if graphic.get('uri') != ink_uri:
            continue
        anchor = graphic
        while anchor in parents and anchor.tag != '{' + namespaces['wp'] + '}anchor':
            anchor = parents[anchor]
        doc_pr = anchor.find('wp:docPr', namespaces)
        extent = anchor.find('wp:extent', namespaces)
        content_part = graphic.find('w14:contentPart', namespaces)
        relation_id = content_part.get('{' + namespaces['r'] + '}id') if content_part is not None else None
        relationship = relationships.get(relation_id, {})
        part_name = relationship.get('target')
        native.append({
            'docPr': dict(doc_pr.attrib) if doc_pr is not None else None,
            'anchorAttributes': dict(anchor.attrib),
            'extent': dict(extent.attrib) if extent is not None else None,
            'horizontalPosition': ElementTree.tostring(anchor.find('wp:positionH', namespaces), encoding='unicode')
                                  if anchor.find('wp:positionH', namespaces) is not None else None,
            'verticalPosition': ElementTree.tostring(anchor.find('wp:positionV', namespaces), encoding='unicode')
                                if anchor.find('wp:positionV', namespaces) is not None else None,
            'contentPartRelationshipId': relation_id,
            'contentPartTarget': part_name,
            'contentPartSha256': hashes.get(part_name),
            'graphicUri': graphic.get('uri'),
        })
    fallback = []
    for item in xml.findall('.//mc:Fallback', namespaces):
        fallback.append({
            'vmlShapeCount': len(item.findall('.//v:shape', namespaces)),
            'imageData': [dict(image.attrib) for image in item.findall('.//v:imagedata', namespaces)],
        })
    return {
        'partHashes': hashes,
        'inkPartHashes': {name: value for name, value in hashes.items() if name.startswith('word/ink/')},
        'mediaPartHashes': {name: value for name, value in hashes.items() if name.startswith('word/media/')},
        'nativeInkShapeCount': len(native), 'nativeInkShapes': native,
        'alternateContentFallbacks': fallback,
    }


def retained_contents(original, recovered):
    return not (collections.Counter(original) - collections.Counter(recovered))


def content_matches(original, recovered):
    return [{'originalPart': name, 'sha256': value,
             'recoveredPartsWithSameContent': sorted(target for target, candidate in recovered.items() if candidate == value)}
            for name, value in sorted(original.items())]


def native_shape_matches(original, recovered):
    available = list(recovered)
    matches = []
    for source in original:
        target = next((shape for shape in available
                       if shape['contentPartSha256'] == source['contentPartSha256']), None)
        result = {'originalDocPr': source['docPr'], 'contentPartSha256': source['contentPartSha256'],
                  'matched': target is not None}
        if target is not None:
            available.remove(target)
            result.update(recoveredDocPr=target['docPr'],
                          extentUnchanged=source['extent'] == target['extent'],
                          horizontalPositionUnchanged=source['horizontalPosition'] == target['horizontalPosition'],
                          verticalPositionUnchanged=source['verticalPosition'] == target['verticalPosition'],
                          anchorAttributeChanges=[{'attribute': name, 'original': source['anchorAttributes'].get(name),
                                                   'recovered': target['anchorAttributes'].get(name)}
                                                  for name in sorted(source['anchorAttributes'].keys() | target['anchorAttributes'].keys())
                                                  if source['anchorAttributes'].get(name) != target['anchorAttributes'].get(name)])
        matches.append(result)
    return matches


def copy_pdf_evidence():
    source = WORK / 'pdf-review'
    readouts = DELIVERY / '_readouts/pdf-review'
    screenshots = DELIVERY / 'screenshots/pdf-review'
    readouts.mkdir(parents=True, exist_ok=True)
    screenshots.mkdir(parents=True, exist_ok=True)
    rows = read_json(source / 'header-pdf-review.json')
    for row in rows:
        row['pdf'] = '_previews/edited/' + Path(row['pdf']).name
        for page in row['pages']:
            for key in ('full_png', 'header_png'):
                original = Path(page[key])
                target = screenshots / original.name
                shutil.copy2(original, target)
                page[key] = relative(target)
    (readouts / 'header-pdf-review.json').write_text(
        json.dumps(rows, ensure_ascii=False, indent=2), encoding='utf-8'
    )
    shutil.copy2(source / 'REVIEW.md', readouts / 'REVIEW.md')
    return {'json': relative(readouts / 'header-pdf-review.json'),
            'notes': relative(readouts / 'REVIEW.md'),
            'screenshotsDirectory': relative(screenshots)}


def markdown_files(path):
    files = []
    for line in path.read_text(encoding='utf-8-sig').splitlines():
        if line.startswith('|'):
            first = line.split('|')[1].strip().strip('`')
            if first.endswith('.docx'):
                files.append(first)
    return files


parser = argparse.ArgumentParser()
parser.add_argument('--copy-pdf-evidence', action='store_true')
parser.add_argument('--final', action='store_true', help='Require final counts: 44 UI cases, 53 resaves, 17 Task D files.')
parser.add_argument('--package', type=Path, help='Audit a completed delivery ZIP against current delivery files.')
parser.add_argument('--output', type=Path, help='Override report path. Package audit defaults outside the delivery tree.')
args = parser.parse_args()
OUTPUT = args.output or (WORK / 'final-package-audit.json' if args.package else
                         DELIVERY / '_scripts/final-independent-audit.json')
report = {
    'auditedAt': datetime.now(timezone.utc).isoformat(),
    'scope': 'Final independent audit.' if args.final else 'Current independent baseline audit; final counts may be pending.',
    'method': 'SHA-256 against supplied ZIP members, structured JSON/XML/ZIP parsing, PDF page parsing and image decoding. No Word application calls.',
    'validationLimit': 'ZIP CRC, lengths, unique member names and XML well-formedness are checked. This is not complete OOXML schema validation or a substitute for Word UI observations.',
    'deliveryRoot': str(DELIVERY),
    'finalCountsRequired': args.final,
}
if args.copy_pdf_evidence:
    report['copiedPdfReviewEvidence'] = copy_pdf_evidence()

initial = read_json(DELIVERY / '_scripts/input-audit.json')
manifest = read_json(INPUTS / '_roundtrip/edited/manifest.json')
generated = [row for row in manifest if not row.get('status', '').startswith('skipped:')]
source_rows = []
with zipfile.ZipFile(INPUT_ZIP) as archive:
    for entry in archive.infolist():
        if entry.is_dir() or not entry.filename.endswith('.docx'):
            continue
        local = WORK / entry.filename
        expected = digest(archive.read(entry))
        actual = digest(local.read_bytes()) if local.exists() else None
        source_rows.append({'file': entry.filename, 'sha256': actual,
                            'archiveSha256': expected, 'unchanged': actual == expected})
report['inputHashes'] = {
    'archive': str(INPUT_ZIP), 'checkedDocx': len(source_rows),
    'unchangedDocx': sum(row['unchanged'] for row in source_rows),
    'mismatches': [row for row in source_rows if not row['unchanged']],
    'files': source_rows,
}

baseline_rows = []
with zipfile.ZipFile(BASELINE_ZIP) as archive:
    for name in sorted({row['base'] for row in manifest}):
        expected = digest(archive.read('real-word-corpus-20260906/' + name))
        local = BASELINE / name
        actual = digest(local.read_bytes()) if local.exists() else None
        baseline_rows.append({'file': name, 'sha256': actual, 'archiveSha256': expected,
                              'unchanged': actual == expected})
report['baselineHashes'] = {
    'archive': str(BASELINE_ZIP), 'checkedDocx': len(baseline_rows),
    'unchangedDocx': sum(row['unchanged'] for row in baseline_rows),
    'mismatches': [row for row in baseline_rows if not row['unchanged']], 'files': baseline_rows,
}

resaved_rows = []
for item in initial['roundtrip']:
    path = DELIVERY / '_resaved' / (Path(item['file']).stem + '-resaved-by-word.docx')
    original = INPUTS / '_roundtrip' / item['file']
    original_hash = digest(original.read_bytes())
    row = {'original': item['file'], 'resaved': relative(path),
           'originalSha256': original_hash, 'originalMatchesInitial': original_hash == item['sha256'],
           'exists': path.exists()}
    if path.exists():
        content = path.read_bytes()
        row.update(sha256=digest(content), differsFromOriginal=digest(content) != original_hash,
                   package=package_check(content))
    resaved_rows.append(row)
report['roundtrip'] = {
    'expected': 9, 'inputRows': len(initial['roundtrip']),
    'markdownRows': len(markdown_files(DELIVERY / 'ROUNDTRIP2.md')),
    'resavedExists': sum(row['exists'] for row in resaved_rows),
    'originalsUnchanged': sum(row['originalMatchesInitial'] for row in resaved_rows),
    'resavedDifferent': sum(row.get('differsFromOriginal', False) for row in resaved_rows),
    'entriesIncludingEmbeddedPackages': sum(row.get('package', {}).get('entries', 0) for row in resaved_rows),
    'xmlParts': sum(row.get('package', {}).get('xmlParts', 0) for row in resaved_rows),
    'packageErrors': [error for row in resaved_rows for error in row.get('package', {}).get('errors', [])],
    'files': resaved_rows,
}

edited = read_json(DELIVERY / 'edited-results.json')
expected_names = {row['file'] for row in generated}
actual_names = collections.Counter(row['file'] for row in edited)
report['edited'] = {
    'expected': 944, 'manifestRows': len(manifest), 'generatedRows': len(generated),
    'skippedRows': len(manifest) - len(generated), 'resultRows': len(edited),
    'uniqueResultFiles': len(actual_names), 'markdownRows': len(markdown_files(DELIVERY / 'EDITED.md')),
    'missingResultFiles': sorted(expected_names - actual_names.keys()),
    'unexpectedResultFiles': sorted(actual_names.keys() - expected_names),
    'duplicateResultFiles': [name for name, count in actual_names.items() if count > 1],
    'openStatusCounts': dict(collections.Counter(str(row.get('open')) for row in edited)),
    'objectModelAssessmentCounts': dict(collections.Counter(str(row.get('object_model_assessment')) for row in edited)),
    'uiCheckedCount': sum(row.get('ui_checked') is True for row in edited),
}

toggle_files = sorted((DELIVERY / '_scripts/toggle-read/round2-20260907-live').glob('*.json'))
toggle_rows = [row for path in toggle_files for row in read_json(path)['rows']]
key = lambda row: (row['file'], row['sentence'], row['property'])
expected_toggle = {key(row) for row in initial['toggleRequiredRows']}
actual_toggle = collections.Counter(key(row) for row in toggle_rows)
toggle_evidence = [item['path'] for row in toggle_rows for item in row.get('uiEvidence', []) if item.get('path')]
report['toggle'] = {
    'expected': 25, 'files': len(toggle_files), 'rows': len(toggle_rows),
    'markdownRows': len(markdown_files(DELIVERY / 'TOGGLE.md')),
    'samples': sum(len(row.get('samples', [])) for row in toggle_rows),
    'rowsWithExactlyTwoSamples': sum(len(row.get('samples', [])) == 2 for row in toggle_rows),
    'consistentRows': sum(row.get('consistent') is True for row in toggle_rows),
    'uiVerifiedRows': sum(row.get('uiVerified') is True for row in toggle_rows),
    'comparisonCounts': dict(collections.Counter(row.get('comparison') for row in toggle_rows)),
    'missingRows': sorted(expected_toggle - actual_toggle.keys()),
    'unexpectedRows': sorted(actual_toggle.keys() - expected_toggle),
    'duplicateRows': [item for item, count in actual_toggle.items() if count > 1],
    'rowErrors': [row for row in toggle_rows if row.get('error')],
    'missingEvidence': sorted({name for name in toggle_evidence if not (DELIVERY / name).is_file()}),
}

ui_rows = read_json(DELIVERY / 'ui-edited.json')
ui_names = collections.Counter(row['file'] for row in ui_rows)
ui_plan = read_json(DELIVERY / '_scripts/selection.json')['selected']
ui_plan_names = {row['file'] for row in ui_plan}
manifest_by_name = {row['file']: row for row in generated}
ui_operation_counts = collections.Counter(manifest_by_name.get(row['file'], {}).get('op') for row in ui_rows)
batch_failure_names = {row['file'] for row in edited if row.get('open') == 'error'}
merged_ui_names = {row['file'] for row in edited if row.get('ui_checked') is True}
ui_evidence = [path for row in ui_rows for name in ('screenshot', 'pdf', 'resaved', 'object_model_readout')
               for path in evidence_paths(row.get(name))]
report['ui'] = {
    'expectedFinalRows': 44, 'rows': len(ui_rows), 'uniqueFiles': len(ui_names),
    'duplicateFiles': [name for name, count in ui_names.items() if count > 1],
    'unknownFiles': sorted(ui_names.keys() - expected_names),
    'missingEvidence': sorted({name for name in ui_evidence if not (DELIVERY / name).is_file()}),
    'rowsWithResavedPath': sum(bool(row.get('resaved')) for row in ui_rows),
    'plannedFiles': len(ui_plan_names), 'missingPlannedFiles': sorted(ui_plan_names - ui_names.keys()),
    'operationCounts': dict(ui_operation_counts),
    'batchFailures': len(batch_failure_names), 'batchFailuresWithoutUi': sorted(batch_failure_names - ui_names.keys()),
    'uiMissingFromMergedResults': sorted(ui_names.keys() - merged_ui_names),
    'mergedUiWithoutObservation': sorted(merged_ui_names - ui_names.keys()),
}

all_resaved = []
resaved_lock_files = []
for path in sorted((DELIVERY / '_resaved').rglob('*.docx')):
    if path.name.startswith('~$'):
        resaved_lock_files.append(relative(path))
        continue
    content = path.read_bytes()
    all_resaved.append({'file': relative(path), 'sha256': digest(content), 'package': package_check(content)})
report['allResaved'] = {
    'expectedFinalFiles': 53, 'fileCount': len(all_resaved),
    'lockFilesExcluded': resaved_lock_files,
    'entriesIncludingEmbeddedPackages': sum(row['package']['entries'] for row in all_resaved),
    'xmlParts': sum(row['package']['xmlParts'] for row in all_resaved),
    'errors': [{'file': row['file'], 'error': error}
               for row in all_resaved for error in row['package']['errors']],
    'duplicateNames': [{'file': row['file'], 'part': name}
                       for row in all_resaved for name in row['package']['duplicateNames']],
    'files': all_resaved,
}

d_result_path = DELIVERY / '_scripts/task-d-results.json'
d_results = read_json(d_result_path) if d_result_path.exists() else []
d_by_case = {row['case']: row for row in d_results}
d_case_counts = collections.Counter(row['case'] for row in d_results)
d_rows = []
for case in D_CASES:
    path = DELIVERY / (case + '.docx')
    recorded = d_by_case.get(case, {})
    row = {'case': case, 'file': relative(path), 'exists': path.exists(),
           'recordedSha256': recorded.get('sha256'), 'recordedStatus': recorded.get('status'),
           'recordedSelfcheckPassed': (recorded.get('selfcheck') or {}).get('passed'),
           'recordedVisualChecked': recorded.get('visualChecked')}
    if path.exists():
        content = path.read_bytes()
        row.update(sha256=digest(content), matchesFirstSavedHash=digest(content) == recorded.get('sha256'),
                   package=package_check(content))
    d_rows.append(row)
report['taskD'] = {
    'expectedFinalFiles': len(D_CASES), 'filesPresent': sum(row['exists'] for row in d_rows),
    'resultRows': len(d_results),
    'missingResultCases': sorted(set(D_CASES) - d_by_case.keys()),
    'unexpectedResultCases': sorted(d_by_case.keys() - set(D_CASES)),
    'duplicateResultCases': [case for case, count in d_case_counts.items() if count > 1],
    'hashMatches': sum(row.get('matchesFirstSavedHash') is True for row in d_rows),
    'hashMismatches': [row['file'] for row in d_rows if row['exists'] and not row.get('matchesFirstSavedHash')],
    'packageErrors': [{'file': row['file'], 'error': error}
                      for row in d_rows for error in row.get('package', {}).get('errors', [])],
    'selfcheckProvenance': 'recordedSelfcheckPassed and recordedVisualChecked are reported by task-d-results.json, not independently reproduced by this audit.',
    'recordedIncompleteSelfchecks': [row['file'] for row in d_rows
                                     if row['exists'] and row['recordedSelfcheckPassed'] is not True],
    'files': d_rows,
}

d_review_path = DELIVERY / '_readouts/task-d-pdf-review.json'
d_review = read_json(d_review_path) if d_review_path.exists() else {}
d_review_rows = d_review.get('rows', [])
d_review_counts = collections.Counter(row['case'] for row in d_review_rows)
d_review_integrity = []
for row in d_review_rows:
    case = row['case']
    source = DELIVERY / (case + '.docx')
    pdf = DELIVERY / '_previews' / (case + '.pdf')
    annotation = row.get('visualReview', {})
    checks = {
        'DOCX review hash current': source.exists() and digest(source.read_bytes()) == row.get('sha256'),
        'PDF review hash current': pdf.exists() and digest(pdf.read_bytes()) == row.get('pdfSha256'),
        'visual annotation hash current': bool(annotation) and annotation.get('pdfSha256') == row.get('pdfSha256'),
        'every PDF page visually reviewed': row.get('visualReviewed') is True and
            sorted(annotation.get('pageNumbers', [])) == list(range(1, row.get('pageCount', 0) + 1)),
        'rendered evidence exists': bool(row.get('pages')) and
            all((DELIVERY / page['png']).is_file() for page in row.get('pages', [])),
    }
    d_review_integrity.extend({'case': case, 'check': name} for name, passed in checks.items() if not passed)
report['taskDIndependentReview'] = {
    'source': relative(d_review_path), 'rows': len(d_review_rows),
    'missingCases': sorted(set(D_CASES) - d_review_counts.keys()),
    'unexpectedCases': sorted(d_review_counts.keys() - set(D_CASES)),
    'duplicateCases': [case for case, count in d_review_counts.items() if count > 1],
    'integrityFailures': d_review_integrity,
    'structurePassed': sum(row.get('structure', {}).get('passed') is True for row in d_review_rows),
    'structureFailures': [{'case': row['case'], 'failedChecks': [item for item in row.get('structure', {}).get('checks', [])
                                                               if not item['passed']]}
                          for row in d_review_rows if row.get('structure', {}).get('passed') is not True],
    'visualIssues': [{'case': row['case'], 'issues': row['visualReview']['issues']}
                     for row in d_review_rows if row.get('visualReview', {}).get('issues')],
    'interpretation': 'The audit integrity gate requires current evidence and complete review coverage. Substantive structure failures and visual issues remain explicit limitations; an integrity pass does not mean all requested Word behaviors were achieved.',
}

ink_cases = read_json(DELIVERY / '_scripts/failure-structure-audit.json')['failures']
ink_rows = []
for case in ink_cases:
    source = INPUTS / '_roundtrip/edited' / case['file']
    recovered = DELIVERY / '_resaved' / (Path(case['file']).stem + '-resaved-by-word.docx')
    row = {'file': case['file'], 'recoveredFile': relative(recovered), 'recoveredExists': recovered.exists()}
    if recovered.exists():
        original = ink_package_details(source)
        saved = ink_package_details(recovered)
        left, right = original['partHashes'], saved['partHashes']
        row.update(
            original=original, recovered=saved,
            removedParts=sorted(left.keys() - right.keys()),
            addedParts=sorted(right.keys() - left.keys()),
            changedParts=[{'part': name, 'originalSha256': left[name], 'recoveredSha256': right[name]}
                          for name in sorted(left.keys() & right.keys()) if left[name] != right[name]],
            unchangedParts=sorted(name for name in left.keys() & right.keys() if left[name] == right[name]),
            nativeInkShapeCountRetained=original['nativeInkShapeCount'] == saved['nativeInkShapeCount'],
            nativeInkReferencedContentsRetained=retained_contents(
                [shape['contentPartSha256'] for shape in original['nativeInkShapes']],
                [shape['contentPartSha256'] for shape in saved['nativeInkShapes']]),
            inkPartContentsRetained=retained_contents(original['inkPartHashes'].values(), saved['inkPartHashes'].values()),
            mediaPartContentsRetained=retained_contents(original['mediaPartHashes'].values(), saved['mediaPartHashes'].values()),
            inkContentMatches=content_matches(original['inkPartHashes'], saved['inkPartHashes']),
            mediaContentMatches=content_matches(original['mediaPartHashes'], saved['mediaPartHashes']),
            nativeShapeComparisons=native_shape_matches(original['nativeInkShapes'], saved['nativeInkShapes']),
        )
    ink_rows.append(row)
report['recoveredInk'] = {
    'expectedFinalFiles': 9, 'recoveredFilesPresent': sum(row['recoveredExists'] for row in ink_rows),
    'method': 'Input and Word-recovered ZIP part SHA-256, relationship-resolved native Ink content parts, native drawing count and anchor representations. Content comparisons permit part renaming and preserve multiplicity. This is package evidence, not a visual assertion.',
    'countsOrInkContentDifferences': [row['file'] for row in ink_rows if row['recoveredExists'] and
                                    not (row['nativeInkShapeCountRetained'] and row['nativeInkReferencedContentsRetained']
                                         and row['inkPartContentsRetained'])],
    'mediaContentDifferences': [row['file'] for row in ink_rows if row['recoveredExists'] and not row['mediaPartContentsRetained']],
    'files': ink_rows,
}

artifact_errors = []
artifact_counts = collections.Counter()
deliverable_docx = []
for path in DELIVERY.rglob('*'):
    if not path.is_file() or path == OUTPUT:
        continue
    suffix = path.suffix.lower()
    try:
        if suffix == '.docx' and not path.name.startswith('~$') and path.name != '01-source-test.docx':
            content = path.read_bytes()
            package = package_check(content)
            deliverable_docx.append({'file': relative(path), 'sha256': digest(content),
                                     'entries': package['entries'], 'xmlParts': package['xmlParts']})
            artifact_errors.extend({'file': relative(path), 'error': error} for error in package['errors'])
            artifact_errors.extend({'file': relative(path), 'error': 'duplicate ZIP member: ' + name}
                                   for name in package['duplicateNames'])
            artifact_counts['docx'] += 1
        elif suffix == '.json':
            read_json(path)
            artifact_counts['json'] += 1
        elif suffix == '.jsonl':
            for line in path.read_text(encoding='utf-8-sig').splitlines():
                if line.strip():
                    json.loads(line)
            artifact_counts['jsonl'] += 1
        elif suffix == '.pdf':
            document = pdfium.PdfDocument(path)
            if len(document) == 0:
                raise ValueError('empty PDF')
            document.close()
            artifact_counts['pdf'] += 1
        elif suffix in ('.png', '.jpg', '.jpeg'):
            with Image.open(path) as image:
                image.verify()
            artifact_counts['images'] += 1
    except Exception as error:
        artifact_errors.append({'file': relative(path), 'error': str(error)})
report['artifacts'] = {
    'validCounts': dict(artifact_counts), 'errors': artifact_errors,
    'docxScope': 'Every non-lock DOCX in the delivery tree, including documented trial originals; the excluded smoke artifact is omitted.',
    'docxFiles': deliverable_docx,
    'nonPdfPreviewFiles': [relative(path) for path in (DELIVERY / '_previews').rglob('*')
                           if path.is_file() and path.suffix.lower() != '.pdf'],
    'rootDocxFiles': [relative(path) for path in DELIVERY.glob('*.docx')],
    'temporaryControlDirectoryPresent': (DELIVERY / '_control').exists(),
}

if args.package:
    package_rows = []
    excluded_names = {'01-source-test.docx', 'edited-current-item.json'}
    intended = {relative(path): path for path in DELIVERY.rglob('*') if path.is_file()
                and '_control' not in path.relative_to(DELIVERY).parts
                and not path.name.startswith('~$') and relative(path) not in excluded_names}
    with zipfile.ZipFile(args.package) as archive:
        members = [entry for entry in archive.infolist() if not entry.is_dir()]
        names = [entry.filename.replace('\\', '/') for entry in members]
        prefix = DELIVERY.name + '/'
        strip_prefix = all(name.startswith(prefix) for name in names)
        member_names = []
        for entry, name in zip(members, names):
            name = name[len(prefix):] if strip_prefix else name
            member_names.append(name)
            try:
                content = archive.read(entry)
                local = intended.get(name)
                package_rows.append({'file': name, 'crcValid': True,
                                     'matchesDelivery': local is not None and digest(content) == digest(local.read_bytes())})
            except Exception as error:
                package_rows.append({'file': name, 'crcValid': False, 'error': str(error)})
    member_counts = collections.Counter(member_names)
    report['package'] = {
        'file': str(args.package), 'sha256': digest(args.package.read_bytes()), 'memberFiles': len(package_rows),
        'expectedFiles': len(intended), 'prefixStripped': strip_prefix,
        'missingFiles': sorted(intended.keys() - member_counts.keys()),
        'unexpectedFiles': sorted(member_counts.keys() - intended.keys()),
        'duplicateNames': [name for name, count in member_counts.items() if count > 1],
        'invalidOrDifferentFiles': [row for row in package_rows if not row.get('crcValid') or not row.get('matchesDelivery')],
        'lockFiles': [name for name in member_names if Path(name).name.startswith('~$')],
    }

failures = []
for name, condition in (
    ('input hashes unchanged', not report['inputHashes']['mismatches']),
    ('baseline hashes unchanged', not report['baselineHashes']['mismatches']),
    ('9 roundtrip rows and changed resaves', report['roundtrip']['markdownRows'] == 9
     and report['roundtrip']['resavedExists'] == 9 and report['roundtrip']['resavedDifferent'] == 9),
    ('944 edited rows with exact input coverage', len(edited) == 944 and report['edited']['markdownRows'] == 944
     and not report['edited']['missingResultFiles'] and not report['edited']['unexpectedResultFiles']
     and not report['edited']['duplicateResultFiles']),
    ('25 toggle rows and 50 consistent readings', len(toggle_rows) == 25 and report['toggle']['markdownRows'] == 25
     and report['toggle']['rowsWithExactlyTwoSamples'] == 25 and report['toggle']['consistentRows'] == 25
     and not report['toggle']['missingRows'] and not report['toggle']['rowErrors'] and not report['toggle']['missingEvidence']),
    ('all resaved packages valid', not report['allResaved']['errors'] and not report['allResaved']['duplicateNames']),
    ('artifact formats valid', not artifact_errors),
    ('preview directory contains only PDFs', not report['artifacts']['nonPdfPreviewFiles']),
    ('UI evidence exists', not report['ui']['missingEvidence'] and not report['ui']['duplicateFiles']),
    ('current Task D hashes and packages valid', not report['taskD']['hashMismatches']
     and not report['taskD']['packageErrors'] and not report['taskD']['duplicateResultCases']),
):
    if not condition:
        failures.append(name)
if args.final:
    for name, condition in (
        ('44 unique UI rows', len(ui_rows) == 44 and len(ui_names) == 44),
        ('three UI cases per operation and every normal-open failure reviewed', len(ui_plan_names) == 36
         and len(ui_operation_counts) == 12 and all(count >= 3 for count in ui_operation_counts.values())
         and not report['ui']['missingPlannedFiles'] and not report['ui']['batchFailuresWithoutUi']
         and not report['ui']['uiMissingFromMergedResults'] and not report['ui']['mergedUiWithoutObservation']),
        ('53 resaved files', len(all_resaved) == 53),
        ('17 Task D originals with matching first-save hashes', report['taskD']['filesPresent'] == 17
         and report['taskD']['hashMatches'] == 17 and report['taskD']['resultRows'] == 17),
        ('9 recovered Ink files audited', report['recoveredInk']['recoveredFilesPresent'] == 9),
        ('17 current independent Task D PDF and structure reviews', len(d_review_rows) == 17
         and not report['taskDIndependentReview']['missingCases']
         and not report['taskDIndependentReview']['unexpectedCases']
         and not report['taskDIndependentReview']['duplicateCases'] and not d_review_integrity),
    ):
        if not condition:
            failures.append(name)
if args.package:
    package = report['package']
    if any(package[name] for name in ('missingFiles', 'unexpectedFiles', 'duplicateNames', 'invalidOrDifferentFiles', 'lockFiles')):
        failures.append('delivery ZIP exactly matches included files with valid CRCs and no lock files')
report['auditPassed'] = not failures
report['failedChecks'] = failures
OUTPUT.parent.mkdir(parents=True, exist_ok=True)
OUTPUT.write_text(json.dumps(report, ensure_ascii=True, indent=2), encoding='utf-8')
print(json.dumps({
    'report': str(OUTPUT),
    'inputs': {k: v for k, v in report['inputHashes'].items() if k != 'files'},
    'baselines': {k: v for k, v in report['baselineHashes'].items() if k != 'files'},
    'roundtrip': {k: v for k, v in report['roundtrip'].items() if k != 'files'},
    'edited': report['edited'], 'toggle': report['toggle'], 'ui': report['ui'],
    'allResaved': {k: v for k, v in report['allResaved'].items() if k != 'files'},
    'taskD': {k: v for k, v in report['taskD'].items() if k != 'files'},
    'taskDIndependentReview': report['taskDIndependentReview'],
    'recoveredInk': {k: v for k, v in report['recoveredInk'].items() if k != 'files'},
    'artifacts': {k: v for k, v in report['artifacts'].items() if k != 'docxFiles'}, 'package': report.get('package'),
    'auditPassed': report['auditPassed'], 'failedChecks': failures,
}, ensure_ascii=True, indent=2))
sys.exit(1 if failures else 0)
