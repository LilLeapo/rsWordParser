"""Read-only input/source audit and deterministic round-3 UI sampling plan."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import zipfile


def sha256(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--input-zip', type=Path, default=Path('C:/word/real-word-round3-inputs-20260907.zip'))
    parser.add_argument('--round2', type=Path, default=Path('C:/word/real-word-round2-20260907'))
    parser.add_argument('--baseline', type=Path, default=Path('C:/word/round2-work-20260907/baseline/real-word-corpus-20260906'))
    parser.add_argument('--output', type=Path, default=Path('C:/word/real-word-round3-20260907/_scripts/edited3-plan.json'))
    args = parser.parse_args()
    with zipfile.ZipFile(args.input_zip) as archive:
        manifest = json.loads(archive.read('real-word-round3-inputs/edited/manifest.json'))
        generated = [entry for entry in manifest if entry['status'] == 'generated']
        assert len(generated) == 1544
        by_file = {entry['file']: entry for entry in generated}
        assert len(by_file) == 1544
        actual_docx = {Path(entry.filename).name for entry in archive.infolist() if entry.filename.startswith('real-word-round3-inputs/edited/') and entry.filename.endswith('.docx')}
        assert actual_docx == set(by_file)
    m7_results = json.loads((args.round2 / '_scripts/task-d-results.json').read_text(encoding='utf-8-sig'))
    m7_bases = {entry['case'] + '.docx' for entry in m7_results}
    assert len(m7_bases) == 17
    sources = []
    for base in sorted({entry['base'] for entry in generated}):
        candidates = [root / base for root in (args.baseline, args.round2) if (root / base).is_file()]
        assert len(candidates) == 1, (base, candidates)
        source = candidates[0]
        category = 'm7' if base in m7_bases else 'round2_resaved' if base.startswith('_resaved/') else 'original'
        sources.append({'base': base, 'source_path': source.as_posix(), 'source_sha256': sha256(source), 'source_category': category})
    assert Counter(entry['source_category'] for entry in sources) == {'original': 110, 'round2_resaved': 53, 'm7': 17}
    mandatory = [f'{base}--{op}.docx' for base in ('ink-pen', 'ink-highlighter', 'ink-to-shape') for op in ('newimage', 'newchart', 'ink')]
    mandatory += [f'{base}--chartdata.docx' for base in ('chart-no-title', 'chart-scatter', 'chart-scatter-lines', 'chart-bubble')]
    old_plan = json.loads((args.round2 / '_scripts/selection.json').read_text(encoding='utf-8-sig'))
    replacement = {'ink-pen--newchart.docx': 'fields-toc--newchart.docx', 'chart-scatter--chartdata.docx': 'chart-no-legend--chartdata.docx'}
    regular = [replacement.get(entry['file'], entry['file']) for entry in old_plan['selected']]
    assert len(regular) == 36 and set(regular).isdisjoint(mandatory)
    m7_choice = {
        'insert': 'rev-insert-delete--insert.docx',
        'newimage': 'image-z-order--newimage.docx',
        'newchart': 'fields-citations--newchart.docx',
        'ink': 'textbox-linked--ink.docx',
        'deleteblock': 'rev-move--deleteblock.docx',
        'replaceimage': 'fields-seq-captions--replaceimage.docx',
        'insertrow': 'rev-table--insertrow.docx',
        'mergecells': 'rev-table--mergecells.docx',
        'header': 'sections-breaks-zoo--header.docx',
        'comment': 'rev-comment-threads--comment.docx',
        'split': 'fields-page-in-footer--split.docx',
    }
    selected = []
    for group, files in (('mandatory', mandatory), ('regular', regular), ('m7_extra', list(m7_choice.values()))):
        for filename in files:
            entry = dict(by_file[filename])
            entry.update({'selection_group': group, 'domain': entry['base'].split('/')[0], 'm7_source': entry['base'] in m7_bases})
            if group == 'm7_extra':
                assert entry['m7_source']
            selected.append(entry)
    counts = Counter(entry['file'] for entry in selected)
    assert len(counts) == 60 and max(counts.values()) == 1
    assert Counter(by_file[file]['op'] for file in regular) == {op: 3 for op in {entry['op'] for entry in generated}}
    m7_operation_counts = Counter(entry['op'] for entry in generated if entry['base'] in m7_bases)
    assert m7_operation_counts['chartdata'] == 0
    result = {
        'status': 'Planned samples and read-only source audit; not Word observations.',
        'input_zip': args.input_zip.as_posix(), 'input_zip_sha256': sha256(args.input_zip),
        'manifest_count': len(manifest), 'generated_count': len(generated),
        'generated_per_operation': dict(sorted(Counter(entry['op'] for entry in generated).items())),
        'baseline_count': len(sources), 'baseline_category_counts': dict(Counter(entry['source_category'] for entry in sources)),
        'task_text_discrepancy': 'Actual 180 sources = 110 original + 53 round2 resaves + 17 M7; task text 127 omits 53 resaves.',
        'requested_slots': 61, 'available_slots': len(selected), 'unique_selected_count': len(counts),
        'selection_groups': dict(Counter(entry['selection_group'] for entry in selected)),
        'duplicates': {file: count for file, count in counts.items() if count > 1},
        'unavailable_requested_samples': [{'operation': 'chartdata', 'group': 'm7_extra', 'reason': 'No chartdata entry exists among the 17 M7 sources in the supplied manifest.'}],
        'm7_generated_per_operation': dict(sorted(m7_operation_counts.items())),
        'selected': selected, 'sources': sources,
        'non_generated': [entry for entry in manifest if entry['status'] != 'generated'],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=True) + '\n', encoding='utf-8')
    print(json.dumps({key: result[key] for key in ('generated_count', 'baseline_count', 'selection_groups', 'unique_selected_count', 'duplicates', 'unavailable_requested_samples')}, ensure_ascii=True))


if __name__ == '__main__':
    main()
