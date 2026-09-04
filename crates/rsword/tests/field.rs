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
