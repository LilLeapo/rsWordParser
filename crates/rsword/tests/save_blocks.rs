//! `SaveBlock[]` 兼容映射（任务 1.13，`EDIT-04` / `COMPAT-08`）：用 TS 测试导出的
//! `corpus/synthetic/*.save.<k>.json`（`blocks` + `options`）驱动 `apply_save_blocks`，保存后主 part 与
//! 其中的 `documentXml`（TS `saveDocx` 的输出）按 `xml::canon` 规范化后相等——等价于任何 XPath 子集
//! 表达式在两者上结果相同。等价是**手段不是目标**：TS 不是验收权威，`INTENTIONAL` 列出我们有意做得
//! 不同的用例（`docs/04` §8）。M1 范围：`options` 只含 `savedAt` / `removePersonalInfo`（`SAVE-07`），
//! 块只含 original / generated / xml；`options` 到 2.6 为止支持 `savedAt` / `removePersonalInfo` /
//! `comments` / `footnotes` / `endnotes`（`SAVE-07` + 权威条目列表）；用到页眉页脚、图表、图片、
//! 墨迹等后续里程碑能力的用例记为"跳过"并列出原因。

mod common;

use std::collections::BTreeMap;

use rsword::bind::compat_ts::apply_save_blocks;
use rsword::diag::DiagCode;
use rsword::edit::EditSession;
use rsword::error::Error;
use rsword::package::{Package, PartId};
use rsword::xml::canon::first_difference;
use rsword::xml::{CanonOptions, Dom, LocalName, NodeId, NsId, QName, canonical, xpath_strings};
use serde_json::Value;

/// 保存后 `w:p` 上的段落 id / rsid 与 TS 输出无关（TS 生成的段落是裸 `<w:p>`，我们复用原节点保留它们）；
/// `xml:space="preserve"`：`SAVE-03` 对 New `w:t` 一律写，TS 只对自己生成的文本写、逐字片段照抄。
fn ignore_attr(dom: &Dom, node: NodeId, attr: QName) -> bool {
    let Some(element) = dom.name(node) else { return false };
    // `xml:space`：只对**文本元素**放行（`SAVE-03` 对 New 的 `w:t` 一族一律写，TS 只对自己
    // 生成的文本写、逐字片段照抄）。别的元素上出现 `xml:space` 是真差异，不该被这条盖住
    // （7.9 的复核：从"全元素放行"收窄到这四个，等价数一个没少）。
    // `RSWORD_STRICT_XML_SPACE=1` 把这条整个关掉，用来量它到底盖住了多少——复核当天的读数
    // 写在 `docs/04` §16 的 7.9b 条目里
    if attr == QName::new(NsId::Xml, LocalName::Space)
        && std::env::var("RSWORD_STRICT_XML_SPACE").is_err()
        && matches!(
            element,
            QName { ns: NsId::W, local: LocalName::T }
                | QName { ns: NsId::W, local: LocalName::DelText }
                | QName { ns: NsId::W, local: LocalName::InstrText }
                | QName { ns: NsId::W, local: LocalName::DelInstrText }
        )
    {
        return true;
    }
    // 墨迹 run 的 `relativeHeight` 由 `docPr/@id` 派生（TS `251658240 + docPrId`，id 从 9001 起计），
    // 与 id 一样是分配细节（任务 6.8）；普通锚定图片的 `relativeHeight` 是输入的 z-order，照常比较
    if element == QName::new(NsId::Wp, LocalName::Anchor)
        && attr == QName::new(NsId::None, LocalName::RelativeHeight)
        && dom.children(node).iter().any(|&c| {
            dom.is(c, QName::new(NsId::Wp, LocalName::DocPr))
                && dom
                    .attr_value(c, QName::new(NsId::None, LocalName::Name))
                    .is_some_and(|v| v.starts_with("aidocs-ink"))
        })
    {
        return true;
    }
    // `COMPAT-09`：新绘图的 `wp:docPr/@id`（与由它派生的 `@name`）是分配细节——TS 从 8000 起计数，
    // 我们按 `EDIT-06` 取最大值 + 1（任务 6.6）；`pic:cNvPr` 同理（6.7）
    if (element == QName::new(NsId::Wp, LocalName::DocPr)
        || element == QName::new(NsId::Pic, LocalName::CNvPr))
        && attr.ns == NsId::None
        && matches!(attr.local, LocalName::Id | LocalName::Name)
    {
        return true;
    }
    element == QName::w(LocalName::P)
        && (attr.ns == NsId::W14
            || (attr.ns == NsId::W
                && matches!(
                    attr.local,
                    LocalName::RsidR
                        | LocalName::RsidRDefault
                        | LocalName::RsidP
                        | LocalName::RsidRPr
                        | LocalName::RsidDel
                )))
}

