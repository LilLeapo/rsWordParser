//! 属性表在语料上的冒烟测试（任务 1.1，`PROP-02/05/07/09`）：
//! 每个 XML part 里的每个 `w:rPr` 都能读；read → emit → read 建模字段相等；
//! 顺序表与 Word 实际输出顺序对照（`PROP-05` 的"待语料校验"）。

mod common;

use std::collections::BTreeMap;

use rsword::package::{Package, PartFlavor, PartId};
use rsword::semantic::props::{
    PropsPatch, RunProps, diff_run_props, emit_run_props, order_index_run_props, read_run_props,
};
use rsword::xml::{Dom, LocalName, QName};

const W_T: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";

#[test]
fn prop_07_run_props_roundtrip_on_corpus() {
    let mut docs = 0usize;
    let mut containers = 0usize;
    let mut bad_values = 0usize;
    let mut unknown_children: BTreeMap<String, usize> = BTreeMap::new();
    let mut order_violations: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut sequences = 0usize;
    let rpr = QName::w(LocalName::RPr);

    for kind in ["synthetic", "hostile"] {
        for path in common::docx_paths(kind) {
            let bytes = std::fs::read(&path).unwrap();
            let Ok(mut pkg) = Package::open(&bytes) else { continue };
            docs += 1;
            let ids: Vec<PartId> = pkg.parts().iter().filter(|p| p.is_xml).map(|p| p.id).collect();
            for id in ids {
                let Ok(Some(dom)) = pkg.dom(id) else { continue };
                let mut scratch = Dom::parse(
                    PartId(0),
                    format!(r#"<w:r xmlns:w="{W_T}" xmlns:w14="{W14}"/>"#).as_bytes(),
                )
                .unwrap();
                for node in dom.descendants(dom.root()).filter(|&n| dom.is(n, rpr)) {
                    containers += 1;
                    let mut diags = Vec::new();
                    let props = read_run_props(dom, Some(node), &mut diags);
                    bad_values += diags.len();

                    // 顺序表对照：子元素序号应单调不减
                    let mut prev: Option<(u16, String)> = None;
                    let mut seq_ok = true;
                    for child in dom.semantic_children(node) {
                        let Some(name) = dom.name(child) else { continue };
                        let text = name.display(dom.interner()).to_string();
                        match order_index_run_props(name) {
                            None => *unknown_children.entry(text).or_default() += 1,
                            Some(i) => {
                                if let Some((pi, pname)) = &prev
                                    && *pi > i
                                {
                                    *order_violations
                                        .entry((pname.clone(), text.clone()))
                                        .or_default() += 1;
                                    seq_ok = false;
                                }
                                prev = Some((i, text));
                            }
                        }
                    }
                    if seq_ok {
                        sequences += 1;
                    }

                    // read → emit → read
                    let e = emit_run_props(&props, PartFlavor::Transitional);
                    let id = e.materialize(&mut scratch);
                    let root = scratch.root();
                    scratch.append_child(root, id);
                    let mut again = read_run_props(&scratch, Some(id), &mut Vec::new());
                    again.text_fill = props.text_fill; // Raw 字段不由 emit 生成
                    assert!(
                        diff_run_props(&props, &again).is_empty(),
                        "{}: rPr 往返不等\n{props:#?}\n{again:#?}",
                        path.display()
                    );
                    assert_eq!(again, props);
                }
            }
        }
    }

    eprintln!(
        "props: {docs} docs, {containers} rPr, {bad_values} PROP_BAD_VALUE, {sequences} in schema order"
    );
    eprintln!("props: children outside the rPr order table: {unknown_children:?}");
    eprintln!("props: order violations (prev, next): {order_violations:?}");
    assert!(docs > 100, "corpus missing? {docs} docs");
    assert!(containers > 1000, "too few rPr: {containers}");
    let _ = RunProps::default();
}
