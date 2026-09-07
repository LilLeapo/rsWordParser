"""Package a finalized round-3 delivery after its independent final audit passes."""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile


def included_files(root):
    # Keep this inclusion predicate identical to audit-final.py.
    excluded_names = {'edited3-current-item.json'}
    return {path.relative_to(root).as_posix(): path for path in root.rglob('*')
            if path.is_file() and not path.name.startswith('~$') and path.name not in excluded_names
            and not {'_control', '__pycache__'}.intersection(path.relative_to(root).parts)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path('C:/word/real-word-round3-20260907'))
    parser.add_argument('--output', type=Path)
    parser.add_argument('--audit', type=Path, default=Path('C:/word/round3-work-20260907/final-independent-audit.json'), help='Passed final-audit JSON outside the delivery tree; may be passed explicitly.')
    args = parser.parse_args()
    root = args.root.resolve(strict=True)
    output = (args.output or root.with_suffix('.zip')).resolve()
    audit_path = args.audit.resolve(strict=True)
    if output.exists():
        raise FileExistsError(f'Refusing to overwrite an existing delivery: {output}')
    if output.is_relative_to(root):
        raise ValueError('Delivery ZIP must be outside the source delivery tree.')
    if audit_path.is_relative_to(root):
        raise ValueError('Pass an explicit final-audit JSON outside the delivery tree.')
    if output.suffix.lower() != '.zip':
        raise ValueError('Delivery output must have a .zip suffix.')
    audit = json.loads(audit_path.read_text(encoding='utf-8-sig'))
    if not (audit.get('finalCountsRequired') is True and audit.get('auditPassed') is True and audit.get('finalDeliveryComplete') is True and not audit.get('failedChecks') and not audit.get('pendingChecks')):
        raise ValueError('The independent audit has not passed all final requirements.')
    if Path(audit.get('deliveryRoot', '')).resolve() != root:
        raise ValueError('The final audit covers a different delivery root.')
    intended = included_files(root)
    if not intended:
        raise ValueError('No delivery files found.')
    output.parent.mkdir(parents=True, exist_ok=True)
    prefix = root.name + '/'
    with zipfile.ZipFile(output, mode='x', compression=zipfile.ZIP_DEFLATED, compresslevel=6, allowZip64=True) as archive:
        for name, path in sorted(intended.items()):
            archive.write(path, prefix + name)
    with output.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest().upper()
    print(json.dumps({'file': str(output), 'bytes': output.stat().st_size, 'sha256': digest, 'included_files': len(intended), 'root_prefix': prefix, 'audit': str(audit_path)}, indent=2))


if __name__ == '__main__':
    main()