fn body_of(dom: &Dom) -> rsword::xml::NodeId {
    dom.children(dom.root())
        .iter()
        .copied()
        .find(|&c| dom.is(c, QName::w(LocalName::Body)))
        .expect("w:body")
}

/// **有意**与 TS 输出不同的用例（`<save 文件名> <理由>`）。TS 不是验收权威：目标是功能等价或更强
/// （`docs/04` §8 开头的政策），这几条是我们按 `EDIT-06` 分配修订 id 的结果，比 TS 的固定值更安全。
const INTENTIONAL: &[(&str, &str)] = &[
    (
        "revisions__007.save.1.json",
        "块级修订 w:id 按 EDIT-06 取全文档最大值 + 1；TS 缺省写 0（多次插入会重号）",
    ),
    ("revisions__007.save.2.json", "同上（表格的块级修订）"),
    ("revisions__007.save.3.json", "run 级修订 w:id 同样按 EDIT-06 分配；TS 从固定的 9001 起"),
    (
        "comments__001.save.2.json",
        "权威列表删掉批注后：我们把空掉的 commentReference run 整个删掉，TS 留下一个 <w:r></w:r>",
    ),
];

#[test]
fn compat_08_save_blocks_match_ts_save_docx_output() {
    let dir = common::corpus_dir("synthetic");
    let files = common::save_cases();
    let opts = CanonOptions { ignore_attr: &ignore_attr };
    let mut passed = Vec::new();
    let mut skipped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut failed = Vec::new();
    let mut unchanged = 0;
    for path in &files {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let case: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let stem = file.split(".save.").next().unwrap();
        let docx = dir.join(format!("{stem}.docx"));
        let Ok(bytes) = std::fs::read(&docx) else {
            skipped.entry("源 docx 缺失".into()).or_default().push(file);
            continue;
        };
        let mut session = EditSession::open(&bytes).unwrap();
        let outcome = match apply_save_blocks(&mut session, &case["blocks"], &case["options"]) {
            Ok(o) => o,
            Err(Error::Edit { code: DiagCode::EditUnsupported, message }) => {
                let reason = message.split('（').next().unwrap_or(&message).to_string();
                skipped.entry(reason).or_default().push(file);
                continue;
            }
            Err(e) => panic!("{file}: {e}"),
        };
        let saved = session
            .save_with(&outcome.save_options)
            .unwrap_or_else(|e| panic!("{file}: save: {e}"));
        if case["outputIdenticalToSource"] == Value::Bool(true) {
            // TS 自己记录的"输出与源文件逐字节相同"：我们也必须走不变式 1 的短路
            assert_eq!(saved, bytes, "{file}: 无变更保存应返回原字节");
            assert!(outcome.unchanged, "{file}: 没有编辑却产生了 EditOp");
            unchanged += 1;
        }
        let mut pkg = Package::open(&saved).unwrap();
        let main = pkg.main_part();
        let ours = pkg.dom(main).unwrap().unwrap();
        let expected =
            Dom::parse(PartId(0), case["documentXml"].as_str().unwrap().as_bytes()).unwrap();
        let a = canonical(ours, body_of(ours), &opts);
        let b = canonical(&expected, body_of(&expected), &opts);
        if a == b {
            passed.push(file);
            continue;
        }
        // 定位第一个不同的顶层块，给可读的 XPath 结果
        let n = xpath_strings(ours, "count(/w:document/w:body/*)").unwrap();
        let m = xpath_strings(&expected, "count(/w:document/w:body/*)").unwrap();
        let mut detail = format!("body 子元素数 ours={} ts={}", n[0], m[0]);
        let count: usize = n[0].parse::<usize>().unwrap().min(m[0].parse::<usize>().unwrap());
        for i in 1..=count {
            let xp = format!("string(/w:document/w:body/*[{i}])");
            let (x, y) =
                (xpath_strings(ours, &xp).unwrap(), xpath_strings(&expected, &xp).unwrap());
            if x != y {
                detail.push_str(&format!("\n  首个文本不同的块 #{i}: ours={x:?} ts={y:?}"));
                break;
            }
        }
        if let Some((x, y)) = first_difference(&a, &b) {
            detail.push_str(&format!("\n  规范化文本差异:\n    ours: {x}\n    ts:   {y}"));
        }
        failed.push((file, detail));
    }
    // `spec/18` 门 4：比较范围从 `documentXml` 扩到**每个被 TS 改写的 XML part**
    let (part_stats, part_failed) = compare_changed_parts(&dir, &files, &opts);
    for line in &part_stats {
        eprintln!("save-blocks: {line}");
    }
    eprintln!(
        "save-blocks: {} 用例，{} 等价（其中 {} 份逐字节相同），{} 失败，{} 跳过",
        files.len(),
        passed.len(),
        unchanged,
        failed.len(),
        skipped.values().map(Vec::len).sum::<usize>()
    );
    for (reason, fs) in &skipped {
        eprintln!("save-blocks: 跳过 {} × {reason}: {}", fs.len(), fs.join(", "));
    }
    for (f, d) in &failed {
        eprintln!("save-blocks: FAIL {f}\n  {d}");
    }
    let unexpected: Vec<_> =
        failed.iter().filter(|(f, _)| !INTENTIONAL.iter().any(|(k, _)| k == f)).collect();
    assert!(unexpected.is_empty(), "{} 个用例意外地与 TS saveDocx 输出不等价", unexpected.len());
    for (case, part, detail) in &part_failed {
        eprintln!("save-blocks: FAIL part {case} :: {part}\n  {detail}");
    }
    let unexpected: Vec<_> = part_failed
        .iter()
        .filter(|(case, part, _)| {
            !PART_INTENTIONAL.iter().any(|(p, _)| p == part)
                && !INTENTIONAL.iter().any(|(k, _)| k == case)
        })
        .collect();
    assert!(unexpected.is_empty(), "{} 个 part 意外地与 TS 不等价", unexpected.len());
    // 必须覆盖的 text-patch 用例
    for must in [
        "insert-and-layout__001.save.8.json",
        "insert-and-layout__001.save.9.json",
        "entity-decoding__003.save.1.json",
        "bookmarks-crossref__002.save.1.json",
        "out-of-run-breaks__005.save.1.json",
        "positioned-frame__001.save.1.json",
        "revisions__006.save.1.json",
        "sections__003.save.2.json",
        "sdt__008.save.2.json",
        "sdt__008.save.3.json",
        "verify16-p1p2__013.save.2.json",
        "insert-and-layout__001.save.2.json",
        "math__003.save.1.json",
        "ruby__002.save.1.json",
        "smartart-ole__007.save.1.json",
        "comments__001.save.4.json",
        "revisions__001.save.1.json",
        "write-protection__003.save.1.json",
        "write-protection__004.save.1.json",
        "write-protection__005.save.1.json",
        "write-protection__006.save.1.json",
        "docprops__001.save.2.json",
    ] {
        assert!(passed.iter().any(|f| f == must), "{must} 应在等价用例里");
    }
    assert!(passed.len() >= 75, "等价用例过少: {}", passed.len());
    for (f, why) in INTENTIONAL {
        assert!(
            failed.iter().any(|(x, _)| x == f),
            "{f} 现在与 TS 等价（有意差异：{why}），请从 INTENTIONAL 移除"
        );
    }
}

