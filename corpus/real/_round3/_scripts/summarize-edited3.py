"""Offline monitor for append-only Word batch evidence; never accesses Word."""
import argparse
from collections import Counter, defaultdict
from datetime import datetime, timedelta, timezone
import json
from pathlib import Path
import statistics
import xml.etree.ElementTree as ET
import zipfile


def array(value):
    if value is None:
        return []
    return value if isinstance(value, list) else [value]


def mapping(value):
    return value if isinstance(value, dict) else {}


def read_json(path, default=None):
    try:
        return json.loads(path.read_text(encoding='utf-8-sig'))
    except (FileNotFoundError, json.JSONDecodeError):
        return default


def read_jsonl(path, key):
    latest, history, invalid = {}, defaultdict(list), []
    if not path.exists():
        return latest, history, invalid
    data = path.read_bytes()
    raw_lines = data.splitlines(keepends=True)
    for number, raw in enumerate(raw_lines, 1):
        if not raw.strip():
            continue
        try:
            row = json.loads(raw.decode('utf-8-sig'))
            if not isinstance(row, dict) or not isinstance(row.get(key), str):
                raise ValueError(f'Expected an object with string {key}')
        except (ValueError, UnicodeDecodeError) as error:
            invalid.append({'line': number, 'error': str(error), 'tail_without_newline': number == len(raw_lines) and not raw.endswith(b'\n'), 'bytes': len(raw)})
            continue
        latest[row[key]] = row
        history[row[key]].append({'line': number, 'row': row})
    return latest, history, invalid


def leaf_error(error, path):
    error = mapping(error)
    return {'path': path, 'message': error.get('message', str(error)), 'hresult': error.get('hresult'), 'id': error.get('fully_qualified_error_id')}


def diagnostics(row):
    found = []
    for key in ('error', 'close_error'):
        if row.get(key) is not None:
            found.append(leaf_error(row[key], key))
    for key in ('metrics', 'checks'):
        owner = mapping(row.get(key))
        for index, error in enumerate(array(owner.get('errors'))):
            error = mapping(error)
            found.append(leaf_error(error.get('error'), f'{key}.errors[{index}].{error.get("stage", "unknown")}'))
    baseline = mapping(row.get('baseline'))
    for key in ('error', 'close_error'):
        if baseline.get(key) is not None:
            found.append(leaf_error(baseline[key], f'baseline.{key}'))
    for index, error in enumerate(array(mapping(baseline.get('metrics')).get('errors'))):
        error = mapping(error)
        found.append(leaf_error(error.get('error'), f'baseline.metrics.errors[{index}].{error.get("stage", "unknown")}'))
    for index, geometry in enumerate(array(row.get('shape_geometry'))):
        for eindex, error in enumerate(array(mapping(geometry).get('errors'))):
            error = mapping(error)
            found.append(leaf_error(error.get('error'), f'shape_geometry[{index}].errors[{eindex}].{error.get("stage", "unknown")}'))
    for owner_name, owner in (('charts', row), ('baseline.charts', baseline)):
        for cindex, chart in enumerate(array(owner.get('charts'))):
            chart = mapping(chart)
            for key in ('error', 'activation_error', 'workbook_read_error', 'workbook_close_error'):
                if chart.get(key) is not None:
                    found.append(leaf_error(chart[key], f'{owner_name}[{cindex}].{key}'))
            for phase in ('before', 'after'):
                for eindex, error in enumerate(array(mapping(chart.get(phase)).get('errors'))):
                    error = mapping(error)
                    found.append(leaf_error(error.get('error'), f'{owner_name}[{cindex}].{phase}.errors[{eindex}].{error.get("stage", "unknown")}'))
    return found


def expected_collector_assessment(row, errors):
    # This reviews the declared collector checks, not all document semantics.
    if row.get('error') is not None:
        return 'incomplete' if row.get('close_error') is not None else 'error'
    metrics, checks, baseline = (mapping(row.get(key)) for key in ('metrics', 'checks', 'baseline'))
    active = [checks[key] for key in ('marker', 'shape_check', 'chart_check', 'table_check') if isinstance(checks.get(key), dict)]
    result = 'mismatch' if metrics.get('compat') != 15 or any(check.get('matches') is False for check in active) else 'pass'
    relevant_errors = [error for error in errors if not error['path'].startswith('baseline.charts')]
    if not metrics or not checks or baseline.get('open') != 'ok' or not mapping(baseline.get('metrics')) or relevant_errors or any(check.get('matches') is None for check in active):
        return 'incomplete'
    return result


