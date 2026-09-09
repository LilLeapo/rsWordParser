"""Build a factual round3 README draft from current records without controlling Word."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path


def load(path, default=None):
    return json.loads(path.read_text(encoding='utf-8-sig')) if path.is_file() else default


def rows(value):
    return value if isinstance(value, list) else value.get('rows', []) if isinstance(value, dict) else []


def build(root, audit_path, package_audit_path):
    env = load(root / 'environment.json', {})
    initial = load(root / '_scripts/round3-input-audit.json', {})
    a = load(root / '_scripts/task-a-final-summary.json', {})
    b = load(root / '_scripts/edited3-summary.json', {})
    b_readings = rows(load(root / 'edited3-results.json', []))
    b_ui = rows(load(root / 'ui-edited3.json', []))
    chart_audit = load(root / '_scripts/edited3-chart-audit.json', {})
    c = load(root / '_scripts/task-c-summary.json', {})
    d = rows(load(root / '_scripts/task-d-results.json', []))
    d_inspection = load(root / '_readouts/task-d-inspection.json', {})
    audit = load(audit_path, {})
    restore = load(root / '_readouts/settings-restored.json', {})
    counts = initial.get('counts', {})
    missing = []
    lines = [
        '# Windows Word 第三轮实测交付', '',
        f'记录日期：{env.get("date", "待确认")}。本说明生成于 {datetime.now(timezone.utc).isoformat()}。结论以引用的实际记录为准；未完成和存疑项目单列如下。', '',
        '## 环境与方法', '',
        f'- Office LTSC Professional Plus 2021，{env.get("office", {}).get("ProductReleaseIds", "待确认")}，{env.get("office", {}).get("Platform", "待确认")}。',
        f'- WINWORD.EXE 文件版本：{env.get("product", {}).get("FileVersion", "待确认")}；Word 对象模型 Version={env.get("version", "待确认")}，Build={env.get("build", "待确认")}。',
        f'- Windows：{env.get("windows", {}).get("Caption", "待确认")}，版本 {env.get("windows", {}).get("Version", "待确认")}，Build {env.get("windows", {}).get("BuildNumber", "待确认")}，{env.get("windows", {}).get("OSArchitecture", "待确认")}。',
        '- 本轮使用 Windows 中的真实 Word。文档操作、保存、转换及对象模型读数与实际 UI 截图分开记录。未运行、构建或修改 rsWordParser；没有 Microsoft 365 对照。',
        '- DOCX 仅由 Word 创建或另存；包内 XML、哈希和 ZIP 检查均为只读。原始输入和前两轮底稿的保存状态由独立哈希审计核实。',
        '- Word 导出 PDF 是独立的版面证据。修订 PDF 通常呈现最终正文，修订气泡和标记以 Word UI 截图及原生 XML 为证。PDF 导出后不再保存 DOCX。', '',
        '## 输入范围', '',
        f'- 输入 ZIP SHA256：`{initial.get("inputZipSha256", "待审计")}`。',
        f'- 已核对 {counts.get("archiveFiles", "待审计")} 个解包文件；DOCX {counts.get("inputDocx", "待审计")} 份，其中 edited/ 为 {counts.get("actualEditedDocx", "待审计")} 份、fixture 为 {counts.get("fixtureDocx", "待审计")} 份。',
        '- manifest 实际引用 180 份底稿：110 份第一轮原件、53 份第二轮 Word 另存件、17 份第二轮新建样本。任务书写的 127 未计入 53 份另存件。',
        '- manifest 共 1556 行，1544 行为 generated。11 个 ChartEx chartdata 操作不支持而跳过；fields-toc-stale--deleteblock.docx 因引擎 FLD_STRAY_END 保存失败，未生成。这 12 行不属于 Word 打开失败。',
        '- UI 计划共 60 个唯一文件：36 个常规样本、13 个必须复验样本、11 个 M7 额外样本。17 份 M7 底稿没有 chartdata 派生文件，无法提供这一类第 12 个额外样本。',
        '- 八份原始 fixture 与第二轮逐字节相同。compat15 版本只新增 settings.xml 及必要的关系和内容类型登记，document.xml/styles.xml 字节保持不变。', '',
        '## 任务 A', '',
    ]
    if a:
        ac = a.get('counts', {})
        lines.append(f'已记录 {len(a.get("rows", []))}/24 份 Word 原生保存件，8 个 case；结构要求全部通过：{a.get("allRecordedStructuralRequirementsPassed") }。详见 REVISIONS.md、_scripts/task-a-final-summary.json 和 _readouts/revfix-inspection.json。')
        lines.extend(['', '实际 Word 结果：', ''])
        lines.extend('- ' + text for text in a.get('nativeWordOutcomes', []))
        lines.extend(['', '与字面界面操作的差别：', ''])
        lines.extend('- ' + text for text in a.get('methodDeviations', []))
        if a.get('literalUiProcedureFullyPerformed') is False:
            missing.append('任务 A：原生文件和结构要求已完成，但部分操作使用 Word COM；分节符删除、图片移动缩放等未按字面鼠标或键盘流程执行，差别详见任务 A。')
        if len(a.get('rows', [])) != 24 or not a.get('allRecordedStructuralRequirementsPassed'):
            missing.append('任务 A：原生文件、结构自检或最终汇总尚未齐全。')
    else:
        lines.append('待完成：任务 A 最终报告尚未生成。')
        missing.append('任务 A：最终汇总未生成。')
    lines.extend(['', '## 任务 B', ''])
    if b:
        lines.append(f'当前 COM 记录 {b.get("recorded", 0)}/1544，open=ok {b.get("open_ok", 0)}，open error {b.get("open_error", 0)}；独立 UI 记录 {b.get("ui_checked", 0)}。详见 EDITED3.md、edited3-results.json 和原始 JSONL。')
        lines.append('批量打开禁用提示，因此不能从 COM 成功推断没有恢复提示。13 份重点复验与全部抽检的恢复结论来自实际 Word UI。chartdata 保留激活前的图表值、内嵌工作簿值及关闭工作簿后的读数，回滚现象单独报告。')
        raw_counts = {item['result']: item['count'] for item in b.get('assessments', [])}
        reviewed_counts = {item['result']: item['count'] for item in b.get('reviewed_counts', [])}
        if raw_counts:
            lines.append('原始检查分类：' + '，'.join(f'{key} {count}' for key, count in raw_counts.items()) + '。')
        if b.get('independent_ink_review_count'):
            lines.append(f'其中 {b["independent_ink_review_count"]} 个 ink 计数 mismatch 经独立 XML/代码复核，来自检测器“底稿数量 + 1”的假设不适用于覆盖列表替换语义，不能据此判引擎编辑失败。原始读数及 mismatch 均保留；复核后的解释为 ' + '，'.join(f'{key} {count}' for key, count in reviewed_counts.items()) + '。')
        incomplete_rows = [row for row in b_readings if row.get('object_model_assessment') == 'incomplete']
        if incomplete_rows and all(row.get('op') == 'newchart' and 'chartex-' in row.get('base', '') for row in incomplete_rows):
            lines.append(f'{len(incomplete_rows)} 个 incomplete 均为 ChartEx 底稿的 newchart 派生件，保留原有 ChartEx 对象模型读数限制；这不是 Word 打开失败。')
            missing.append(f'任务 B：{len(incomplete_rows)} 份 ChartEx 底稿的原有图表 COM 读数仍不完整。')
        chartdata_rows = [row for row in b_readings if row.get('op') == 'chartdata']
        rollbacks = [row for row in chartdata_rows if ((row.get('checks') or {}).get('chart_check') or {}).get('rollback_to_baseline') is True]
        if chartdata_rows:
            lines.append(f'实际 COM 激活前后读数显示，chartdata 中 {len(rollbacks)}/{len(chartdata_rows)} 份在激活内嵌数据工作簿后回到各自底稿的第一系列值。激活前的请求值与图表标题读取成功，不代表工作簿内容已同步。')
            if rollbacks:
                missing.append(f'任务 B：{len(rollbacks)} 份 chartdata 存在激活内嵌工作簿后的数据回滚；前后读数已保留。')
        independently_mismatched_charts = {item['file'] for item in chart_audit.get('summary', {}).get('sourceCacheWorkbookMismatches', [])}
        if independently_mismatched_charts:
            lines.append(f'另对 {len(independently_mismatched_charts)} 个抽检 chartdata 样本独立解析图表缓存及内嵌工作簿，确认输入中的两者不一致，详见 _scripts/edited3-chart-audit.json。这项 {len(independently_mismatched_charts)} 份结构复核与上述 {len(chartdata_rows)} 份 COM 激活读数是不同证据范围，也与 ink 的计数假阳性复核分开。')
        ink_names = {f'{base}--{op}.docx' for base in ('ink-pen', 'ink-highlighter', 'ink-to-shape') for op in ('newimage', 'newchart', 'ink')}
        chart_names = {f'{base}--chartdata.docx' for base in ('chart-no-title', 'chart-scatter', 'chart-scatter-lines', 'chart-bubble')}
        ui_by_name = {row['file']: row for row in b_ui}
        ink_confirmed = sum(ui_by_name.get(name, {}).get('recovery') in ('none', 'no_prompt', 'no recovery prompt', '无', '无恢复提示') for name in ink_names)
        chart_confirmed = sum(ui_by_name.get(name, {}).get('consistent') is True for name in chart_names)
        lines.append(f'重点复验：{ink_confirmed}/9 个此前恢复提示样本在实际 UI 打开时没有恢复提示；{chart_confirmed}/4 个指定图表样本的激活前标题/数据与请求一致。工作簿激活后的回滚结论单列如上。')
        if b.get('recorded') != 1544:
            missing.append(f'任务 B：COM 记录尚为 {b.get("recorded", 0)}/1544。')
        if b.get('missing_planned_ui'):
            missing.append(f'任务 B：计划 UI 样本仍缺 {len(b["missing_planned_ui"])} 份。')
    else:
        lines.append('待完成：1544 份 COM 记录及 60 个计划 UI 样本尚未汇总。')
        missing.append('任务 B：最终汇总未生成。')
    lines.extend(['', '## 任务 C', ''])
    if c:
        lines.append(f'当前模式 15 测点 {c.get("measuredPrimaryRows", 0)}/25；与第二轮一致 {c.get("sameAsRound2", 0)}，不同 {c.get("differentFromRound2", 0)}。转换交叉验证 {c.get("convertedRows", 0)}/12，三方一致 {c.get("allThreeAgreementRows", 0)}/12。详见 TOGGLE15.md 和 _scripts/task-c-summary.json。')
        missing.extend('任务 C：' + text for text in c.get('incomplete', []))
    else:
        lines.append('待完成：八份兼容模式 15 fixture 的 25 个测点、三个字体对话框以及模式 12 原件经 Word 转换后的 12 个交叉验证读数。')
        missing.append('任务 C：读数与交叉验证汇总尚未生成。')
    lines.append('第二轮 toggle-para-and-char.docx 未记录精确的 CompatibilityMode 数值，仅标题栏证实兼容性模式。历史记录不补造数值。')
    lines.extend(['', '## 任务 D', ''])
    d_descriptions = {
        'unmet-preserved-native-trial': (
            '在 Word 绘图选项卡启用“墨迹转形状”，执行一次连续拖动，得到并保存一条原生直线墨迹，未出现转换后的圆形。',
            '当前绘图接口只支持起点到终点拖动，不能提供连续曲线路径，因此未完成一笔闭合圆；该直线试件不能判断 Word 是否能转换正确画出的圆。'),
        'unmet-depth-two-native-ui-result-preserved': (
            '先用 Word COM 创建正文及三条根批注；随后在界面点击第一条根批注的“答复”，再点击第一条回复自己的“答复”，最后在界面解决整个线程。两次回复未使用 COM Replies.Add。',
            '实际执行了回复上“答复”按钮，但保存结果把两条回复都挂在同一根批注下，未保留第二层父子关系；第一线程已成功标为已解决。'),
    }
    for name in ('ink2/ink-to-shape-2.docx', 'comments2/comment-nesting.docx'):
        match = next((item for item in d if Path(str(item.get('file') or item.get('case') or item.get('path') or '')).stem == Path(name).stem), None)
        if match:
            limitation = [match.get(key) for key in ('limitation', 'unmetRequirements', 'incomplete', 'limitations') if match.get(key)]
            method, limitation_text = d_descriptions.get(match.get('status'), (str(match.get('method', '')), json.dumps(limitation, ensure_ascii=False)))
            state = '已保留原生结果，目标未达成' if limitation else str(match.get('status', '未声明'))
            lines.extend([f'`{name}`：{state}。', '', method, ''])
            if limitation:
                lines.extend([limitation_text, ''])
            inspection = next((item for item in d_inspection.get('rows', []) if item.get('file') == name), {})
            structure = inspection.get('structure', {})
            if inspection:
                if name.startswith('comments2/'):
                    lines.append(f'独立结构检查：{structure.get("commentCount")} 条批注、{structure.get("rootCount")} 条根批注，最大回复深度 {structure.get("maximumDepth")}，目标深度应为 2。')
                else:
                    lines.append('独立结构检查：保留 1 条原生墨迹，存在 w14:contentPart，没有 wps:wsp 圆形，目标结构未通过。')
            if match.get('evidenceCaveat'):
                lines.append('截图中另一桌面应用遮挡了页面下部；第一线程及点击的答复、解决控件仍可见，初始图可见三条根批注。独立 PDF 未被遮挡。')
            if match.get('trials'):
                trial_paths = [item if isinstance(item, str) else item.get('file') or item.get('path') or '(未记录路径)' for item in match['trials']]
                lines.append('保留试件：' + '，'.join(f'`{path}`' for path in trial_paths) + '。')
            if limitation:
                missing.append(name + '：' + limitation_text)
            lines.append('')
        else:
            lines.append(f'- `{name}`：待实际 UI 操作及原生结构检查。')
            missing.append(name + '：尚无最终操作记录。')
    lines.append('逐步操作、首次保存哈希及截图索引见 [_scripts/task-d-results.json](_scripts/task-d-results.json)；独立结构和 PDF 复核见 [_readouts/task-d-inspection.json](_readouts/task-d-inspection.json)。两份 DOCX 保存后均未再次保存。')
    lines.extend(['', '## 完整性与环境恢复', ''])
    if audit.get('finalCountsRequired') and audit.get('auditPassed') and audit.get('deliveryRoot') and Path(audit['deliveryRoot']).resolve() == root.resolve():
        lines.append(f'本地目录的独立完整性审计已通过，报告位于交付目录外：`{audit_path.as_posix()}`。此结论验证文件、哈希和证据覆盖，不会把已声明的未达成事项变成已达成。')
    else:
        lines.append(f'待完成：本地目录最终完整性审计。报告将写入交付目录外：`{audit_path.as_posix()}`。')
        missing.append('本地目录最终完整性审计尚未完成。')
    lines.append(f'ZIP 打包后 CRC 和逐文件哈希核对使用单独的外部报告：`{package_audit_path.as_posix()}`。本地目录审计与 ZIP 校验是两项不同的记录；本 README 不声明 ZIP 已生成或通过校验，打包后不再改写它。')
    if restore.get('allSettingsRestored') is True and restore.get('environmentUnchanged') is True:
        lines.append(f'环境恢复：{len(restore.get("checks", []))} 项原始设置逐项回读一致，environment.json 保持不变；任务 Word 实例剩余文档 {restore.get("remainingDocuments")}，Word 退出记录为 {restore.get("wordQuit")}。详见 _readouts/settings-restored.json。')
    elif restore:
        lines.append('环境恢复记录已存在，但尚未确认全部恢复成功，详见 _readouts/settings-restored.json。')
        missing.append('Word 原始设置恢复尚未全部确认。')
    else:
        lines.append('待完成：恢复 environment.json 中的原始用户信息及 Word 选项，关闭本任务打开的文档，并记录 _readouts/settings-restored.json。')
        missing.append('Word 原始设置恢复和任务文档清理尚未记录。')
    if (root / '_readouts/word-worker-finish.json').is_file():
        lines.append('任务 Word worker 的最终清理和退出另见 _readouts/word-worker-finish.json。')
    lines.extend(['', '## 未完成 / 存疑', ''])
    lines.extend('- ' + text for text in missing)
    if not missing:
        lines.append('没有当前未完成项。字面 UI 操作差异及历史缺失读数已在对应章节明确保留。')
    lines.extend(['', '## 证据边界', '',
                  '- REVISIONS.md：逐 case、逐文件的实际正文、操作和结构结果。',
                  '- EDITED3.md / edited3-results.json：1544 份输入的 COM 读数、实际抽检观察和与第二轮对照。',
                  '- TOGGLE15.md：25 个模式 15 测点及 12 个 Word 转换交叉验证。',
                  '- screenshots/：实际 Word UI 截图与独立 PDF 渲染证据，文件名区分来源。_previews/ 只存 PDF。',
                  '- _scripts/ 与 _readouts/：作者操作日志、原始读数、复核脚本、只读审计及未达成项。', ''])
    return '\n'.join(lines)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path('C:/word/real-word-round3-20260907'))
    parser.add_argument('--audit', type=Path, default=Path('C:/word/round3-work-20260907/final-independent-audit.json'), help='External directory audit report. Regenerate the audit after writing README and before packaging.')
    parser.add_argument('--package-audit', type=Path, default=Path('C:/word/round3-work-20260907/final-package-audit.json'), help='External package verification report path; no success is inferred from its existence.')
    parser.add_argument('--output', type=Path, help='Default: _scripts/README-draft.md. Explicitly use root/README.md only after reviewing the draft.')
    args = parser.parse_args()
    target = args.output or args.root / '_scripts/README-draft.md'
    target.write_text(build(args.root, args.audit, args.package_audit), encoding='utf-8')
    print(str(target))
    print('README written. Run the final directory audit after this write and before packaging. Package validation must not rewrite README.')
