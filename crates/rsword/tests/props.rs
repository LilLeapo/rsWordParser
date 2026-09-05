//! 属性表在语料上的冒烟测试（任务 1.1 / 1.2，`PROP-02/05/07/09`）：
//! 每个 XML part 里的每个 `w:rPr` / `w:pPr` 都能读；read → emit → read 建模字段相等；
//! 顺序表与语料里的实际顺序对照（`PROP-05` 的"待语料校验"）。

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use rsword::package::{Package, PartFlavor, PartId};
use rsword::semantic::props::{
    CellProps, ParaProps, PropsPatch, RowProps, RunProps, SectionProps, TableProps,
    diff_cell_props, diff_para_props, diff_row_props, diff_run_props, diff_section_props,
    diff_table_props, emit_cell_props, emit_para_props, emit_row_props, emit_run_props,
    emit_section_props, emit_table_props, order_index_cell_props, order_index_para_props,
    order_index_row_props, order_index_run_props, order_index_section_props,
    order_index_table_props, read_cell_props, read_para_props, read_row_props, read_run_props,
    read_section_props, read_table_props,
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

/// 打印统计；`min_containers` 是语料里该容器至少该有的个数（少了说明语料或匹配出了问题）。
fn report(what: &str, st: &Stats, min_containers: usize) {
    eprintln!(
        "props[{what}]: {} docs, {} containers, {} PROP_BAD_VALUE, {} in schema order",
        st.docs, st.containers, st.bad_values, st.in_order
    );
    eprintln!("props[{what}]: children outside the order table: {:?}", st.unknown_children);
    eprintln!("props[{what}]: order violations (prev, next): {:?}", st.order_violations);
    assert!(st.docs > 100, "corpus missing? {} docs", st.docs);
    assert!(st.containers >= min_containers, "too few containers: {}", st.containers);
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
    report("rPr", &st, 1000);
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
            // sect_pr 从任务 5.1 起是嵌套表，emit 会生成，所以这里不再抄回来
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
    report("pPr", &st, 1000);
    let _ = ParaProps::default();
}

/// 表格属性容器的语料往返（任务 3.1，`PROP-07`）：四个容器同形，收成宏。可选的 `fixup` 把 emit
/// 不生成的 Raw 字段从原值抄回去再比较。
macro_rules! corpus_roundtrip {
    ($what:literal, $container:expr, $read:ident, $emit:ident, $diff:ident, $order:ident, $min:expr
     $(, fixup = |$again:ident, $orig:ident| $fix:block)?) => {{
        let st = for_each_container($container, $order, |dom, node, scratch, path| {
            let mut diags = Vec::new();
            let props = $read(dom, Some(node), &mut diags);
            let e = $emit(&props, PartFlavor::Transitional);
            let id = e.materialize(scratch);
            let root = scratch.root();
            scratch.append_child(root, id);
            #[allow(unused_mut)]
            let mut again = $read(scratch, Some(id), &mut Vec::new());
            $({
                let $again = &mut again;
                let $orig = &props;
                $fix
            })?
            assert!(
                $diff(&props, &again).is_empty(),
                "{}: {} 往返不等\n{props:#?}\n{again:#?}",
                path.display(),
                $what
            );
            assert_eq!(again, props);
            diags.len()
        });
        report($what, &st, $min);
    }};
}

#[test]
fn prop_07_cell_props_roundtrip_on_corpus() {
    corpus_roundtrip!(
        "tcPr",
        QName::w(LocalName::TcPr),
        read_cell_props,
        emit_cell_props,
        diff_cell_props,
        order_index_cell_props,
        100,
        fixup = |again, orig| {
            again.headers = orig.headers;
        }
    );
    let _ = CellProps::default();
}

#[test]
fn prop_07_row_props_roundtrip_on_corpus() {
    corpus_roundtrip!(
        "trPr",
        QName::w(LocalName::TrPr),
        read_row_props,
        emit_row_props,
        diff_row_props,
        order_index_row_props,
        5
    );
    let _ = RowProps::default();
}

#[test]
fn prop_07_table_props_roundtrip_on_corpus() {
    corpus_roundtrip!(
        "tblPr",
        QName::w(LocalName::TblPr),
        read_table_props,
        emit_table_props,
        diff_table_props,
        order_index_table_props,
        100
    );
    // 行级例外用同一张表
    corpus_roundtrip!(
        "tblPrEx",
        QName::w(LocalName::TblPrEx),
        read_table_props,
        emit_table_props,
        diff_table_props,
        order_index_table_props,
        0
    );
    let _ = TableProps::default();
}

/// 节属性容器的语料往返（任务 5.1，`PROP-07`）：`w:body` 与 `w:pPr` 下的 `w:sectPr` 都算。
#[test]
fn prop_07_section_props_roundtrip_on_corpus() {
    corpus_roundtrip!(
        "sectPr",
        QName::w(LocalName::SectPr),
        read_section_props,
        emit_section_props,
        diff_section_props,
        order_index_section_props,
        500
    );
    let _ = SectionProps::default();
}
