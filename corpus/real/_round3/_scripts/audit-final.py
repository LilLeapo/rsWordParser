"""Round3 delivery integrity gates. Read-only; never opens or controls Word."""
from __future__ import annotations

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import sys
import xml.etree.ElementTree as ET
import zipfile

from PIL import Image
import pypdfium2 as pdfium

DEFAULT_ROOT = Path('C:/word/real-word-round3-20260907')
DEFAULT_WORK = Path('C:/word/round3-work-20260907')
TRIPLES = ('run-edits', 'para-split-merge', 'table-and-move', 'tracked-two-authors')
PAIRS = ('sect-insert', 'sect-delete', 'z-order', 'move-resize')
A_EXPECTED = [(case, stage) for case in TRIPLES for stage in ('base', 'tracked', 'accepted', 'rejected')] + [(case, stage) for case in PAIRS for stage in ('before', 'after')]
D_EXPECTED = ('ink2/ink-to-shape-2.docx', 'comments2/comment-nesting.docx')
W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
W15 = 'http://schemas.microsoft.com/office/word/2012/wordml'
REVISION_NAMES = {'ins', 'del', 'moveFrom', 'moveTo', 'moveFromRangeStart', 'moveFromRangeEnd', 'moveToRangeStart', 'moveToRangeEnd', 'rPrChange', 'pPrChange', 'tblPrChange', 'tblGridChange', 'trPrChange', 'tcPrChange', 'sectPrChange', 'numberingChange', 'cellIns', 'cellDel', 'cellMerge', 'customXmlInsRangeStart', 'customXmlInsRangeEnd', 'customXmlDelRangeStart', 'customXmlDelRangeEnd', 'customXmlMoveFromRangeStart', 'customXmlMoveFromRangeEnd', 'customXmlMoveToRangeStart', 'customXmlMoveToRangeEnd'}


def digest(data):
    return hashlib.sha256(data).hexdigest().upper()


def file_hash(path):
    return digest(path.read_bytes()) if path.is_file() else None


def read_json(path, default=None):
    return json.loads(path.read_text(encoding='utf-8-sig')) if path.is_file() else default


def as_rows(value):
    if isinstance(value, list):
        return value
    if isinstance(value, dict):
        return value.get('rows', value.get('results', []))
    return []


def local_path(root, value):
    path = Path(str(value))
    return path if path.is_absolute() else root / path


def relative(root, path):
    try:
        return path.relative_to(root).as_posix()
    except ValueError:
        return str(path)


def evidence_paths(value):
    if isinstance(value, str):
        return [value] if value.strip() else []
    if isinstance(value, list):
        return [path for item in value for path in evidence_paths(item)]
    if isinstance(value, dict):
        return evidence_paths(value.get('path'))
    return []


def package_check(data, prefix=''):
    result = {'entries': 0, 'xmlParts': 0, 'duplicateNames': [], 'errors': []}
    try:
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            result['duplicateNames'] = [prefix + name for name, count in Counter(archive.namelist()).items() if count > 1]
            for entry in archive.infolist():
                if entry.is_dir():
                    continue
                name = prefix + entry.filename
                result['entries'] += 1
                try:
                    content = archive.read(entry)
                    if len(content) != entry.file_size:
                        raise ValueError('Uncompressed length differs from directory entry.')
                    if entry.filename.endswith(('.xml', '.rels')):
                        ET.fromstring(content)
                        result['xmlParts'] += 1
                    if entry.filename.endswith(('.docx', '.xlsx', '.pptx')):
                        nested = package_check(content, name + '!')
                        result['entries'] += nested['entries']
                        result['xmlParts'] += nested['xmlParts']
                        result['duplicateNames'].extend(nested['duplicateNames'])
                        result['errors'].extend(nested['errors'])
                except Exception as error:
                    result['errors'].append({'part': name, 'error': str(error)})
    except Exception as error:
        result['errors'].append({'part': prefix or '(package)', 'error': str(error)})
    return result


def revision_counts(path):
    counts = Counter()
    with zipfile.ZipFile(path) as archive:
        for name in archive.namelist():
            if name.startswith('word/') and name.endswith('.xml'):
                for element in ET.fromstring(archive.read(name)).iter():
                    if element.tag.startswith('{' + W + '}'):
                        local = element.tag.rsplit('}', 1)[-1]
                        if local in REVISION_NAMES:
                            counts[local] += 1
    return dict(counts)


def included_files(root):
    excluded_names = {'edited3-current-item.json'}
    return {path.relative_to(root).as_posix(): path for path in root.rglob('*')
            if path.is_file() and not path.name.startswith('~$') and path.name not in excluded_names
            and not {'_control', '__pycache__'}.intersection(path.relative_to(root).parts)}


def markdown_file_rows(path):
    if not path.is_file():
        return []
    rows = []
    for line in path.read_text(encoding='utf-8-sig').splitlines():
        if line.startswith('|'):
            cells = line.split('|')
            first = cells[1].strip().strip('`')
            if first.endswith('.docx'):
                rows.append(first)
    return rows