def package_has_chartex(path):
    if not path.is_file():
        return False
    try:
        with zipfile.ZipFile(path) as archive:
            for name in archive.namelist():
                if name.startswith('word/charts/') and name.endswith('.xml'):
                    if ET.fromstring(archive.read(name)).tag == '{http://schemas.microsoft.com/office/drawing/2014/chartex}chartSpace':
                        return True
    except (OSError, zipfile.BadZipFile, ET.ParseError):
        return False
    return False


def compact_row(row, errors):
    return {'file': row.get('file'), 'base': row.get('base'), 'op': row.get('op'), 'open': row.get('open'), 'assessment': row.get('object_model_assessment'), 'timestamp': row.get('timestamp'), 'errors': errors}


def drawing_inventory(path):
    result = {'path': str(path), 'method': 'Read-only XML inventory includes every MCE branch; not a Word shape count.'}
    try:
        with zipfile.ZipFile(path) as archive:
            root = ET.fromstring(archive.read('word/document.xml'))
            wp = '{http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing}'
            result['doc_pr'] = [dict(node.attrib) for node in root.iter(wp + 'docPr')]
            result['anchor_count'] = sum(1 for node in root.iter(wp + 'anchor'))
            result['inline_count'] = sum(1 for node in root.iter(wp + 'inline'))
            ids = Counter(node['id'] for node in result['doc_pr'] if 'id' in node)
            result['duplicate_doc_pr_ids'] = {key: count for key, count in ids.items() if count > 1}
    except (OSError, zipfile.BadZipFile, KeyError, ET.ParseError) as error:
        result['error'] = str(error)
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, default=Path('C:/word/real-word-round3-20260907'))
    parser.add_argument('--input', type=Path, default=Path('C:/word/round3-work-20260907/real-word-round3-inputs/edited'))
    parser.add_argument('--output', type=Path, help='Default: _scripts/edited3-diagnostics.json')
    parser.add_argument('--require-complete', action='store_true')
    args = parser.parse_args()
    generated = [row for row in read_json(args.input / 'manifest.json', []) if row.get('status') == 'generated']
    manifest = {row['file']: row for row in generated}
    if len(manifest) != 1544:
        raise ValueError(f'Expected 1544 unique generated entries, found {len(manifest)}')
    latest, history, invalid = read_jsonl(args.root / 'edited3-results.jsonl', 'file')
    baselines, baseline_history, baseline_invalid = read_jsonl(args.root / 'edited3-baselines.jsonl', 'base')
    stage = read_json(args.root / 'edited3-current-item.json', {})
    ui_rows = array(read_json(args.root / 'ui-edited3.json', []))
    ui_files = {row['file'] for row in ui_rows if isinstance(row, dict) and 'file' in row}
    observed = [], [], [], [], [], [], []
    top_level, structural_mismatches, chartex_limits, other_diagnostics, classification, metadata, silent_geometry_nulls = observed
    all_error_groups = defaultdict(lambda: {'count': 0, 'files': []})
    chart_rollbacks = []
    for file, row in latest.items():
        if file not in manifest:
            metadata.append({'file': file, 'issue': 'not in generated manifest'})
        elif any(row.get(key) != manifest[file].get(key) for key in ('base', 'op', 'expect')):
            metadata.append({'file': file, 'issue': 'base/op/expect differs from supplied manifest'})
        errors = diagnostics(row)
        for error in errors:
            scope = error['path'].split('[', 1)[0]
            fingerprint = (scope, error['message'], str(error.get('hresult')))
            group = all_error_groups[fingerprint]
            group['count'] += 1
            if file not in group['files']:
                group['files'].append(file)
        if row.get('open') != 'ok' or row.get('error') is not None or row.get('close_error') is not None:
            entry = compact_row(row, errors)
            entry['already_has_ui'] = file in ui_files
            top_level.append(entry)
        checks = mapping(row.get('checks'))
        failed = {key: value for key, value in checks.items() if isinstance(value, dict) and value.get('matches') is False}
        if failed:
            entry = {'file': file, 'op': row.get('op'), 'assessment': row.get('object_model_assessment'), 'failed_checks': failed, 'actual_metrics': row.get('metrics'), 'baseline_metrics': mapping(row.get('baseline')).get('metrics'), 'already_has_ui': file in ui_files}
            if 'shape_check' in failed:
                entry['input_drawing_xml'] = drawing_inventory(args.input / file)
                source = mapping(row.get('baseline')).get('source_path')
                if source:
                    entry['baseline_drawing_xml'] = drawing_inventory(Path(source))
            structural_mismatches.append(entry)
        getters = [error for error in errors if error['path'].startswith('charts[') and any(part in error['path'] for part in ('.chart.series_name', '.chart.first_series_values'))]
        has_chartex = bool(getters and package_has_chartex(args.input / file))
        other = [error for error in errors if error not in getters and not error['path'].startswith('baseline.charts[')]
        if has_chartex:
            entry = compact_row(row, getters)
            entry.update({'package_has_chartex': True, 'added_chart_check': checks.get('chart_check'), 'all_other_current_diagnostics_empty': not other, 'already_has_ui': file in ui_files})
            chartex_limits.append(entry)
        elif getters:
            other += getters
        if other:
            other_diagnostics.append(compact_row(row, other))
        independently_expected = expected_collector_assessment(row, errors)
        if independently_expected != row.get('object_model_assessment'):
            classification.append({'file': file, 'recorded': row.get('object_model_assessment'), 'expected_from_declared_checks': independently_expected, 'errors': errors})
        for index, geometry in enumerate(array(row.get('shape_geometry'))):
            geometry = mapping(geometry)
            nulls = [key for key in ('type', 'width', 'height') if geometry.get(key) is None]
            if nulls and not array(geometry.get('errors')):
                silent_geometry_nulls.append({'file': file, 'geometry_index': index, 'collection': geometry.get('collection'), 'null_fields': nulls, 'interpretation': 'Supplemental getter returned null without throwing; not a failed dimension check if only Type is null.'})
        if row.get('op') == 'chartdata':
            for index, chart in enumerate(array(row.get('charts'))):
                chart = mapping(chart)
                chart_rollbacks.append({'file': file, 'chart_index': index, 'before': chart.get('before'), 'activation': chart.get('activation'), 'workbook_values': chart.get('workbook_used_range_values'), 'after': chart.get('after'), 'values_changed': chart.get('values_changed_after_activate'), 'check': checks.get('chart_check')})
    attempts = sum(len(values) for values in history.values())
    retry_files = [{'file': file, 'attempt_count': len(values), 'attempts': [{'line': entry['line'], 'timestamp': entry['row'].get('timestamp'), 'open': entry['row'].get('open'), 'assessment': entry['row'].get('object_model_assessment')} for entry in values]} for file, values in history.items() if len(values) > 1]
    now = datetime.now(timezone.utc)
    performance = {'as_of_utc': now.isoformat()}
    try:
        stage_time = datetime.fromisoformat(stage['timestamp'])
        elapsed = float(stage['elapsed_seconds'])
        run_start = stage_time - timedelta(seconds=elapsed)
        run_rows = [row for values in history.values() for record in values if (row := record['row']) and datetime.fromisoformat(row['timestamp']) >= run_start - timedelta(seconds=1)]
        rate = len(run_rows) / elapsed if elapsed > 0 else None
        timestamps = sorted(datetime.fromisoformat(row['timestamp']) for row in run_rows)
        gaps = [(b - a).total_seconds() for a, b in zip(timestamps, timestamps[1:])]
        performance.update({'stage_age_seconds': round((now - stage_time).total_seconds(), 1), 'current_run_start': run_start.isoformat(), 'current_run_recorded': len(run_rows), 'stage_elapsed_seconds': elapsed, 'records_per_second_including_report_work': round(rate, 3) if rate else None, 'remaining_seconds_at_observed_rate': round((1544 - len(latest)) / rate, 1) if rate else None, 'inter_record_gap_median_seconds': round(statistics.median(gaps), 3) if gaps else None, 'inter_record_gap_max_seconds': round(max(gaps), 3) if gaps else None})
    except (KeyError, ValueError, TypeError):
        performance['note'] = 'Stage file unavailable or changed during read; rerun for a coherent live snapshot.'
    cached_source_errors = []
    for base, row in baselines.items():
        if row.get('error') is not None or row.get('close_error') is not None or array(mapping(row.get('metrics')).get('errors')):
            cached_source_errors.append({'base': base, 'open': row.get('open'), 'error': row.get('error'), 'close_error': row.get('close_error'), 'metric_errors': mapping(row.get('metrics')).get('errors'), 'resume_caches_without_new_baseline_attempt': row.get('open') == 'ok' and row.get('reread_method') == 'visible_chart_pass'})
    groups = [{'scope': key[0], 'message': key[1], 'hresult': key[2], **group} for key, group in all_error_groups.items()]
    groups.sort(key=lambda row: (-row['count'], row['scope'], row['message']))
    out = {
        'method': 'Offline snapshot of append-only JSONL. No Word/GUI access and no mutation of batch evidence. Last valid record per filename is current; all earlier attempts remain listed.',
        'expected': 1544, 'recorded_unique': len(latest), 'actual_attempts': attempts, 'remaining': len(set(manifest) - set(latest)),
        'open_counts': dict(Counter(row.get('open') for row in latest.values())), 'assessment_counts': dict(Counter(row.get('object_model_assessment') for row in latest.values())),
        'operation_counts': dict(Counter(row.get('op') for row in latest.values())), 'unique_baselines_read': len(baselines),
        'stage': stage, 'performance': performance, 'retry_files': retry_files, 'invalid_jsonl_lines': invalid, 'invalid_baseline_jsonl_lines': baseline_invalid,
        'missing_files': sorted(set(manifest) - set(latest)), 'manifest_metadata_issues': metadata,
        'top_level_errors_requiring_ui': top_level, 'top_level_errors_without_ui': [row['file'] for row in top_level if not row['already_has_ui']],
        'failed_declared_checks': structural_mismatches, 'confirmed_chartex_getter_limitations': chartex_limits, 'other_diagnostics': other_diagnostics,
        'classification_discrepancies': classification, 'error_groups': groups, 'cached_baseline_diagnostics': cached_source_errors,
        'silent_supplemental_geometry_nulls': silent_geometry_nulls, 'chartdata_activation_readings': chart_rollbacks,
        'resume_review': ['PS5 manifest/results/UI arrays are explicitly unwrapped; JSONL objects are scalar records.', 'Resume skips previously recorded files unless named in RetryFiles; use OnlyFiles with RetryFiles for targeted retries.', 'A trailing partial JSONL line is tolerated during live writing. An invalid interior line needs investigation before accepting complete coverage.', 'Baseline rows marked open=ok are reused even if a later baseline read or close had errors; the cached_baseline_diagnostics list identifies affected sources.', 'Reports rebuild every 25 newly processed records; all JSONL records remain the source of truth for live progress.'],
    }
    destination = args.output or args.root / '_scripts/edited3-diagnostics.json'
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(out, ensure_ascii=True, indent=2) + '\n', encoding='utf-8')
    console = {key: out[key] for key in ('recorded_unique', 'actual_attempts', 'remaining', 'open_counts', 'assessment_counts', 'performance', 'top_level_errors_without_ui')}
    console.update({'chartex_limitation_files': len(chartex_limits), 'classification_discrepancies': len(classification), 'failed_check_files': len(structural_mismatches), 'manifest_issues': len(metadata), 'invalid_jsonl_lines': len(invalid), 'cached_baseline_diagnostics': len(cached_source_errors), 'silent_geometry_nulls': len(silent_geometry_nulls), 'output': str(destination)})
    print(json.dumps(console, ensure_ascii=True))
    if args.require_complete and (len(latest) != 1544 or metadata or invalid or baseline_invalid or classification):
        raise SystemExit(2)


if __name__ == '__main__':
    main()
