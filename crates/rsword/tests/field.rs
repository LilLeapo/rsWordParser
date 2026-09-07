//! `FLD-01`–`FLD-06`、`FLD-10`、`FLD-13`：字段配对、指令解析与策略（任务 2.4）。

mod common;

use rsword::diag::DiagCode;
use rsword::edit::{EditContext, EditOp, EditSession, InlinePos};
use rsword::package::{Package, PartId};
use rsword::span::field::form::read_form_data;
use rsword::span::field::{FieldForm, FieldIndex, FieldPolicy, FormData, InstrToken, Keyword};
use rsword::xml::{Dom, LocalName, NodeId, QName};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn dom(body: &str) -> Dom {
    let xml = format!(r#"<w:document xmlns:w="{W}"><w:body>{body}</w:body></w:document>"#);
    Dom::parse(PartId(0), xml.as_bytes()).unwrap()
}

fn index(body: &str) -> (Dom, FieldIndex) {
    let d = dom(body);
    let idx = FieldIndex::build(&d);
    (d, idx)
}

/// 一个复杂字段的三段：`begin` / `instrText` / `separate` / 结果 / `end`。
fn complex(instr: &str, result: &str) -> String {
    format!(
        concat!(
            r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
            r#"<w:r><w:instrText xml:space="preserve">{}</w:instrText></w:r>"#,
            r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>{}"#,
            r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#
        ),
        instr, result
    )
}

fn run(text: &str) -> String {
    format!("<w:r><w:t>{text}</w:t></w:r>")
}

fn find(dom: &Dom, local: LocalName) -> NodeId {
    dom.descendants(dom.root()).find(|&n| dom.is(n, QName::w(local))).unwrap()
}

// ---- FLD-02：配对 ----

/// `FLD-02` 验收行：begin 在 `w:hyperlink` 内、end 在其外的字段正确配对。
#[test]
fn fld_02_begin_inside_hyperlink_pairs_with_end_outside() {
    let body = format!(
        r#"<w:p><w:hyperlink r:id="rId9"><w:r><w:fldChar w:fldCharType="begin"/></w:r>
           <w:r><w:instrText> PAGE </w:instrText></w:r></w:hyperlink>
           <w:r><w:fldChar w:fldCharType="separate"/></w:r>{}
           <w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#,
        run("7")
    );
    let (d, idx) = index(&body.replace("r:id", "w:id"));
    assert_eq!(idx.len(), 1, "跨 w:hyperlink 边界仍配成一个字段");
    let f = &idx.fields()[0];
    assert_eq!(*f.keyword(), Keyword::Page);
    assert_eq!(f.policy, FieldPolicy::Atom);
    assert!(!f.cross_paragraph, "begin 与 end 在同一段落里");
    let FieldForm::Complex { begin, separate, end, instr_nodes, result_nodes } = &f.form else {
        panic!("复杂字段");
    };
    assert!(separate.is_some() && *begin != *end);
    assert_eq!(instr_nodes.len(), 1);
    assert_eq!(result_nodes.len(), 1);
    assert_eq!(d.text(d.children(find(&d, LocalName::T))[0]).unwrap(), "7");
    // 结构 run 与结果 run 都能反查到字段
    assert_eq!(idx.field_of(*begin).map(|f| f.id), Some(f.id));
    assert_eq!(idx.field_of(result_nodes[0]).map(|f| f.id), Some(f.id));
    assert!(idx.is_structure_run(*begin) && !idx.is_structure_run(result_nodes[0]));
}

/// `FLD-02` 验收行：嵌套 `IF { MERGEFIELD }` 得到 nested，占位符进指令文本（`FLD-03`）。
#[test]
fn fld_02_nested_field_in_the_instruction() {
    let inner = complex(" MERGEFIELD Name ", &run("Ada"));
    let body = format!(
        r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r>
           <w:r><w:instrText xml:space="preserve"> IF </w:instrText></w:r>{inner}
           <w:r><w:instrText xml:space="preserve"> = "Ada" "yes" "no" </w:instrText></w:r>
           <w:r><w:fldChar w:fldCharType="separate"/></w:r>{}
           <w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#,
        run("yes")
    );
    let (_, idx) = index(&body);
    assert_eq!(idx.len(), 2, "内层 MERGEFIELD 与外层 IF");
    // 嵌套字段先闭合，所以 id 在前
    let (inner_f, outer_f) = (&idx.fields()[0], &idx.fields()[1]);
    assert_eq!(*inner_f.keyword(), Keyword::MergeField);
    assert_eq!(*outer_f.keyword(), Keyword::If);
    assert_eq!(outer_f.nested, vec![inner_f.id]);
    assert_eq!(inner_f.parent, Some(outer_f.id));
    assert!(outer_f.instr.raw.contains('\u{FFFC}'), "指令里留占位符: {:?}", outer_f.instr.raw);
    assert_eq!(outer_f.instr.tokens[0], InstrToken::Nested(inner_f.id));
    assert_eq!(outer_f.instr.nested().collect::<Vec<_>>(), vec![inner_f.id]);
    assert_eq!(idx.roots().count(), 1);
}

/// `FLD-02` 第 5 条：`w:fldSimple` 直接成字段，子 `fldSimple` 是嵌套。
#[test]
fn fld_01_simple_field_and_nested_simple() {
    let body = format!(
        r#"<w:p><w:fldSimple w:instr=" MERGEFIELD Outer "><w:fldSimple w:instr=" PAGE ">{}</w:fldSimple></w:fldSimple></w:p>"#,
        run("3")
    );
    let (_, idx) = index(&body);
    assert_eq!(idx.len(), 2);
    let inner = &idx.fields()[0];
    let outer = &idx.fields()[1];
    assert_eq!(*inner.keyword(), Keyword::Page);
    assert_eq!(*outer.keyword(), Keyword::MergeField);
    assert_eq!(outer.nested, vec![inner.id]);
    assert!(matches!(outer.form, FieldForm::Simple { .. }));
    // 内层的结果 run 归内层；外层的 result_nodes 不含它（run 在内层的子树里）
    assert_eq!(inner.form.result_nodes().len(), 1);
    // 简单字段的指令来自 w:instr 属性，不是 instrText；结果区的嵌套不留占位符
    assert_eq!(outer.instr.raw, " MERGEFIELD Outer ");
    assert!(outer.instr.nested().next().is_none());
}

/// `FLD-02` 第 2/3 条：孤立的 separate 与 end 各记一条诊断，不产出字段。
#[test]
fn fld_02_stray_separate_and_end_are_diagnostics() {
    let body = r#"<w:p><w:r><w:fldChar w:fldCharType="separate"/></w:r>
       <w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    let (_, idx) = index(body);
    assert!(idx.is_empty());
    let codes: Vec<DiagCode> = idx.diagnostics().iter().map(|d| d.code).collect();
    assert_eq!(codes, vec![DiagCode::FldStraySeparate, DiagCode::FldStrayEnd]);
    assert!(
        idx.diagnostics()
            .iter()
            .all(|d| d.origin == rsword::ValidationOrigin::PreExistingDamage && d.range.is_some())
    );
}

/// `FLD-02` 第 7 条：字段禁止跨内容流配对（文本框是独立流）。
#[test]
fn fld_02_fields_never_pair_across_content_flows() {
    let body = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r>
       <w:r><w:pict><v:shape xmlns:v="urn:schemas-microsoft-com:vml"><v:textbox><w:txbxContent>
         <w:p><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>
       </w:txbxContent></v:textbox></v:shape></w:pict></w:r></w:p>"#;
    let (_, idx) = index(body);
    assert!(idx.is_empty(), "两端在不同的流里，不配对");
    let codes: Vec<DiagCode> = idx.diagnostics().iter().map(|d| d.code).collect();
    assert!(codes.contains(&DiagCode::FldStrayEnd), "{codes:?}");
    assert!(codes.contains(&DiagCode::FldUnclosed), "{codes:?}");
}

// ---- FLD-03：指令文本 ----

/// `FLD-03` 验收行：`PAGE` 拆成两个 `w:instrText`（`PA` + `GE`）仍识别为 PAGE。
#[test]
fn fld_03_instruction_split_across_runs_is_still_recognized() {
    let body = format!(
        r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r>
           <w:r><w:instrText xml:space="preserve"> PA</w:instrText></w:r>
           <w:r><w:instrText xml:space="preserve">GE </w:instrText></w:r>
           <w:r><w:fldChar w:fldCharType="separate"/></w:r>{}
           <w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#,
        run("1")
    );
    let (_, idx) = index(&body);
    let f = &idx.fields()[0];
    assert_eq!(f.instr.raw, " PAGE ", "拼接不 trim");
    assert_eq!(*f.keyword(), Keyword::Page);
    let FieldForm::Complex { instr_nodes, .. } = &f.form else { panic!() };
    assert_eq!(instr_nodes.len(), 2, "两个指令 run 都记下来（保存真相）");
}

/// `FLD-03`：`w:delInstrText` 拼进 raw 并标记指令被修订删除。
#[test]
fn fld_03_del_instr_text_marks_the_field() {
    let body = format!(
        r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r>
           <w:r><w:delInstrText xml:space="preserve"> PAGE </w:delInstrText></w:r>
           <w:r><w:fldChar w:fldCharType="separate"/></w:r>{}
           <w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#,
        run("2")
    );
    let (_, idx) = index(&body);
    let f = &idx.fields()[0];
    assert!(f.instr_deleted);
    assert_eq!(*f.keyword(), Keyword::Page, "策略照常");
    assert_eq!(f.policy, FieldPolicy::Atom);
}

// ---- FLD-04：fldChar 上的事实 ----

#[test]
fn fld_04_fld_char_carries_lock_dirty_and_ff_data() {
    let body = r#"<w:p><w:r><w:fldChar w:fldCharType="begin" w:fldLock="true" w:dirty="1">
         <w:ffData><w:name w:val="check1"/><w:enabled/><w:checkBox><w:sizeAuto/><w:default w:val="1"/></w:checkBox></w:ffData>
       </w:fldChar></w:r>
       <w:r><w:instrText> FORMCHECKBOX </w:instrText></w:r>
       <w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    let (d, idx) = index(body);
    let f = &idx.fields()[0];
    assert!(f.lock && f.dirty_flag);
    assert!(f.ff_data.is_some(), "begin run 里的 w:ffData");
    assert_eq!(f.policy, FieldPolicy::Form);
    // `FLD-10`：没有 w:checked 时取 w:default
    let Some(FormData::CheckBox { checked, default, size, .. }) = read_form_data(&d, f.ff_data)
    else {
        panic!("checkBox")
    };
    assert!(checked && default == Some(true) && size.is_none());
    assert_eq!(rsword::span::field::form_name(&d, f.ff_data).as_deref(), Some("check1"));
    // separate 缺省（无结果的字段）是合法的
    let FieldForm::Complex { separate, result_nodes, .. } = &f.form else { panic!() };
    assert!(separate.is_none() && result_nodes.is_empty());
}

/// `FLD-10`：`w:checked` 存在而无 `w:val` → true；没有 `w:checkBox` 的 FORMCHECKBOX 降为 `Unknown`。
#[test]
fn fld_10_checkbox_state_and_policy_downgrade() {
    let with_checked = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:default w:val="0"/><w:checked/></w:checkBox></w:ffData></w:fldChar></w:r>
       <w:r><w:instrText> FORMCHECKBOX </w:instrText></w:r>
       <w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    let (d, idx) = index(with_checked);
    let f = &idx.fields()[0];
    let Some(FormData::CheckBox { checked, default, .. }) = read_form_data(&d, f.ff_data) else {
        panic!()
    };
    assert!(checked, "w:checked 无 w:val 视为 true");
    assert_eq!(default, Some(false));

    let without = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r>
       <w:r><w:instrText> FORMCHECKBOX </w:instrText></w:r>
       <w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    let (_, idx) = index(without);
    assert_eq!(idx.fields()[0].policy, FieldPolicy::Unknown, "没有 w:checkBox → Unknown");

    let dropdown = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"><w:ffData><w:ddList><w:result w:val="1"/><w:listEntry w:val="a"/><w:listEntry w:val="b"/></w:ddList></w:ffData></w:fldChar></w:r>
       <w:r><w:instrText> FORMDROPDOWN </w:instrText></w:r>
       <w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    let (d, idx) = index(dropdown);
    let form = read_form_data(&d, idx.fields()[0].ff_data).unwrap();
    assert_eq!(form.selected_entry(), Some("b"));
}

// ---- FLD-06：策略 ----

/// `FLD-06` 覆盖规则：跨段字段一律 `Block`，与关键字无关。
#[test]
fn fld_06_cross_paragraph_field_is_always_block() {
    let body = format!(
        r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r>
           <w:r><w:instrText> REF x </w:instrText></w:r>
           <w:r><w:fldChar w:fldCharType="separate"/></w:r>{}</w:p>
           <w:p>{}<w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#,
        run("a"),
        run("b")
    );
    let (_, idx) = index(&body);
    let f = &idx.fields()[0];
    assert_eq!(*f.keyword(), Keyword::Ref, "关键字本来是 Atom");
    assert!(f.cross_paragraph);
    assert_eq!(f.policy, FieldPolicy::Block);
    assert!(f.is_block() && !f.is_atomic());
    assert_eq!(f.form.result_nodes().len(), 2, "结果横跨两段");
}

#[test]
fn fld_06_policies_come_from_the_keyword_table() {
    for (instr, keyword, policy) in [
        (" XE \"term\" ", Keyword::Xe, FieldPolicy::Marker),
        (" PAGEREF _Ref1 \\h ", Keyword::PageRef, FieldPolicy::Atom),
        (" HYPERLINK \"http://x\" ", Keyword::Hyperlink, FieldPolicy::Link),
        (" INCLUDEPICTURE \"p.png\" ", Keyword::IncludePicture, FieldPolicy::Picture),
        (" EMBED Excel.Sheet ", Keyword::Embed, FieldPolicy::Object),
        (" TOC \\o \"1-3\" ", Keyword::Toc, FieldPolicy::Block),
        (" ACMEPRIVATE 1 ", Keyword::Unknown("ACMEPRIVATE".into()), FieldPolicy::Unknown),
    ] {
        let body = format!("<w:p>{}</w:p>", complex(instr, &run("r")));
        let (_, idx) = index(&body);
        let f = &idx.fields()[0];
        assert_eq!(*f.keyword(), keyword, "{instr}");
        assert_eq!(f.policy, policy, "{instr}");
        assert_eq!(f.is_transparent(), policy == FieldPolicy::Link, "{instr}");
    }
}

// ---- FLD-02 第 6 条 / FLD-13：未闭合 ----

/// `FLD-02` 验收行：未闭合 begin 记诊断且段落可编辑；`corpus/hostile/field-unclosed.docx`。
#[test]
fn fld_02_unclosed_field_keeps_its_bytes_and_the_paragraph_stays_editable() {
    let path = common::corpus_dir("hostile").join("field-unclosed.docx");
    let bytes = std::fs::read(&path).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let main = pkg.main_part();
    let idx = FieldIndex::build(pkg.dom(main).unwrap().unwrap());
    assert!(idx.is_empty(), "未闭合的字段不产出 FieldSpan");
    assert_eq!(idx.diagnostics().len(), 1);
    assert_eq!(idx.diagnostics()[0].code, DiagCode::FldUnclosed);
    assert_eq!(idx.diagnostics()[0].origin, rsword::ValidationOrigin::PreExistingDamage);
    assert_eq!(pkg.save().unwrap(), bytes, "未编辑保存字节相同");

    // 第二段可编辑，第一段（含未闭合 begin）原字节不动
    let mut s = EditSession::open(&bytes).unwrap();
    let second = s.document().text_blocks().nth(1).unwrap().node;
    s.apply(
        EditOp::InsertText { at: InlinePos::new(second, 0), text: "Z".into(), props: None },
        &EditContext::default(),
    )
    .unwrap();
    assert!(
        s.diagnostics().iter().any(|d| d.code == DiagCode::FldUnclosed),
        "解析期诊断报到会话上: {:?}",
        s.diagnostics()
    );
    let out = s.save().unwrap();
    let mut pkg = Package::open(&out).unwrap();
    let main = pkg.main_part();
    let xml = pkg.dom(main).unwrap().unwrap().src().to_string();
    assert!(
        xml.contains(r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r><w:r><w:t>tail</w:t></w:r>"#),
        "未闭合字段所在段落一个字节都没动: {xml}"
    );
    assert!(xml.contains("Zhello"), "{xml}");
}

// ---- 全语料 ----

/// 语料里每个 `fldChar` 都被恰好一个字段认领（未闭合的除外），策略分布可复现。
#[test]
fn fld_02_every_field_in_the_corpus_is_accounted_for() {
    // 语料里本来就有 3 份带缺陷的字段夹具（TS 的截断测试用例，`w:fldChar` 不成对）
    const KNOWN_DAMAGED: &[(&str, usize, usize)] = &[
        // (文档, 未闭合 begin 数, 孤立 separate/end 数)
        ("field-display__001", 1, 0), // TOC 只留了第一条目录项，缺 end
        ("field-display__002", 0, 1), // 只有一个 fldChar end
        ("protected-text-edit__001", 1, 0), // 同 field-display__001
    ];
    let mut docs = 0usize;
    let mut fields = 0usize;
    let mut damaged: Vec<(String, usize, usize)> = Vec::new();
    let mut policies: std::collections::BTreeMap<String, usize> = Default::default();
    for path in common::docx_paths("synthetic") {
        let bytes = std::fs::read(&path).unwrap();
        let Ok(mut pkg) = Package::open(&bytes) else { continue };
        let ids: Vec<PartId> = (0..pkg.parts().len() as u32).map(PartId).collect();
        let mut doc_has_field = false;
        for id in ids {
            let Ok(Some(dom)) = pkg.dom(id) else { continue };
            let idx = FieldIndex::build(dom);
            // 每个 begin / end fldChar 要么属于某个字段，要么有诊断
            let begins: Vec<NodeId> = dom
                .descendants(dom.root())
                .filter(|&n| dom.is(n, QName::w(LocalName::FldChar)))
                .filter(|&n| {
                    dom.attr_value(n, QName::w(LocalName::FldCharType)).as_deref() == Some("begin")
                })
                .collect();
            let claimed = idx.fields().iter().filter(|f| f.form.is_complex()).count();
            let unclosed_here =
                idx.diagnostics().iter().filter(|d| d.code == DiagCode::FldUnclosed).count();
            assert_eq!(
                begins.len(),
                claimed + unclosed_here,
                "{}: {} 个 begin，认领 {}，未闭合 {}",
                path.display(),
                begins.len(),
                claimed,
                unclosed_here
            );
            for f in idx.fields() {
                *policies.entry(format!("{:?}", f.policy)).or_default() += 1;
                // 指令解析出的 raw 与节点原文一致（视图不改真相）
                if let FieldForm::Complex { instr_nodes, .. } = &f.form {
                    assert!(
                        instr_nodes.iter().all(|&n| dom.node(n).lex.is_some()),
                        "指令 run 都来自原文"
                    );
                }
            }
            fields += idx.fields().len();
            let stray_here = idx
                .diagnostics()
                .iter()
                .filter(|d| matches!(d.code, DiagCode::FldStrayEnd | DiagCode::FldStraySeparate))
                .count();
            if unclosed_here > 0 || stray_here > 0 {
                let stem = path.file_stem().unwrap().to_string_lossy().to_string();
                damaged.push((stem, unclosed_here, stray_here));
            }
            doc_has_field |= !idx.is_empty() || unclosed_here > 0;
        }
        if doc_has_field {
            docs += 1;
        }
    }
    eprintln!("fields: {docs} 份文档 / {fields} 个字段；带缺陷的夹具 {damaged:?}");
    eprintln!("policies: {policies:?}");
    assert!(fields >= 55, "语料里至少 55 个字段，实得 {fields}");
    let expected: Vec<(String, usize, usize)> =
        KNOWN_DAMAGED.iter().map(|&(n, u, s)| (n.to_string(), u, s)).collect();
    damaged.sort();
    assert_eq!(damaged, expected, "只有已登记的那 3 份夹具带字段缺陷");
}

// ---- 字段进模型与 compat（任务 2.5）----

use rsword::bind::compat_ts::parsed_doc;
use rsword::model::{Block, Document, Inline, ProtectedKind};
use serde_json::Value;

fn document(body: &str) -> (Package, Document) {
    let mut pkg = Package::open(&common::docx_with_body(body)).unwrap();
    let doc = Document::rebuild(&mut pkg).unwrap();
    (pkg, doc)
}

fn json_of(body: &str) -> Value {
    let mut pkg = Package::open(&common::docx_with_body(body)).unwrap();
    parsed_doc(&mut pkg).unwrap()
}

fn blocks(v: &Value) -> &Vec<Value> {
    v.get("blocks").unwrap().as_array().unwrap()
}

/// `MOD-06` / `FLD-14`：原子形态字段在坐标流里恒为 1 个 `U+FFFC`，与结果文字长度无关。
#[test]
fn mod_06_atomic_field_takes_one_coordinate_unit() {
    let (_pkg, doc) =
        document(&format!("<w:p>{}{}{}</w:p>", run("a"), complex(" PAGE ", &run("12")), run("b")));
    let tb = doc.text_blocks().next().unwrap();
    assert_eq!(tb.text(), "a\u{FFFC}b", "结果 `12` 只占 1 个单位");
    assert_eq!(tb.inlines.len(), 3);
    let Inline::Field { id, result } = &tb.inlines[1] else { panic!("{:?}", tb.inlines[1]) };
    assert_eq!(*doc.fields.get(*id).unwrap().keyword(), Keyword::Page);
    assert_eq!(result.len(), 1, "结果 run 仍在，只是不参与坐标");
    // 结构 run（begin / instrText / separate / end）不出现在 inlines 里
    assert!(tb.inlines.iter().all(|i| !matches!(i, Inline::Run(r)
        if r.segments.iter().any(|s| matches!(s.kind, rsword::model::SegmentKind::FldChar)))));
}

/// `FLD-07` 透明形态：`Link` 策略字段的结果 run 正常出现，带 `field` 与 `Link::Field`。
#[test]
fn fld_07_hyperlink_field_is_transparent() {
    let body =
        format!("<w:p>{}</w:p>", complex(r#" HYPERLINK "http://x" \o "tip" "#, &run("click")));
    let (_pkg, doc) = document(&body);
    let tb = doc.text_blocks().next().unwrap();
    assert_eq!(tb.text(), "click", "透明字段：结果就是段落文字");
    let runs: Vec<_> = tb
        .inlines
        .iter()
        .filter_map(|i| match i {
            Inline::Run(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(runs.len(), 5, "begin / instr / separate / 结果 / end 五个 run 都在");
    assert!(runs.iter().all(|r| r.field.is_some()), "结构 run 也带 field");
    let result = runs.iter().find(|r| !r.text.is_empty()).unwrap();
    assert!(matches!(result.link, Some(rsword::model::Link::Field(_))));
}

/// `MOD-05` R09 / `FLD-08`：`Block` 策略字段（TOC）的头段、尾段与其间的段落都是保护块。
#[test]
fn mod_05_r09_block_field_protects_its_paragraphs() {
    let body = format!(
        concat!(
            r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
            r#"<w:r><w:instrText> TOC \o "1-3" </w:instrText></w:r>"#,
            r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>{}</w:p>"#,
            "<w:p>{}</w:p>",
            r#"<w:p>{}<w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#,
            "<w:p>{}</w:p>"
        ),
        run("head"),
        run("middle"),
        run("tail"),
        run("after")
    );
    let (_pkg, doc) = document(&body);
    let kinds: Vec<String> = doc
        .main
        .iter()
        .map(|b| match b {
            Block::Protected(p) => format!("{:?}", p.kind),
            Block::Text(_) => "Text".into(),
            _ => "other".into(),
        })
        .collect();
    assert_eq!(kinds.len(), 4);
    for k in &kinds[..3] {
        assert!(k.starts_with("FieldBlockResult"), "{kinds:?}");
    }
    assert_eq!(kinds[3], "Text", "字段之后的段落照常可编辑");
    let field = doc.fields.fields().iter().find(|f| *f.keyword() == Keyword::Toc).unwrap();
    assert!(field.cross_paragraph && field.is_block());
    assert!(matches!(
        doc.main[1],
        Block::Protected(ref p) if matches!(p.kind, ProtectedKind::FieldBlockResult(id) if id == field.id)
    ));
}

/// `COMPAT-07`：可折叠字段折成一个 run（REF / XE / 简单内联 / FORMCHECKBOX）。
#[test]
fn compat_07_collapsible_fields_become_one_run() {
    let v = json_of(&format!(
        "<w:p>{}{}{}</w:p>",
        run("详见"),
        complex(r" REF 市场规模 \h ", &run("第二节")),
        run("一节。")
    ));
    let runs = blocks(&v)[0].get("runs").unwrap().as_array().unwrap();
    assert_eq!(runs.len(), 3, "{runs:?}");
    assert_eq!(runs[1]["refField"], "市场规模");
    assert_eq!(runs[1]["refInstr"], " REF 市场规模 \\h ");
    assert_eq!(runs[1]["text"], "第二节");

    let v = json_of(&format!("<w:p>{}{}</w:p>", run("热点"), complex(r#" XE "人工智能" "#, "")));
    let runs = blocks(&v)[0].get("runs").unwrap().as_array().unwrap();
    assert_eq!(runs[1]["xeTerm"], "人工智能");
    assert_eq!(runs[1]["text"], "", "XE 是零宽标记");

    let v = json_of(&format!("<w:p>{}</w:p>", complex(" PAGE ", "")));
    let runs = blocks(&v)[0].get("runs").unwrap().as_array().unwrap();
    assert_eq!(runs[0]["instrField"], "PAGE");
    assert_eq!(runs[0]["text"], " ", "没有结果的简单内联字段留一个空格");

    let checkbox = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:checked/></w:checkBox></w:ffData></w:fldChar></w:r>
        <w:r><w:instrText> FORMCHECKBOX </w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
    let v = json_of(checkbox);
    let runs = blocks(&v)[0].get("runs").unwrap().as_array().unwrap();
    assert_eq!(runs[0]["instrField"], "FORMCHECKBOX");
    assert_eq!(runs[0]["text"], "☒");
    assert!(runs[0]["fldBeginXml"].as_str().unwrap().contains("w:ffData"));
}

/// `COMPAT-07`：可转换 HYPERLINK 的结果 run 带 `link`；带别的开关的不折叠（整段 passthrough）。
#[test]
fn compat_07_convertible_hyperlink_gets_a_link() {
    let v = json_of(&format!(
        "<w:p>{}{}</w:p>",
        run("see "),
        complex(r#" HYPERLINK "http://x" \o "tip" "#, &run("x.org"))
    ));
    let runs = blocks(&v)[0].get("runs").unwrap().as_array().unwrap();
    assert_eq!(runs[1]["link"]["href"], "http://x");
    assert_eq!(runs[1]["link"]["tooltip"], "tip");

    // `\l` 锚点形式 TS 不折叠：整段 passthrough
    let v = json_of(&format!("<w:p>{}</w:p>", complex(r#" HYPERLINK \l "bm1" "#, &run("x"))));
    assert_eq!(blocks(&v)[0]["type"], "passthrough");
    assert_eq!(blocks(&v)[0]["label"], "Hyperlink field");
}

/// `COMPAT-03`：不可折叠字段的段落是 passthrough，带 `fieldDisplay`。
#[test]
fn compat_03_field_paragraph_is_passthrough_with_display() {
    // TOC 行：制表符切成 left / right，级别来自样式
    let v = json_of(&format!(
        r#"<w:p><w:pPr><w:pStyle w:val="TOC2"/></w:pPr><w:r><w:fldChar w:fldCharType="begin"/></w:r>
           <w:r><w:instrText> TOC \o "1-3" </w:instrText></w:r>
           <w:r><w:fldChar w:fldCharType="separate"/></w:r>{}<w:r><w:tab/></w:r>{}<w:r><w:tab/></w:r>{}
           <w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#,
        run("1.1."),
        run("背景"),
        run("7")
    ));
    let b = &blocks(&v)[0];
    assert_eq!(b["type"], "passthrough");
    assert_eq!(b["label"], "Auto TOC (updates when opened in Word)");
    assert_eq!(b["styleId"], "TOC2");
    assert_eq!(b["previewText"], "1.1.背景7");
    assert_eq!(b["fieldDisplay"]["kind"], "tocLine");
    assert_eq!(b["fieldDisplay"]["num"], "1.1.");
    assert_eq!(b["fieldDisplay"]["left"], "背景");
    assert_eq!(b["fieldDisplay"]["right"], "7");
    assert_eq!(b["fieldDisplay"]["level"], 2);
    assert!(b.get("runs").is_none() && b.get("format").is_none(), "只读块不出 runs / format");

    // 普通字段段落：整段文字 + 段落排版
    let v = json_of(&format!(
        r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr>{}{}</w:p>"#,
        run("图 "),
        complex(r" SEQ 图 \* ARABIC ", &run("1"))
    ));
    let b = &blocks(&v)[0];
    assert_eq!(b["label"], "Caption number field");
    assert_eq!(b["fieldDisplay"]["kind"], "text");
    assert_eq!(b["fieldDisplay"]["left"], "图 1");
    assert_eq!(b["fieldDisplay"]["align"], "center");

    // 只有孤立 fldChar end 的段落
    let v = json_of(
        r#"<w:p><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:br w:type="page"/></w:r></w:p>"#,
    );
    let b = &blocks(&v)[0];
    assert_eq!(b["label"], "Field end marker + page break");
    assert_eq!(b["fieldDisplay"]["kind"], "pageBreak");

    // 没有字段的目录样式段落（TS 规则 3）
    let v = json_of(&format!(
        r#"<w:p><w:pPr><w:pStyle w:val="TableofFigures"/></w:pPr>{}<w:r><w:tab/></w:r>{}</w:p>"#,
        run("表 2.1 语法"),
        run("11")
    ));
    let b = &blocks(&v)[0];
    assert_eq!(b["label"], "TOC entry");
    assert_eq!(b["fieldDisplay"]["level"], 1, "图表目录样式算 1 级");
    assert_eq!(b["fieldDisplay"]["left"], "表 2.1 语法");
}

/// `FLD-13` / `EDIT-05`：块只含跨段字段的一端时 `DeleteBlock` 当场拒绝，而不是让**每次**保存都栽在
/// `FLD_STRAY_END` 上（真实 Word 语料 `fields-toc-stale` 的 TOC 横跨四个块，`docs/09` 第三轮发现）。
#[test]
fn fld_13_delete_block_refuses_to_strand_a_field_end() {
    let path = common::corpus_dir("real").join("fields2/fields-toc-stale.docx");
    let bytes = std::fs::read(&path).expect("真实语料里的 fields-toc-stale.docx");
    let mut s = EditSession::open(&bytes).expect("open");
    let blocks: Vec<NodeId> = s
        .document()
        .main
        .iter()
        .filter(|b| {
            !matches!(b, rsword::model::Block::Protected(p)
                if p.kind == rsword::model::ProtectedKind::SectionProps)
        })
        .map(rsword::model::Block::node)
        .collect();
    // blocks[1] 带着 TOC 的 begin / separate，end 在 blocks[4]
    let err = s
        .apply(EditOp::DeleteBlock { part: None, node: blocks[1] }, &EditContext::default())
        .expect_err("应当拒绝");
    assert!(matches!(&err, rsword::Error::Edit { code: DiagCode::EditSplitField, .. }), "{err:?}");
    // 拒绝之后状态没动，保存照旧（以前这里会永远失败）
    assert_eq!(s.save().expect("save"), bytes, "拒绝的操作不该改动任何字节");
    // 对照：字段之外的块照样删得掉
    let mut s = EditSession::open(&bytes).expect("open");
    let last = *blocks.last().expect("末块");
    s.apply(EditOp::DeleteBlock { part: None, node: last }, &EditContext::default())
        .expect("字段之外的块可以删");
    assert!(s.save().is_ok());
}