def valid_toggle_reading(row):
    samples = row.get('samples', [])
    if len(samples) != 2 or samples[0].get('raw') != samples[1].get('raw') or row.get('consistent') is not True or row.get('error'):
        return False
    raw = samples[0].get('raw')
    if row.get('property') == 'HeaderDefault':
        expected = raw.rstrip('\r\n\x07') if isinstance(raw, str) else None
    else:
        expected = True if raw == -1 else False if raw == 0 else None
    return expected is not None and row.get('desktopWord') == expected and all(sample.get('value') == expected for sample in samples)


def inspect_d(path):
    with zipfile.ZipFile(path) as archive:
        names = set(archive.namelist())
        document = ET.fromstring(archive.read('word/document.xml'))
        if path.name == 'ink-to-shape-2.docx':
            wps = 'http://schemas.microsoft.com/office/word/2010/wordprocessingShape'
            w14 = 'http://schemas.microsoft.com/office/word/2010/wordml'
            drawing = 'http://schemas.openxmlformats.org/drawingml/2006/main'
            shapes = len(document.findall('.//{' + wps + '}wsp'))
            ink = len(document.findall('.//{' + w14 + '}contentPart'))
            presets = [e.get('prst') for e in document.findall('.//{' + drawing + '}prstGeom')]
            return {'wpsShapeCount': shapes, 'contentPartCount': ink, 'presetGeometries': presets,
                    'requiredStructureMet': bool(shapes and not ink and {'ellipse', 'flowChartConnector'}.intersection(presets)),
                    'limit': 'Package structure cannot prove a one-stroke gesture or actual Ink to Shape UI operation.'}
        if 'word/commentsExtended.xml' not in names:
            return {'requiredStructureMet': False, 'error': 'commentsExtended.xml missing'}
        extension = ET.fromstring(archive.read('word/commentsExtended.xml'))
        records = [{'id': e.get('{' + W15 + '}paraId'), 'parent': e.get('{' + W15 + '}paraIdParent'), 'done': e.get('{' + W15 + '}done')}
                   for e in extension.findall('{' + W15 + '}commentEx')]
        by_id = {r['id']: r for r in records}
        depths, cycles, missing = {}, [], []
        for record in records:
            current, seen, depth = record, set(), 0
            while current.get('parent'):
                parent = current['parent']
                if parent in seen:
                    cycles.append(record['id'])
                    break
                seen.add(parent)
                if parent not in by_id:
                    missing.append(parent)
                    break
                depth += 1
                current = by_id[parent]
            depths[record['id']] = depth
        roots = [r for r in records if not r['parent']]
        resolved_roots = [r['id'] for r in roots if r['done'] in ('1', 'true')]
        max_depth = max(depths.values(), default=0)
        return {'comments': records, 'rootCount': len(roots), 'depths': depths, 'maximumReplyDepth': max_depth,
                'resolvedRoots': resolved_roots, 'parentCycles': cycles, 'missingParentIds': sorted(set(missing)),
                'requiredStructureMet': len(roots) == 3 and max_depth >= 2 and bool(resolved_roots) and not cycles and not missing,
                'limit': 'Package structure cannot prove replies were entered through native Word UI buttons.'}