/// 整个 part 就是**分配细节**、与 TS 不可能逐项相同的那些（`spec/18` 门 4 的登记）。
const PART_INTENTIONAL: &[(&str, &str)] = &[
    (
        "[Content_Types].xml",
        "TS 每次保存整份重写并按自己的顺序排 Default / Override；我们只在新建 part 时补一条（`SAVE-05`），未变的原字节不动（不变式 1）",
    ),
    (
        "word/_rels/document.xml.rels",
        "新媒体 part 的命名：TS 叫 `media/aidocs{N}.{ext}` / `media/aidocsink{N}.png`，我们按 6.7 的规则叫 `media/image{N}.{ext}`（第一个空闲的 N）。`rId` 本身不比——这里比的是 `(类型, 目标, 模式)` 的多重集合",
    ),
    (
        "word/comments.xml",
        "我们保留原 part 根元素上的 `mc:Ignorable`；TS 整份重写成裸根。保留更忠实（不变式 1）",
    ),
    (
        "word/header1.xml",
        "水印：我们把水印段落**加进**原有页眉；TS 把页眉正文整个换成只剩水印那一段。页眉里本来有内容时 TS 的做法会把它丢掉",
    ),
    (
        "word/footnotes.xml",
        "`SAVE-05` 新建注释 part 的模板：TS 的分隔段带 `pPr/spacing`、引用 run 带 `rPr/vertAlign`，我们发的是最小合法形态。两者 Word 都认",
    ),
    ("word/endnotes.xml", "同 `word/footnotes.xml`"),
    (
        "word/commentsExtended.xml",
        "`w15:paraId` 是分配细节（TS 从 `10001112` 起数、我们按 `EDIT-06` 铸）；另外我们给回复写 `w15:paraIdParent`，TS 不写（真实 Word 是写的，`docs/09` 第三轮）",
    ),
    (
        "word/styles.xml",
        "`styleUpsert` 建的样式我们写 `w:type`，TS 不写。没有 `w:type` 的 `w:style` 不合 schema",
    ),
    (
        "word/settings.xml",
        "TS 整份从自己的模型重写（顺手丢掉源里的 `w:zoom`、补自己的默认项）；我们只按 `PROP-06` 合并请求里的字段，别的原字节不动",
    ),
    (
        "docProps/core.xml",
        "没给 `savedAt` 时 TS 照样把 `dcterms:modified` 盖成**保存那一刻**；我们不动它（`SAVE-07`：单独设 `savedAt` 都不该让未编辑的文档产生输出）。对照件里记的是导语料那天的时间，本来就不可能对上",
    ),
];

