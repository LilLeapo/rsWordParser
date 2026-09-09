"""Combine reviewed case tables; missing cases are explicitly incomplete."""
from pathlib import Path
import re
import argparse
import runpy

parser = argparse.ArgumentParser()
parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
args = parser.parse_args()
root = args.root
catalog = runpy.run_path(str(Path(__file__).with_name("audit-corpus.py")))["NAMES"]
rows = {}
fragments = sorted((root / "_scripts").glob("observed-*.md"))
ui_fragment = root / "_scripts" / "observed-ui.md"
fragments = [path for path in fragments if path != ui_fragment] + ([ui_fragment] if ui_fragment.exists() else [])
for fragment in fragments:
    for line in fragment.read_text(encoding="utf-8-sig").splitlines():
        match = re.match(r"\|\s*`?([^`|\s]+\.docx)`?\s*\|", line)
        if match:
            if len(line.strip().strip("|").split("|")) != 5:
                raise ValueError(f"Expected five columns in {fragment}: {line}")
            rows[match.group(1)] = f"| `{match.group(1)}` |" + line[match.end():]
version = "16.0.14334.20848 / Windows 11 x64 (Office LTSC 2021)"
for domain, names in catalog.items():
    for name in names.split():
        file = f"{domain}/{name}.docx"
        if file not in rows:
            state = "文件存在，但尚无完整的制作与目视验收记录；未判为通过" if (root / file).exists() else "未完成：本次尚未生成或观察此案例"
            rows[file] = f"| `{file}` | {version} | 未完成 | 参见制作清单 | {state} |"
for domain in catalog:
    for path in (root / domain).glob("*.docx"):
        file = path.relative_to(root).as_posix()
        if file not in rows and not path.name.startswith("~$"):
            rows[file] = f"| `{file}` | {version} | 待核对运行日志 | 保留的 Word 原件 | 文件存在，未完成目视验收；不判为通过 |"
intro = """# OBSERVED - Windows Word 实测记录

执行平台为 Office LTSC Professional Plus 2021，64 位，build 16.0.14334.20848；不代表 Microsoft 365、macOS Word 或 WPS 的结果。任务 A 的字面清单共 92 个案例。保留失败尝试，重做文件用数字后缀区分。

记录中的渲染观察来自实际 Word 窗口或该创建实例直接导出的 PDF，并由代理查看。PDF、截图与脚本日志随附；包结构自检在副本上进行。包内存在某个标记或脚本成功执行，均不单独视为目视验收通过。历史运行日志中的 pending 状态不覆盖此表的最终记录。

| 文件 | Word 版本（build）/ 平台 | 制作方式（UI / 脚本名） | 步骤要点 | 看到什么 |
| --- | --- | --- | --- | --- |
"""
ordered = sorted(rows, key=lambda file: (list(catalog).index(file.split("/")[0]) if file.split("/")[0] in catalog else len(catalog), file))
(root / "OBSERVED.md").write_text(intro + "\n".join(rows[file] for file in ordered) + "\n", encoding="utf-8")
print(f"OBSERVED.md: {len(rows)} rows from {len(fragments)} reviewed fragments")
