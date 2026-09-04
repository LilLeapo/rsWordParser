//! 属性表在语料上的冒烟测试（任务 1.1 / 1.2，`PROP-02/05/07/09`）：
//! 每个 XML part 里的每个 `w:rPr` / `w:pPr` 都能读；read → emit → read 建模字段相等；
//! 顺序表与语料里的实际顺序对照（`PROP-05` 的"待语料校验"）。

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use rsword::package::{Package, PartFlavor, PartId};
use rsword::semantic::props::{
    ParaProps, PropsPatch, RunProps, diff_para_props, diff_run_props, emit_para_props,
    emit_run_props, order_index_para_props, order_index_run_props, read_para_props, read_run_props,
};
use rsword::xml::{Dom, LocalName, QName};

const W_T: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";

#[derive(Default)]
struct Stats {
    docs: usize,
    containers: usize,
    bad_values: usize,
    in_order: usize,
    unknown_children: BTreeMap<String, usize>,
    order_violations: BTreeMap<(String, String), usize>,
}

/// 对语料里所有 XML part 的每个 `container` 元素调用 `f(dom, node, scratch, path)`，
/// 并统计子元素顺序是否符合 `order_index`。
fn for_each_container(
    container: QName,
    order_index: fn(QName) -> Option<u16>,
    mut f: impl FnMut(&Dom, rsword::xml::NodeId, &mut Dom, &Path) -> usize,
) -> Stats {
    let mut st = Stats::default();
    for kind in ["synthetic", "hostile"] {
        for path in common::docx_paths(kind) {
            let bytes = std::fs::read(&path).unwrap();
            let Ok(mut pkg) = Package::open(&bytes) else { continue };
            st.docs += 1;
            let ids: Vec<PartId> = pkg.parts().iter().filter(|p| p.is_xml).map(|p| p.id).collect();
            for id in ids {
                let Ok(Some(dom)) = pkg.dom(id) else { continue };
                let mut scratch = Dom::parse(
                    PartId(0),
                    format!(r#"<w:p xmlns:w="{W_T}" xmlns:w14="{W14}"/>"#).as_bytes(),
                )
                .unwrap();
                for node in dom.descendants(dom.root()).filter(|&n| dom.is(n, container)) {
                    st.containers += 1;
                    st.bad_values += f(dom, node, &mut scratch, &path);
                    let mut prev: Option<(u16, String)> = None;
                    let mut ok = true;
                    for child in dom.semantic_children(node) {
                        let Some(name) = dom.name(child) else { continue };
                        let text = name.display(dom.interner()).to_string();
                        match order_index(name) {
                            None => *st.unknown_children.entry(text).or_default() += 1,
                            Some(i) => {
                                if let Some((pi, pname)) = &prev
                                    && *pi > i
                                {
                                    *st.order_violations
                                        .entry((pname.clone(), text.clone()))
                                        .or_default() += 1;
                                    ok = false;
                                }
                                prev = Some((i, text));
                            }
                        }
                    }
                    if ok {
                        st.in_order += 1;
                    }
                }
            }
        }
    }
    st
}

fn report(what: &str, st: &Stats) {
    eprintln!(
        "props[{what}]: {} docs, {} containers, {} PROP_BAD_VALUE, {} in schema order",
        st.docs, st.containers, st.bad_values, st.in_order
    );
    eprintln!("props[{what}]: children outside the order table: {:?}", st.unknown_children);
    eprintln!("props[{what}]: order violations (prev, next): {:?}", st.order_violations);
    assert!(st.docs > 100, "corpus missing? {} docs", st.docs);
    assert!(st.containers > 1000, "too few containers: {}", st.containers);
}

#[test]
fn prop_07_run_props_roundtrip_on_corpus() {
    let st = for_each_container(
        QName::w(LocalName::RPr),
        order_index_run_props,
        |dom, node, scratch, path| {
            let mut diags = Vec::new();
            let props = read_run_props(dom, Some(node), &mut diags);
            let e = emit_run_props(&props, PartFlavor::Transitional);
            let id = e.materialize(scratch);
            let root = scratch.root();
            scratch.append_child(root, id);
            let mut again = read_run_props(scratch, Some(id), &mut Vec::new());
            again.text_fill = props.text_fill; // Raw 字段不由 emit 生成
            assert!(
                diff_run_props(&props, &again).is_empty(),
                "{}: rPr 往返不等\n{props:#?}\n{again:#?}",
                path.display()
            );
            assert_eq!(again, props);
            diags.len()
        },
    );
    report("rPr", &st);
    let _ = RunProps::default();
}

#[test]
fn prop_07_para_props_roundtrip_on_corpus() {
    let st = for_each_container(
        QName::w(LocalName::PPr),
        order_index_para_props,
        |dom, node, scratch, path| {
            let mut diags = Vec::new();
            let props = read_para_props(dom, Some(node), &mut diags);
            let e = emit_para_props(&props, PartFlavor::Transitional);
            let id = e.materialize(scratch);
            let root = scratch.root();
            scratch.append_child(root, id);
            let mut again = read_para_props(scratch, Some(id), &mut Vec::new());
            again.sect_pr = props.sect_pr;
            if let (Some(a), Some(b)) = (&mut again.rpr, &props.rpr) {
                a.text_fill = b.text_fill;
            }
            assert!(
                diff_para_props(&props, &again).is_empty(),
                "{}: pPr 往返不等\n{props:#?}\n{again:#?}",
                path.display()
            );
            assert_eq!(again, props);
            diags.len()
        },
    );
    report("pPr", &st);
    let _ = ParaProps::default();
}
