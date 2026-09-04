//! `SaveBlock[]` 兼容映射（任务 1.13，`EDIT-04` / `COMPAT-08`）：用 TS 测试导出的
//! `corpus/synthetic/*.save.<k>.json`（`blocks` + `options`）驱动 `apply_save_blocks`，保存后主 part 与
//! 其中的 `documentXml`（TS `saveDocx` 的输出）按 `xml::canon` 规范化后相等——等价于任何 XPath 子集
//! 表达式在两者上结果相同。等价是**手段不是目标**：TS 不是验收权威，`INTENTIONAL` 列出我们有意做得
//! 不同的用例（`docs/04` §8）。M1 范围：`options` 只含 `savedAt` / `removePersonalInfo`（`SAVE-07`），
//! 块只含 original / generated / xml；用到字段、
//! 新超链接关系、块级修订等后续里程碑能力的用例记为"跳过"并列出原因。

mod common;

use std::collections::BTreeMap;

use rsword::bind::compat_ts::apply_save_blocks;
use rsword::diag::DiagCode;
use rsword::edit::EditSession;
use rsword::error::Error;
use rsword::package::{Package, PartId};
use rsword::xml::canon::first_difference;
use rsword::xml::{CanonOptions, Dom, LocalName, NsId, QName, canonical, xpath_strings};
use serde_json::Value;

/// 保存后 `w:p` 上的段落 id / rsid 与 TS 输出无关（TS 生成的段落是裸 `<w:p>`，我们复用原节点保留它们）；
/// `xml:space="preserve"`：`SAVE-03` 对 New `w:t` 一律写，TS 只对自己生成的文本写、逐字片段照抄。
fn ignore_attr(_: &Dom, element: QName, attr: QName) -> bool {
    if attr == QName::new(NsId::Xml, LocalName::Space) {
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
];

#[test]
fn compat_08_save_blocks_match_ts_save_docx_output() {
    let dir = common::corpus_dir("synthetic");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().contains(".save.")))
        .collect();
    files.sort();
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