/// 门 4：`changedParts` 里每个 part 的规范化对照。返回 `(统计行, 失败明细)`。
///
/// `.rels` 的 `rId` 是分配细节（TS 从 `rId100` 起、我们按 `EDIT-06` 取最大值 + 1），逐项比没有
///意义——比**关系的类型与目标的多重集合**：语义一样就算等价。
fn compare_changed_parts(
    dir: &std::path::Path,
    files: &[std::path::PathBuf],
    opts: &CanonOptions<'_>,
) -> (Vec<String>, Vec<(String, String, String)>) {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut equal: BTreeMap<String, usize> = BTreeMap::new();
    let mut failed: Vec<(String, String, String)> = Vec::new();
    for path in files {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let case: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let Some(parts) = case["changedParts"].as_object() else { continue };
        let stem = file.split(".save.").next().unwrap();
        let Ok(bytes) = std::fs::read(dir.join(format!("{stem}.docx"))) else { continue };
        let mut session = match EditSession::open(&bytes) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let Ok(outcome) = apply_save_blocks(&mut session, &case["blocks"], &case["options"]) else {
            continue;
        };
        let Ok(saved) = session.save_with(&outcome.save_options) else { continue };
        let Ok(mut pkg) = Package::open(&saved) else { continue };
        for (name, content) in parts {
            if name == "word/document.xml" {
                continue; // 主 part 由上面那段专门比
            }
            *seen.entry(name.clone()).or_default() += 1;
            let Some(text) = content.as_str() else { continue };
            let Some(id) = pkg.find_name(name) else {
                failed.push((file.clone(), name.clone(), "我们的包里没有这个 part".into()));
                continue;
            };
            let Ok(Some(ours)) = pkg.dom(id) else {
                failed.push((file.clone(), name.clone(), "这个 part 不是 XML".into()));
                continue;
            };
            let Ok(theirs) = Dom::parse(PartId(0), text.as_bytes()) else {
                failed.push((file.clone(), name.clone(), "TS 的内容解析不了".into()));
                continue;
            };
            let same = if name.ends_with(".rels") {
                rel_multiset(ours) == rel_multiset(&theirs)
            } else {
                canonical(ours, ours.root(), opts) == canonical(&theirs, theirs.root(), opts)
            };
            if same {
                *equal.entry(name.clone()).or_default() += 1;
            } else {
                let detail = if name.ends_with(".rels") {
                    format!(
                        "关系集合不同\n    ours: {:?}\n    ts:   {:?}",
                        rel_multiset(ours),
                        rel_multiset(&theirs)
                    )
                } else {
                    match first_difference(
                        &canonical(ours, ours.root(), opts),
                        &canonical(&theirs, theirs.root(), opts),
                    ) {
                        Some((x, y)) => format!("规范化文本差异\n    ours: {x}\n    ts:   {y}"),
                        None => "规范化文本相同但比较判定不等（不该发生）".into(),
                    }
                };
                failed.push((file.clone(), name.clone(), detail));
            }
        }
    }
    let mut stats: Vec<String> = Vec::new();
    let total: usize = seen.values().sum();
    let ok: usize = equal.values().sum();
    stats.push(format!(
        "门 4：`changedParts` 里主 part 之外的 {total} 项，{ok} 项等价、{} 项不等价（{} 种 part）",
        total - ok,
        seen.len()
    ));
    for (name, n) in &seen {
        let e = equal.get(name).copied().unwrap_or(0);
        if e != *n {
            stats.push(format!("  {name}: {e} / {n} 等价"));
        }
    }
    (stats, failed)
}