def audit(args):
    root, work = args.root.resolve(), args.work.resolve()
    output = args.output or (work / 'final-package-audit.json' if args.package else work / 'final-independent-audit.json')
    output = output.resolve()
    if output.suffix.lower() != '.json' or args.package and output == args.package.resolve():
        raise ValueError('Audit output must be a separate .json report, never an input or delivery ZIP.')
    if (args.final or args.package) and output.is_relative_to(root):
        raise ValueError('Final audit output must be outside the delivery tree so packaging cannot invalidate the audited bytes.')
    checks = []
    report = {'auditedAt': datetime.now(timezone.utc).isoformat(), 'deliveryRoot': str(root), 'finalCountsRequired': args.final,
              'method': 'Read-only hashes, structured JSON/XML/ZIP parsing, PDF page parsing and image decoding. No Word or application-control calls.',
              'validationLimit': 'Integrity gates do not prove a native UI action, visual correctness or complete OOXML schema validity. Claimed observations and unmet requirements remain separately attributed.'}

    def gate(name, passed, *, final_only=False, details=None):
        status = 'passed' if passed else ('pending' if final_only and not args.final else 'failed')
        item = {'check': name, 'status': status}
        if details is not None:
            item['details'] = details
        checks.append(item)

    environment_path = root / 'environment.json'
    environment = read_json(environment_path, {})
    restoration_path = root / '_readouts/settings-restored.json'
    restoration = read_json(restoration_path, {})
    expected_settings = environment.get('originalSettings', {})
    restoration_checks = restoration.get('checks', [])
    environment_hash = file_hash(environment_path)
    restoration_valid = bool(expected_settings and len(restoration_checks) == len(expected_settings)
        and {item.get('name') for item in restoration_checks} == set(expected_settings)
        and restoration.get('originalSettings') == expected_settings
        and restoration.get('actualSettings') == expected_settings
        and all(item.get('matches') is True and item.get('expected') == expected_settings[item['name']]
                and item.get('actual') == expected_settings[item['name']]
                and not item.get('setError') and not item.get('readError') for item in restoration_checks)
        and restoration.get('environmentSha256Before') == environment_hash
        and restoration.get('environmentSha256After') == environment_hash
        and restoration.get('allSettingsRestored') is True and restoration.get('environmentUnchanged') is True
        and restoration.get('pendingSetting') is None and restoration.get('completed'))
    cleanup_valid = bool(restoration_valid and restoration.get('remainingDocuments') == 0
                         and restoration.get('wordQuit') is True and not restoration.get('cleanupErrors'))
    report['environmentRestoration'] = {'file': relative(root, restoration_path), 'sha256': file_hash(restoration_path),
        'environmentSha256': environment_hash, 'expectedSettings': len(expected_settings), 'checkedSettings': len(restoration_checks),
        'settingsVerified': restoration_valid, 'taskDocumentCleanupVerified': cleanup_valid,
        'remainingDocuments': restoration.get('remainingDocuments'), 'wordQuit': restoration.get('wordQuit'),
        'scope': 'Recorded task Word instance only; unrelated pre-existing Word instances are outside this cleanup.'}
    gate('Original Word settings restored and verified against unchanged environment record', restoration_valid, final_only=True)
    gate('Task Word documents closed and its instance quit without cleanup errors', cleanup_valid, final_only=True)

    initial = read_json(root / '_scripts/round3-input-audit.json', {})
    input_root = work / 'real-word-round3-inputs'
    input_zip = args.input_zip
    source_rows = []
    with zipfile.ZipFile(input_zip, metadata_encoding='utf-8') as archive:
        for entry in archive.infolist():
            if entry.is_dir() or not entry.filename.endswith('.docx'):
                continue
            relative_input = entry.filename.removeprefix('real-word-round3-inputs/')
            source = input_root / relative_input
            expected = digest(archive.read(entry))
            actual = file_hash(source)
            source_rows.append({'file': relative_input, 'archiveSha256': expected, 'actualSha256': actual, 'unchanged': expected == actual})
    baseline_rows = []
    for item in initial.get('baselineMap', []):
        candidates = item.get('candidates', [])
        row = {'base': item['base'], 'resolved': len(candidates) == 1}
        if len(candidates) == 1:
            candidate = candidates[0]
            row.update(path=candidate['path'], initialSha256=candidate['sha256'], actualSha256=file_hash(Path(candidate['path'])))
            row['unchanged'] = row['initialSha256'] == row['actualSha256']
        baseline_rows.append(row)
    report['inputs'] = {'inputZipSha256': file_hash(input_zip), 'initialInputZipSha256': initial.get('inputZipSha256'), 'docxCount': len(source_rows), 'unchangedDocx': sum(r['unchanged'] for r in source_rows), 'files': source_rows}
    report['baselines'] = {'expected': 180, 'count': len(baseline_rows), 'unchanged': sum(r.get('unchanged', False) for r in baseline_rows), 'files': baseline_rows}
    gate('1560 input DOCX unchanged against supplied archive', len(source_rows) == 1560 and all(r['unchanged'] for r in source_rows))
    gate('Input ZIP unchanged against initial audit', report['inputs']['inputZipSha256'] == initial.get('inputZipSha256'))
    gate('180 preserved baseline DOCX unchanged against initial audit', len(baseline_rows) == 180 and all(r.get('unchanged') for r in baseline_rows))

    a_rows = []
    for case, stage in A_EXPECTED:
        path = root / 'revfix' / case / (stage + '.docx')
        log_path = root / '_readouts' / ('a-' + case + '-' + stage + '.json')
        log = read_json(log_path, {})
        actual = file_hash(path)
        row = {'case': case, 'stage': stage, 'file': relative(root, path), 'exists': path.is_file(), 'logExists': bool(log), 'sha256': actual,
               'firstSaveSha256': log.get('sha256'), 'firstSaveHashMatches': bool(actual and actual == log.get('sha256'))}
        source_stage = 'tracked' if stage in ('accepted', 'rejected') else 'base' if stage == 'tracked' else 'before' if stage == 'after' else None
        if source_stage:
            source = root / 'revfix' / case / (source_stage + '.docx')
            row.update(sourceFile=relative(root, source), sourceSha256=file_hash(source), recordedSourceSha256=log.get('sourceSha256'),
                       recordedSourcePath=log.get('sourcePath'), sourceHashMatches=bool(log.get('sourceSha256') and file_hash(source) == log.get('sourceSha256')),
                       sourcePathMatches=bool(log.get('sourcePath') and local_path(root, log['sourcePath']).resolve() == source.resolve()))
        if actual and stage != 'tracked':
            row['remainingRevisionMarkers'] = revision_counts(path)
        row['uiScreenshots'] = [relative(root, p) for p in (root / 'screenshots').glob('a-' + case + '-' + stage + '-*') if p.suffix.lower() in ('.png', '.jpg', '.jpeg')]
        a_rows.append(row)
    a_inspection = read_json(root / '_readouts/revfix-inspection.json', {})
    a_review_errors = []
    inspection_by_key = {r['key']: r for r in a_inspection.get('rows', [])}
    for case, stage in A_EXPECTED:
        key = case + '/' + stage
        row = inspection_by_key.get(key)
        if not row:
            a_review_errors.append({'case': key, 'error': 'Independent structure/PDF review absent'})
            continue
        actual = file_hash(root / 'revfix' / case / (stage + '.docx'))
        if actual != row.get('structure', {}).get('sha256'):
            a_review_errors.append({'case': key, 'error': 'Independent structure review hash is stale'})
        pdf = row.get('pdf', {})
        pdf_path = local_path(root, pdf.get('path', '_missing'))
        annotation = pdf.get('visualReview', {})
        if not pdf_path.is_file() or file_hash(pdf_path) != pdf.get('sha256'):
            a_review_errors.append({'case': key, 'error': 'Independent PDF review hash is absent or stale'})
        if not pdf.get('visualReviewed') or annotation.get('pdfSha256') != pdf.get('sha256') or sorted(annotation.get('pageNumbers', [])) != list(range(1, pdf.get('pageCount', 0) + 1)):
            a_review_errors.append({'case': key, 'error': 'Current visual annotation does not cover every PDF page'})
        for page in pdf.get('pages', []):
            image_path = local_path(root, page.get('png', '_missing'))
            if file_hash(image_path) != page.get('pngSha256'):
                a_review_errors.append({'case': key, 'error': 'Rendered PDF page evidence is absent or stale'})
    a_requirements = [dict(case=case['case'], **check) for case in a_inspection.get('cases', []) for check in case.get('checks', []) if not check.get('passed')]
    report['taskA'] = {'expectedFiles': 24, 'filesPresent': sum(r['exists'] for r in a_rows), 'firstSaveMatches': sum(r['firstSaveHashMatches'] for r in a_rows),
                       'sameTrackedBranchPairs': sum(all(next(r for r in a_rows if r['case'] == case and r['stage'] == stage).get('sourceHashMatches') and next(r for r in a_rows if r['case'] == case and r['stage'] == stage).get('sourcePathMatches') for stage in ('accepted', 'rejected')) for case in TRIPLES),
                       'independentReviewErrors': a_review_errors, 'unmetRequirements': a_requirements, 'files': a_rows,
                       'provenanceLimit': 'Authoring logs and original first-save hashes support separate branches from one tracked file; package bytes alone cannot prove the Word commands.'}
    actual_a_names = {p.relative_to(root / 'revfix').as_posix() for p in (root / 'revfix').rglob('*.docx') if not p.name.startswith('~$')}
    expected_a_names = {case + '/' + stage + '.docx' for case, stage in A_EXPECTED}
    report['taskA']['unexpectedCanonicalFiles'] = sorted(actual_a_names - expected_a_names)
    gate('Existing Task A first-save hashes and source links remain current', all(r['firstSaveHashMatches'] and (not r.get('sourceFile') or r.get('sourceHashMatches') and r.get('sourcePathMatches')) for r in a_rows if r['exists']))
    gate('24 Task A files and four same-tracked accepted/rejected pairs', actual_a_names == expected_a_names and report['taskA']['firstSaveMatches'] == 24 and report['taskA']['sameTrackedBranchPairs'] == 4, final_only=True)
    gate('Task A nontracked outputs contain zero revision markers', all(not r.get('remainingRevisionMarkers') for r in a_rows))
    gate('24 Task A screenshot and current independent PDF reviews', all(r['uiScreenshots'] for r in a_rows) and len(inspection_by_key) == 24 and not a_review_errors, final_only=True)
    gate('All Task A recorded structural requirements pass', len(a_inspection.get('cases', [])) == 8 and not a_requirements, final_only=True)

    manifest = read_json(input_root / 'edited/manifest.json', [])
    generated = [r for r in manifest if r.get('status') == 'generated']
    expected_b = {r['file']: r for r in generated}
    b_rows = as_rows(read_json(root / 'edited3-results.json', []))
    b_counts = Counter(r['file'] for r in b_rows)
    b_unknown = sorted(b_counts.keys() - expected_b.keys())
    b_duplicates = sorted(name for name, count in b_counts.items() if count > 1)
    b_wrong_manifest = [r['file'] for r in b_rows if r['file'] in expected_b and (r.get('base') != expected_b[r['file']]['base'] or r.get('op') != expected_b[r['file']]['op'])]
    b_plan = read_json(root / '_scripts/edited3-plan.json', {})
    planned = {r['file'] for r in b_plan.get('selected', [])}
    ui_rows = as_rows(read_json(root / 'ui-edited3.json', []))
    ui_counts = Counter(r['file'] for r in ui_rows)
    ui_names = set(ui_counts)
    # A recorded read/close error still warrants a UI review even when open='ok'.
    error_names = {r['file'] for r in b_rows if r.get('open') != 'ok' or r.get('error') or r.get('close_error') or r.get('object_model_assessment') == 'error'}
    missing_evidence, incomplete_ui, resaves, cleanup_issues, chart_issues = [], [], [], [], []
    for row in ui_rows:
        screenshots = evidence_paths(row.get('screenshot'))
        if not screenshots or not str(row.get('observation', '')).strip() or not str(row.get('recovery', '')).strip() or 'consistent' not in row:
            incomplete_ui.append(row['file'])
        for field in ('screenshot', 'pdf', 'resaved', 'object_model_readout'):
            for name in evidence_paths(row.get(field)):
                path = local_path(root, name)
                if not path.is_file():
                    missing_evidence.append({'file': row['file'], 'kind': field, 'path': name})
                if field == 'resaved':
                    resaves.append({'file': row['file'], 'path': name, 'sha256': file_hash(path), 'inputSha256': file_hash(input_root / 'edited' / row['file'])})
    for row in b_rows:
        base = row.get('baseline') or {}
        geometry_errors = [e for shape in row.get('shape_geometry', []) for e in shape.get('errors', [])]
        diagnostics = {key: value for key, value in {'documentClose': row.get('close_error'), 'baselineRead': base.get('error'), 'baselineClose': base.get('close_error'), 'geometry': geometry_errors}.items() if value}
        if diagnostics:
            cleanup_issues.append({'file': row['file'], 'assessment': row.get('object_model_assessment'), 'diagnostics': diagnostics})
        for chart in row.get('charts', []):
            activation = chart.get('activation')
            if row.get('op') == 'chartdata' and activation == 'ok' and (not chart.get('before') or not chart.get('after') or not chart.get('workbook_name')):
                chart_issues.append({'file': row['file'], 'index': chart.get('index'), 'error': 'Successful activation lacks before/after/workbook provenance'})
            if chart.get('workbook_close_error'):
                cleanup_issues.append({'file': row['file'], 'diagnostics': {'workbookClose': chart['workbook_close_error']}})
    b_markdown = markdown_file_rows(root / 'EDITED3.md')
    report['taskB'] = {'expected': 1544, 'rows': len(b_rows), 'markdownRows': len(b_markdown), 'missingFiles': sorted(expected_b.keys() - b_counts.keys()),
                       'unexpectedFiles': b_unknown, 'duplicateFiles': b_duplicates, 'manifestDisagreements': b_wrong_manifest,
                       'openCounts': dict(Counter(r.get('open') for r in b_rows)), 'assessmentCounts': dict(Counter(r.get('object_model_assessment') for r in b_rows)),
                       'plannedUi': len(planned), 'uiRows': len(ui_rows), 'missingPlannedUi': sorted(planned - ui_names), 'errorFiles': sorted(error_names), 'errorsWithoutUi': sorted(error_names - ui_names),
                       'incompleteUiRecords': incomplete_ui, 'missingUiEvidence': missing_evidence, 'recordedResaves': resaves,
                       'cleanupAndGeometryDiagnostics': cleanup_issues, 'chartProvenanceIssues': chart_issues,
                       'nongeneratedManifestRows': [r for r in manifest if r.get('status') != 'generated'], 'declaredUnavailableSample': 'No chartdata derivative among the17 M7 baselines; supplied plan has60 unique UI cases.'}
    gate('Present B records have unique exact manifest identity', not b_unknown and not b_duplicates and not b_wrong_manifest)
    gate('1544 B JSON and Markdown rows cover all generated inputs', len(b_rows) == 1544 and len(b_markdown) == 1544 and set(b_markdown) == set(expected_b) and not report['taskB']['missingFiles'], final_only=True)
    gate('Present B UI evidence exists and records are unique', not missing_evidence and all(count == 1 for count in ui_counts.values()) and not (ui_names - expected_b.keys()))
    gate('60 planned B cases and every recorded error have complete actual UI observations', len(planned) == 60 and not (planned - ui_names) and not (error_names - ui_names) and not incomplete_ui, final_only=True)
    merged_ui_names = {r['file'] for r in b_rows if r.get('ui_checked') is True}
    gate('B UI observations merged exactly into final JSON', merged_ui_names == ui_names, final_only=True)
    gate('Chart activation records retain preactivation and subsequent provenance', not chart_issues)

    c_summary = read_json(root / '_scripts/task-c-summary.json', {})
    c_rows = as_rows(c_summary)
    c_reference = read_json(root / '_scripts/toggle-round2-reference.json', {})
    c_expected = {(r['file'], r['sentence'], r['property']) for r in c_reference.get('rows', [])}
    c_counts = Counter((r.get('baseFile'), r.get('sentence'), r.get('property')) for r in c_rows)
    c_readouts = [read_json(p, {}) for p in (root / '_scripts/toggle15-read').glob('*.json')]
    c_converted = [r for d in c_readouts if d.get('variant') == 'word-converted' for r in d.get('rows', [])]
    c_hash_errors = []
    current_reference_hash = file_hash(root / '_scripts/toggle-round2-reference.json')
    for d in c_readouts:
        current = file_hash(local_path(root, d.get('fullName', '_missing')))
        if not current or current != d.get('sourceSha256') or current != d.get('sourceSha256AfterRead') or d.get('referenceSha256') != current_reference_hash:
            c_hash_errors.append(d.get('file'))
    raw_primary = {(r.get('baseFile'), r.get('sentence'), r.get('property')): r for d in c_readouts if d.get('variant') == 'supplied-compat15' for r in d.get('rows', [])}
    c_summary_mismatches = []
    for row in c_rows:
        key = (row.get('baseFile'), row.get('sentence'), row.get('property'))
        raw = raw_primary.get(key)
        if not raw or any(row.get(field) != raw.get(field) for field in ('samples', 'desktopWord', 'compatibilityMode', 'consistent', 'error')):
            c_summary_mismatches.append(key)
    c_missing_evidence = []
    c_dialogs = []
    for row in c_rows:
        for e in row.get('uiEvidence', []):
            if not local_path(root, e.get('path', '_missing')).is_file():
                c_missing_evidence.append(e.get('path'))
            if row.get('sentence') in ('strike twice', 'dstrike twice', 'vanish once') and e.get('kind') == 'font-dialog':
                c_dialogs.append(row['sentence'])
    converted_path = root / '_resaved/toggle-other-toggles-converted.docx'
    converted_expected = {r['sentence'] for r in c_reference.get('rows', []) if r['file'] == 'toggle-other-toggles.docx'}
    conversion_record = read_json(root / '_readouts/task-c-conversion.json', {})
    conversion_source = input_root / 'fixtures/toggle-other-toggles.docx'
    conversion_evidence = evidence_paths(conversion_record.get('uiEvidence'))
    conversion_provenance_ok = bool(conversion_record and conversion_record.get('sourceCompatibilityMode') == 12
        and conversion_record.get('convertedCompatibilityMode') == 15
        and conversion_record.get('sourceSha256') == file_hash(conversion_source)
        and conversion_record.get('convertedSha256') == file_hash(converted_path)
        and conversion_record.get('sourcePath') and local_path(root, conversion_record['sourcePath']).resolve() == conversion_source.resolve()
        and conversion_record.get('convertedPath') and local_path(root, conversion_record['convertedPath']).resolve() == converted_path.resolve()
        and conversion_record.get('method') and conversion_evidence and all(local_path(root, p).is_file() for p in conversion_evidence))
    report['taskC'] = {'rows': len(c_rows), 'markdownRows': len(markdown_file_rows(root / 'TOGGLE15.md')), 'rawReadoutFiles': len(c_readouts),
                       'samples': sum(len(r.get('samples', [])) for r in c_rows), 'missingRows': sorted(c_expected - c_counts.keys()),
                       'unexpectedRows': sorted(c_counts.keys() - c_expected), 'duplicateRows': [key for key, count in c_counts.items() if count > 1],
                       'sourceHashErrors': c_hash_errors, 'missingUiEvidence': c_missing_evidence, 'requiredFontDialogs': sorted(set(c_dialogs)),
                       'summaryDisagreesWithRawReadouts': c_summary_mismatches,
                       'convertedRows': len(c_converted), 'convertedDocxSha256': file_hash(converted_path),
                       'conversionOperationRecordPresent': bool(conversion_record), 'conversionProvenanceCurrent': conversion_provenance_ok,
                       'allThreeAgreementRows': c_summary.get('allThreeAgreementRows'), 'declaredIncomplete': c_summary.get('incomplete', [])}
    gate('Existing C source hashes and evidence current', not c_hash_errors and not c_missing_evidence)
    gate('25 C points have two consistent mode15 readings and GUI evidence', len(c_rows) == 25 and set(c_counts) == c_expected and len(c_counts) == 25
         and not c_summary_mismatches and report['taskC']['markdownRows'] == 25 and all(r.get('compatibilityMode') == 15 and valid_toggle_reading(r) and r.get('uiVerified') is True for r in c_rows), final_only=True)
    gate('Three required C Font dialogs recorded', set(c_dialogs) == {'strike twice', 'dstrike twice', 'vanish once'}, final_only=True)
    gate('Word-converted C DOCX and12 precise cross-check points present', converted_path.is_file() and len(c_converted) == 12 and {r['sentence'] for r in c_converted} == converted_expected and all(r.get('compatibilityMode') == 15 and valid_toggle_reading(r) for r in c_converted), final_only=True)
    gate('C conversion operation has current mode12/source and mode15/output provenance', conversion_provenance_ok, final_only=True)
    c_review = read_json(root / '_scripts/task-c-independent-review.json', {})
    c_review_current = bool(c_review.get('passed') and c_review.get('reportSha256') == file_hash(root / 'TOGGLE15.md')
        and c_review.get('summarySha256') == file_hash(root / '_scripts/task-c-summary.json')
        and c_review.get('conversionRecordSha256') == file_hash(root / '_readouts/task-c-conversion.json')
        and all(file_hash(local_path(root, item['file'])) == item['sha256'] for item in c_review.get('evidence', [])))
    report['taskC']['independentReviewCurrent'] = c_review_current
    gate('Independent C review remains bound to current report, readings and screenshots', c_review_current, final_only=True)

    d_records = as_rows(read_json(root / '_scripts/task-d-results.json', []))
    d_by_key = {}
    for row in d_records:
        for key in ('file', 'case', 'path'):
            if row.get(key):
                value = str(row[key]).replace('\\', '/')
                d_by_key[value] = row
                d_by_key[PurePosixPath(value).stem] = row
    readme = (root / 'README.md').read_text(encoding='utf-8-sig') if (root / 'README.md').is_file() else ''
    d_rows = []
    for name in D_EXPECTED:
        path = root / name
        record = d_by_key.get(name) or d_by_key.get(PurePosixPath(name).stem) or {}
        limitation_values = [record.get(key) for key in ('limitation', 'unmetRequirements', 'incomplete', 'limitations', 'error') if record.get(key)]
        limitation = json.dumps(limitation_values, ensure_ascii=False) if limitation_values else ''
        row = {'file': name, 'exists': path.is_file(), 'sha256': file_hash(path), 'recordPresent': bool(record), 'status': record.get('status'), 'declaredLimitation': limitation,
               'limitationMentionedInReadme': bool(limitation and (PurePosixPath(name).stem in readme or name in readme))}
        if path.is_file():
            row['structure'] = inspect_d(path)
            recorded_hash = record.get('sha256') or record.get('firstSaveSha256')
            row['firstSaveHashMatches'] = bool(recorded_hash and recorded_hash.upper() == row['sha256'])
        paths = evidence_paths(record.get('uiEvidence') or record.get('screenshot') or record.get('screenshots'))
        row['uiEvidenceExists'] = bool(paths) and all(local_path(root, p).is_file() for p in paths)
        row['missingUiEvidence'] = [p for p in paths if not local_path(root, p).is_file()]
        row['trials'] = []
        for trial in record.get('trials', []):
            trial_record = {'file': trial} if isinstance(trial, str) else trial
            trial_name = trial_record.get('file') or trial_record.get('path')
            if not trial_name:
                row['trials'].append({'file': None, 'validClaimedArtifact': False, 'error': 'Trial record has no file/path'})
                continue
            trial_path = local_path(root, trial_name)
            actual_trial_hash = file_hash(trial_path)
            recorded_trial_hash = trial_record.get('sha256')
            row['trials'].append({'file': trial_name, 'sha256': actual_trial_hash, 'recordedSha256': recorded_trial_hash,
                                 'validClaimedArtifact': bool(actual_trial_hash and (not recorded_trial_hash or actual_trial_hash == recorded_trial_hash.upper()))})
        row['requirementHandled'] = bool(row.get('structure', {}).get('requiredStructureMet') and row.get('firstSaveHashMatches') and row['uiEvidenceExists'] or row['limitationMentionedInReadme'])
        d_rows.append(row)
    report['taskD'] = {'expected': 2, 'rows': d_rows, 'declaredUnmet': [r['file'] for r in d_rows if r['declaredLimitation']],
                       'interpretation': 'A documented unmet native Word behavior is accepted as a disclosed limitation, not relabeled a successful structure result.'}
    d_inspection_path = root / '_readouts/task-d-inspection.json'
    d_inspection = read_json(d_inspection_path, {})
    d_inspection_rows = {r['file']: r for r in d_inspection.get('rows', [])}
    for row in d_rows:
        independent = d_inspection_rows.get(row['file'], {})
        row['independentInspectionCurrent'] = bool(independent and independent.get('sha256') == row['sha256'])
        row['independentRequestedStructureMet'] = independent.get('requiredStructureMet')
        if not row['limitationMentionedInReadme']:
            row['requirementHandled'] = bool(row['requirementHandled'] and row['independentInspectionCurrent']
                                            and row['independentRequestedStructureMet'] is True)
    report['taskD']['independentInspection'] = relative(root, d_inspection_path)
    gate('Two D cases have actual native artifacts/evidence or explicit per-case limitations', all(r['requirementHandled'] for r in d_rows), final_only=True)
    gate('Present canonical D files match their recorded first-save hashes', all(r.get('firstSaveHashMatches') for r in d_rows if r['exists']), final_only=True)
    gate('All claimed D UI evidence and preserved trials exist with any recorded hashes intact', all(not r['missingUiEvidence'] and all(t['validClaimedArtifact'] for t in r['trials']) for r in d_rows))
    gate('Present D artifacts have independent inspections at their current hashes', all(r['independentInspectionCurrent'] for r in d_rows if r['exists']), final_only=True)

    artifact_counts, artifact_errors, artifact_docx = Counter(), [], []
    intended = included_files(root)
    for name, path in intended.items():
        if path.resolve() == output:
            continue
        try:
            suffix = path.suffix.lower()
            if suffix == '.docx':
                package = package_check(path.read_bytes())
                artifact_docx.append({'file': name, 'sha256': file_hash(path), 'xmlParts': package['xmlParts'], 'entries': package['entries']})
                artifact_errors.extend({'file': name, 'error': error} for error in package['errors'])
                artifact_errors.extend({'file': name, 'error': 'duplicate package part: ' + part} for part in package['duplicateNames'])
                artifact_counts['docx'] += 1
            elif suffix == '.json':
                read_json(path)
                artifact_counts['json'] += 1
            elif suffix == '.jsonl':
                for number, line in enumerate(path.read_text(encoding='utf-8-sig').splitlines(), 1):
                    if line.strip():
                        try:
                            json.loads(line)
                        except Exception as error:
                            raise ValueError(f'JSONL line{number}: {error}') from error
                artifact_counts['jsonl'] += 1
            elif suffix == '.pdf':
                pdf = pdfium.PdfDocument(path)
                try:
                    if not len(pdf):
                        raise ValueError('Empty PDF')
                    for page_number in range(len(pdf)):
                        page = pdf[page_number]
                        try:
                            if min(page.get_size()) <= 0:
                                raise ValueError('Invalid PDF page geometry')
                        finally:
                            page.close()
                finally:
                    pdf.close()
                artifact_counts['pdf'] += 1
            elif suffix in ('.png', '.jpg', '.jpeg'):
                with Image.open(path) as image:
                    image.verify()
                artifact_counts['images'] += 1
        except Exception as error:
            artifact_errors.append({'file': name, 'error': str(error)})
    non_pdf_previews = [relative(root, p) for p in (root / '_previews').rglob('*') if p.is_file() and p.suffix.lower() != '.pdf']
    all_resaves = {name for name in intended if name.startswith('_resaved/') and name.lower().endswith('.docx')}
    referenced_resaves = {relative(root, local_path(root, r['path'])) for r in resaves}
    if converted_path.is_file():
        referenced_resaves.add(relative(root, converted_path))
    file_inventory = [{'file': name, 'bytes': path.stat().st_size, 'sha256': file_hash(path)} for name, path in sorted(intended.items())]
    report['artifacts'] = {'counts': dict(artifact_counts), 'includedFiles': len(file_inventory),
                           'includedBytes': sum(item['bytes'] for item in file_inventory), 'fileInventory': file_inventory,
                           'errors': artifact_errors, 'docxFiles': artifact_docx, 'nonPdfPreviewFiles': non_pdf_previews,
                           'resavedCount': len(all_resaves), 'unreferencedResaves': sorted(all_resaves - referenced_resaves),
                           'excludedLockFiles': [relative(root, p) for p in root.rglob('~$*') if p.is_file()],
                           'packageExclusions': ['Any _control directory', 'Any __pycache__ directory', 'Any filename beginning~$', 'edited3-current-item.json']}
    gate('All included DOCX ZIP/XML, JSON, images and PDF artifacts valid', not artifact_errors)
    gate('_previews contains only PDFs', not non_pdf_previews)
    gate('Every resave is referenced by B evidence or C conversion', not (all_resaves - referenced_resaves), final_only=True)
    gate('Required final reports exist', all((root / name).is_file() for name in ('README.md', 'REVISIONS.md', 'EDITED3.md', 'edited3-results.json', 'TOGGLE15.md', 'environment.json')), final_only=True)

    if args.package:
        intended = included_files(root)
        package_errors, package_rows, member_names = [], [], []
        with zipfile.ZipFile(args.package, metadata_encoding='utf-8') as archive:
            entries = [entry for entry in archive.infolist() if not entry.is_dir()]
            prefix = root.name + '/'
            names = [entry.filename.replace('\\', '/') for entry in entries]
            prefixed = bool(names) and all(name.startswith(prefix) for name in names)
            for entry, full_name in zip(entries, names):
                name = full_name[len(prefix):] if prefixed else full_name
                member_names.append(name)
                try:
                    if name.startswith('/') or '..' in PurePosixPath(name).parts:
                        raise ValueError('Unsafe package member path')
                    content = archive.read(entry)
                    local = intended.get(name)
                    same = bool(local and digest(content) == file_hash(local))
                    package_rows.append({'file': name, 'sha256': digest(content), 'matchesDelivery': same})
                    if not same:
                        package_errors.append({'file': name, 'error': 'Missing from intended delivery or byte mismatch'})
                except Exception as error:
                    package_errors.append({'file': name, 'error': str(error)})
        member_counts = Counter(member_names)
        report['package'] = {'file': str(args.package), 'sha256': file_hash(args.package), 'files': len(member_names), 'intendedFiles': len(intended),
                             'requiredRootPrefix': prefix, 'rootPrefixValid': prefixed,
                             'missingFiles': sorted(intended.keys() - member_counts.keys()), 'unexpectedFiles': sorted(member_counts.keys() - intended.keys()),
                             'duplicateNames': [name for name, count in member_counts.items() if count > 1], 'errors': package_errors,
                             'lockFiles': [name for name in member_names if PurePosixPath(name).name.startswith('~$')], 'members': package_rows}
        gate('Delivery ZIP matches every included byte, has valid CRCs, required root prefix and no lockfiles', prefixed
             and not any(report['package'][key] for key in ('missingFiles', 'unexpectedFiles', 'duplicateNames', 'errors', 'lockFiles')))

    report['checks'] = checks
    report['failedChecks'] = [c['check'] for c in checks if c['status'] == 'failed']
    report['pendingChecks'] = [c['check'] for c in checks if c['status'] == 'pending']
    report['auditPassed'] = not report['failedChecks']
    report['finalDeliveryComplete'] = args.final and report['auditPassed']
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({'report': str(output), 'auditPassed': report['auditPassed'], 'finalDeliveryComplete': report['finalDeliveryComplete'], 'failedChecks': report['failedChecks'], 'pendingChecks': report['pendingChecks'], 'artifactCounts': dict(artifact_counts)}, ensure_ascii=True, indent=2))
    return 0 if report['auditPassed'] else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=DEFAULT_ROOT)
    parser.add_argument('--work', type=Path, default=DEFAULT_WORK)
    parser.add_argument('--input-zip', type=Path, default=Path('C:/word/real-word-round3-inputs-20260907.zip'))
    parser.add_argument('--final', action='store_true', help='Require all final phase counts and evidence. Without this, missing phases are pending.')
    parser.add_argument('--package', type=Path, help='Validate completed delivery ZIP against included files. Output must remain outside delivery.')
    parser.add_argument('--output', type=Path)
    sys.exit(audit(parser.parse_args()))
