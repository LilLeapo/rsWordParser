"""Archive the deliverable without changing any DOCX package."""
from pathlib import Path
from zipfile import ZipFile, ZIP_DEFLATED
import argparse
import hashlib
import json

parser = argparse.ArgumentParser()
parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
parser.add_argument("--output-dir", type=Path)
args = parser.parse_args()
root = args.root.resolve()
required = [root / name for name in ["README.md", "OBSERVED.md", "ROUNDTRIP.md", "_scripts/corpus-audit.json"]]
missing = [str(path) for path in required if not path.is_file()]
if missing:
    raise SystemExit(f"Missing final reports: {missing}")
output_directory = args.output_dir.resolve() if args.output_dir else root.parent
output_directory.mkdir(parents=True, exist_ok=True)
output = output_directory / f"{root.name}.zip"
number = 2
while output.exists():
    output = output_directory / f"{root.name}-{number}.zip"
    number += 1
files = [path for path in root.rglob("*") if path.is_file() and not path.name.startswith("~$") and not any(part in ["_checks", "__pycache__"] for part in path.relative_to(root).parts)]
hashes = {path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest() for path in files if path.suffix.lower() == ".docx"}
with ZipFile(output, "x", compression=ZIP_DEFLATED, compresslevel=6) as archive:
    for path in sorted(files):
        archive.write(path, f"{root.name}/{path.relative_to(root).as_posix()}")
with ZipFile(output) as archive:
    if archive.testzip() is not None:
        raise SystemExit("Delivery ZIP CRC validation failed.")
    for name, digest in hashes.items():
        if hashlib.sha256(archive.read(f"{root.name}/{name}")).hexdigest() != digest:
            raise SystemExit(f"Delivery DOCX hash mismatch: {name}")
    if any(Path(name).name.startswith("~$") for name in archive.namelist()):
        raise SystemExit("A lock file was included in the delivery ZIP.")
print(json.dumps({"zip": str(output), "bytes": output.stat().st_size, "files": len(files), "docxFiles": len(hashes), "allDocxHashesMatch": True, "lockFiles": 0, "sha256": hashlib.sha256(output.read_bytes()).hexdigest()}, indent=2))