/// `.rels` 的语义视图：`(类型, 目标, 模式)` 的多重集合。`rId` 是分配细节，不进比较。
fn rel_multiset(dom: &Dom) -> BTreeMap<(String, String, String), usize> {
    let mut out: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    let attr = |n: NodeId, name: &str| {
        dom.element(n)
            .and_then(|e| e.attrs.iter().find(|a| a.name.local.as_str(dom.interner()) == name))
            .map(|a| dom.attr_str(a).into_owned())
            .unwrap_or_default()
    };
    for n in dom.descendants(dom.root()) {
        if dom.name(n).is_some_and(|q| q.local.as_str(dom.interner()) == "Relationship") {
            let key = (attr(n, "Type"), attr(n, "Target"), attr(n, "TargetMode"));
            *out.entry(key).or_default() += 1;
        }
    }
    out
}

/// `--via js`（M8′ 8.0②，`COMPAT-08` 的绑定等价门）：同一批保存用例经 wasm 绑定 `save`
/// 的输出与原生逐字节相同；原生被拒（`EditUnsupported`）的用例绑定也以同一个 `code` 拒绝。
/// 产物由 `tools/js-parity/save_parity.mjs` 先落 `$RSWORD_JS_SAVE_DIR`；缺省 `cargo test`
/// 不依赖 node（`common::via_js_dir!` 门控）。
#[test]
fn js_binding_save_bytes_parity() {
    let js = common::via_js_dir!("RSWORD_JS_SAVE_DIR");
    let dir = common::corpus_dir("synthetic");
    let mut checked = 0usize;
    for path in common::save_cases() {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let case: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let stem = file.split(".save.").next().unwrap().to_string();
        let Ok(bytes) = std::fs::read(dir.join(format!("{stem}.docx"))) else { continue };
        let mut session = EditSession::open(&bytes).unwrap();
        match apply_save_blocks(&mut session, &case["blocks"], &case["options"]) {
            Ok(outcome) => {
                let saved = session.save_with(&outcome.save_options).unwrap();
                let js_name = format!("{file}.docx");
                assert_eq!(
                    common::binding_bytes(&js, &js_name),
                    saved,
                    "{file}: 绑定 save 与原生不等"
                );
            }
            Err(Error::Edit { code: DiagCode::EditUnsupported, .. }) => {
                let js_name = format!("{file}.docx");
                assert_eq!(
                    common::binding_error_code(&js, &js_name),
                    DiagCode::EditUnsupported.as_str(),
                    "{file}: 绑定应同样以 EDIT_UNSUPPORTED 拒绝"
                );
            }
            Err(e) => panic!("{file}: {e}"),
        }
        checked += 1;
    }
    println!("js_binding_save_bytes_parity: {checked} 份用例逐字节相同");
}
