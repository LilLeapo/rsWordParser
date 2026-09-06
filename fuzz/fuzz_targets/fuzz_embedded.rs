//! TEST-08 `fuzz_embedded`（M6 6.9）：任意字节当作一个 XML part 喂给嵌入对象的三个解析入口——
//! 图表 part（`ChartPart::build`）、SmartArt 数据 / 绘图 part（`diagram_text` / `diagram_shapes`）、
//! OMML（`tokens` / `to_mathml` / `to_latex`）以及画布（`canvas_display`）。不 panic、不死循环、不爆栈。
#![no_main]

use libfuzzer_sys::fuzz_target;
use rsword::model::chart::ChartPart;
use rsword::model::diagram::{canvas_display, diagram_shapes, diagram_text};
use rsword::model::omml::{latex, mathml, tokens};
use rsword::model::ColorScheme;
use rsword::package::PartId;
use rsword::xml::{Dom, LocalName, NsId, QName};

fuzz_target!(|data: &[u8]| {
    let Ok(dom) = Dom::parse(PartId(0), data) else { return };
    let mut warnings = Vec::new();
    // 图表：根是不是 chartSpace 都走一遍（不是时只记 MOD_UNPARSEABLE）
    let cp = ChartPart::build(PartId(0), Some(&dom), &ColorScheme::office_default(), &mut warnings);
    let _ = cp.display.map(|d| d.series.len());
    // 图示：数据 part 的文字树与绘图 part 的形状
    let _ = diagram_text(&dom).map(|t| t.len());
    let _ = diagram_shapes(&dom).len();
    // 公式与画布：文档里每一处
    for n in dom.descendants(dom.root()) {
        if dom.is(n, QName::new(NsId::M, LocalName::OMath)) {
            let _ = tokens(&dom, n).len();
            let _ = mathml::to_mathml(&dom, n).len();
            let _ = latex::to_latex(&dom, n).map(|s| s.len());
        } else if dom.is(n, QName::new(NsId::Lc, LocalName::LockedCanvas)) {
            let _ = canvas_display(&dom, n).shapes.len();
        }
    }
});
