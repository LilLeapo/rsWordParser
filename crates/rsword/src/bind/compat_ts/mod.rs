//! `compat_ts`：把新模型投影为 TS `ParsedDoc` 的 JSON（`spec/10-compat-ts.md`），供编辑器零改动接入
//! 与差分测试（`TEST-03`）。M1 建立、M9 删除。
//!
//! 适配器只读规范状态与投影；所有"半解析"规则集中在本模块，注释标 `docs/01` 小节；无法复现的差异
//! 登记在 `KNOWN_DIFFS.md`。M1 范围：顶层字段、文本块、Run 映射、`docxIndex/originalXml/rawPPr/rawRPr`、
//! UTF-16 索引（`COMPAT-02/04/06/07`）；表格 / 图片 / 字段 / 页眉页脚随后续里程碑补齐。

use serde_json::{Map, Value, json};

use crate::error::Result;
use crate::model::Document;
use crate::package::{Package, RelType};
use crate::resolve::Resolver;

mod blocks;
mod decl;
pub mod diff;
pub mod save_blocks;
pub mod utf16;

pub use diff::{
    Diff, KNOWN_DIFFS_MD, KnownDiff, PathStat, Report, diff_json, filter_known, is_text_case,
    known_diffs, parse_known_diffs, path_key, path_matches, split_known,
};
pub use save_blocks::{SaveBlocksOutcome, apply_save_blocks, bookmark_id_of};
pub use utf16::Utf16Index;

/// 整份 `ParsedDoc`（含 `extras`），键与 TS 一致；`internal.originalBytes` 不输出（导出脚本也省略）。
pub fn parsed_doc(pkg: &mut Package) -> Result<Value> {
    let doc = Document::rebuild(pkg)?;
    Ok(parsed_doc_of(pkg, &doc))
}

/// 用已构建的模型投影（`pkg` 里的 part 已解析）。
pub fn parsed_doc_of(pkg: &Package, doc: &Document) -> Value {
    let main = doc.main_part;
    let dom = pkg.part(main).dom().expect("main part parsed by rebuild");
    let rels = &pkg.part(main).rels;
    let settings_dom = pkg
        .related(main, RelType::Settings)
        .next()
        .or_else(|| pkg.find_name("word/settings.xml"))
        .and_then(|id| pkg.part(id).dom());
    let resolver = Resolver::new(doc);
    let idx = Utf16Index::new(dom.src());
    let numbering = decl::numbering_json(doc);
    let ctx = blocks::Ctx::new(dom, doc, &resolver, &idx, rels, &numbering);
    let (elements, blocks) = blocks::body(&ctx);

    let mut o = Map::new();
    o.insert("blocks".into(), Value::Array(blocks));
    o.insert("comments".into(), decl::comments_json(doc));
    o.insert("footnotes".into(), decl::notes_json(doc, &resolver, false));
    o.insert("endnotes".into(), decl::notes_json(doc, &resolver, true));
    for k in ["sources", "inks"] {
        o.insert(k.into(), Value::Array(Vec::new()));
    }
    o.insert("themeFonts".into(), decl::theme_fonts_json(doc, &resolver));
    o.insert("themeColors".into(), decl::theme_colors_json(doc));
    if let Some(ft) = decl::font_table_json(doc) {
        o.insert("fontTable".into(), ft);
    }
    o.insert("protection".into(), decl::protection_json(doc));
    o.insert("writeProtection".into(), decl::write_protection_json(doc));
    // 页眉页脚（COMPAT-05，M5）：缺省值
    for k in [
        "headerText",
        "headerParas",
        "footerParas",
        "headerImages",
        "footerImages",
        "watermarkText",
        "footerText",
        "headerFirst",
        "footerFirst",
        "headerEven",
        "footerEven",
    ] {
        o.insert(k.into(), Value::Null);
    }
    o.insert("footerHasPageNumber".into(), Value::Bool(false));
    o.insert("headerHasPageNumber".into(), Value::Bool(false));
    o.insert("hfParts".into(), Value::Object(Map::new()));
    o.insert("titlePg".into(), Value::Bool(decl::title_pg(dom)));
    decl::settings_json(doc, settings_dom, &mut o);
    let styles = decl::styles_json(doc, &resolver);
    o.insert("styles".into(), styles.styles);
    if let Some(dd) = styles.doc_defaults {
        o.insert("docDefaults".into(), dd);
    }
    o.insert("headingStyleIds".into(), styles.heading_style_ids);
    if let Some(id) = styles.list_paragraph_style_id {
        o.insert("listParagraphStyleId".into(), Value::String(id));
    }
    o.insert("numbering".into(), numbering.defs);
    // internal：documentXml 与 body 首末子元素的 UTF-16 索引（COMPAT-06）
    let (inner_start, inner_end) = doc
        .body
        .map(|body| {
            let kids: Vec<_> =
                dom.children(body).iter().copied().filter(|&n| dom.name(n).is_some()).collect();
            match (kids.first(), kids.last()) {
                (Some(&f), Some(&l)) => {
                    let s = dom.node(f).lex.as_ref().map_or(0, |x| x.range.start);
                    let e = dom.node(l).lex.as_ref().map_or(0, |x| x.range.end);
                    (idx.at(dom.src(), s), idx.at(dom.src(), e))
                }
                _ => {
                    // 空 body：开标签结束 = 闭标签开始
                    let lex = dom.node(body).lex.as_ref();
                    let s = lex.map_or(0, |x| x.open.end);
                    let e = lex.map_or(0, |x| x.close.start);
                    (idx.at(dom.src(), s), idx.at(dom.src(), e))
                }
            }
        })
        .unwrap_or((0, 0));
    o.insert(
        "internal".into(),
        json!({ "documentXml": dom.src(), "bodyInnerStart": inner_start, "bodyInnerEnd": inner_end }),
    );
    o.insert(
        "extras".into(),
        json!({ "elements": Value::Array(elements), "chartParts": Value::Object(Map::new()) }),
    );
    Value::Object(o)
}
