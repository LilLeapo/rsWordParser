//! 图表的写侧（`EDIT-03` / `SAVE-05` / `SAVE-06`，`spec/17` 任务 6.6）：改缓存文本（`set_chart_data`）、
//! 新建图表（`materialize`：图表 part + 内嵌工作簿 + 关系 + 绘图段落）。
//!
//! `SetChartData` 只改缓存文本节点（TS `patchChartPartXml` 的语义）：数据引用 `c:f`、样式、布局一个字节不动，
//! 所以保存时只有被改的文本节点脏；锚不到的地方（没有标题、缓存里缺的点）留着不补。新图表的 part 内容按
//! TS `buildChartPartXml` / `buildChartWorkbookXlsxBase64` 的模板生成，再解析成 part 的 DOM——之后的编辑与
//! 别的 part 一样走 `MutationPlan`。

use std::io::{Cursor, Write};

use crate::diag::DiagCode;
use crate::edit::plan::{MutationPlan, MutationResult};
use crate::edit::{EditSession, NewBlock};
use crate::error::{Error, Result};
use crate::package::ns_context::NamespaceContext;
use crate::package::{PartId, RelType};
use crate::xml::entities::FragmentText;
use crate::xml::plan::{NewElement, NodeEdit, Target};
use crate::xml::{Dom, LocalName, NodeId, NsId, QName, parse_fragment};

named_enum! {
    /// 新图表的种类（TS `NewChart.kind`）：bar / line 带一对轴，pie 没有轴。
    pub enum NewChartKind {
        Bar = "bar",
        Line = "line",
        Pie = "pie",
    }
}

/// 一个新图表的数据（TS `NewChart`）。
#[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewChart {
    pub kind: NewChartKind,
    pub title: Option<String>,
    pub categories: Vec<String>,
    pub series: Vec<NewChartSeries>,
}

#[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewChartSeries {
    pub name: String,
    /// 按类别位置；`None` = 空档（缓存里不写这个点）。
    pub values: Vec<Option<f64>>,
}

/// `SetChartData` 的补丁（TS `ChartPatch`）：每一项 `None` = 不动；数组里的 `None` = 那一个不动。
#[derive(Debug, Clone, Default, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChartPatch {
    pub title: Option<String>,
    /// 与 `ChartDisplay.categories` 对齐。
    pub categories: Option<Vec<Option<String>>>,
    /// 与 `ChartDisplay.series` 对齐。
    pub series: Option<Vec<Option<ChartSeriesPatch>>>,
}

#[derive(Debug, Clone, Default, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChartSeriesPatch {
    pub name: Option<String>,
    /// 与 `ChartSeries.values` 对齐；`None` = 保留原值。
    pub values: Option<Vec<Option<f64>>>,
}

const CT_CHART: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
const CT_XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const NS_C: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
/// TS 缺省的图表尺寸：5486400 × 3200400 EMU（576 × 336 px）。
pub const DEFAULT_EXTENT_EMU: (i64, i64) = (5_486_400, 3_200_400);

fn c(local: LocalName) -> QName {
    QName::new(NsId::C, local)
}

fn a(local: LocalName) -> QName {
    QName::new(NsId::A, local)
}

// ---- 新建 -----------------------------------------------------------------------------------------

/// `NewBlock::Chart`（含包在 `Wrapped` 里的）→ 建好 part 后的 `NewBlock::Xml` 绘图段落；其他块原样返回。
/// 每个接收 `NewBlock` 的入口（`InsertBlock` / `UpdateBlockField` / 页眉页脚内容）都先过这里。
pub(crate) fn materialize(s: &mut EditSession, block: NewBlock) -> Result<NewBlock> {
    materialize_at(s, block, None)
}

/// 同 `materialize`，但知道这块要落在哪儿——`NewBlock::Caption` 的编号是「位置之前同标签的
/// `SEQ` 数 + 1」，非知道不可。
pub(crate) fn materialize_at(
    s: &mut EditSession,
    block: NewBlock,
    anchor: Option<crate::edit::BlockAt>,
) -> Result<NewBlock> {
    Ok(match block {
        NewBlock::Chart { chart, extent_emu } => {
            NewBlock::Xml(insert_chart_parts(s, &chart, extent_emu)?)
        }
        NewBlock::Image(img) => NewBlock::Xml(super::media_ops::image_paragraph(s, &img)?),
        // 独立公式段：先把 LaTeX 转成 OMML，再按 TS `mathParagraphXml` 生成整段
        NewBlock::MathPara { omml, align } => {
            let body = match &omml {
                crate::edit::NewMath::Omml(x) => x.clone(),
                crate::edit::NewMath::Latex(t) => crate::model::latex_to_omml(t)?,
            };
            let xml = crate::model::math_paragraph_xml(&body, &align);
            let main = s.main_part();
            let w_uri = NsId::W.uri(s.flavor()).expect("w 有两族 URI");
            let m_uri = crate::model::NS_M;
            let xml =
                xml.replacen("<w:p>", &format!(r#"<w:p xmlns:w="{w_uri}" xmlns:m="{m_uri}">"#), 1);
            let dom = s
                .package_mut()
                .dom_mut(main)?
                .ok_or_else(|| Error::edit(DiagCode::EditTargetOpaque, "主 part 没有 DOM"))?;
            let mut frags = crate::xml::parse_fragment(dom, &xml).map_err(|e| {
                Error::edit(DiagCode::EditPlanInvalid, format!("公式段落解析失败: {e}"))
            })?;
            NewBlock::Xml(
                frags
                    .pop()
                    .ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "公式段落为空"))?,
            )
        }
        NewBlock::Wrapped { wrapper, block } => {
            NewBlock::Wrapped { wrapper, block: Box::new(materialize_at(s, *block, anchor)?) }
        }
        // 7.8：块字段与题注
        NewBlock::Field(f) => NewBlock::Many(super::field_ops::materialize_field(s, f)?),
        NewBlock::Caption { label, text } => {
            super::field_ops::materialize_caption(s, &label, &text, anchor)?
        }
        NewBlock::Many(v) => NewBlock::Many(
            v.into_iter()
                .map(|b| materialize_at(s, b, anchor))
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flat_map(|b| match b {
                    NewBlock::Many(inner) => inner,
                    b => vec![b],
                })
                .collect(),
        ),
        // 7.7：新建文本框 / 形状 / 线条
        b @ (NewBlock::Textbox { .. } | NewBlock::Shape { .. } | NewBlock::Line { .. }) => {
            NewBlock::Xml(super::shape_gen::shape_paragraph(s, b)?)
        }
        other => other,
    })
}

/// 一批块：`NewBlock::Many` 就地摊平（生成器可以把一块展开成好几段）。
pub(crate) fn materialize_all(s: &mut EditSession, blocks: Vec<NewBlock>) -> Result<Vec<NewBlock>> {
    let mut out = Vec::with_capacity(blocks.len());
    for b in blocks {
        match materialize(s, b)? {
            NewBlock::Many(v) => out.extend(v),
            b => out.push(b),
        }
    }
    Ok(out)
}

/// 建图表 part、内嵌工作簿、两个 `.rels` 里的关系与内容类型，返回引用它的绘图段落。
///
/// part 名 `word/charts/chart{N}.xml` 取第一个空闲的 N（同一事务里刚建的也已登记在包里，所以也算）；
/// 工作簿 `word/charts/embeddings/workbook{N}.xlsx`（`Default Extension="xlsx"`）；图表 part 的 `.rels`
/// 第一条关系就是工作簿（`c:externalData r:id`）；主 part 的 `chart` 型关系；`wp:docPr/@id` 按 `EDIT-06`。
fn insert_chart_parts(
    s: &mut EditSession,
    chart: &NewChart,
    extent_emu: Option<(i64, i64)>,
) -> Result<NewElement> {
    let main = s.main_part();
    let n = (1u32..)
        .find(|n| s.package().find_name(&format!("word/charts/chart{n}.xml")).is_none())
        .expect("总有空闲的编号");
    let xml = chart_part_xml(chart, Some("rId1"));
    let (chart_part, chart_rid) =
        s.add_part(main, RelType::Chart, &format!("word/charts/chart{n}.xml"), CT_CHART, &xml)?;
    let xlsx = workbook_xlsx(&chart.categories, &chart.series);
    let (_, wb_rid) = s.add_binary_part(
        chart_part,
        RelType::Package,
        &format!("word/charts/embeddings/workbook{n}.xlsx"),
        CT_XLSX,
        xlsx,
    )?;
    if wb_rid != "rId1" {
        // 图表 part 的 `.rels` 里已经有别的关系（不该发生：part 是刚建的）：把 `c:externalData` 指过去
        let dom = s.dom_in(Some(chart_part))?;
        let ext = dom
            .semantic_descendants(dom.root())
            .find(|&x| dom.is(x, c(LocalName::ExternalData)))
            .ok_or_else(|| {
                Error::edit(DiagCode::EditPlanInvalid, "新图表 part 里没有 c:externalData")
            })?;
        let mut plan = MutationPlan::new(chart_part);
        plan.node_edits.push(NodeEdit::SetAttr {
            node: Target::Node(ext),
            name: QName::new(NsId::R, LocalName::Id),
            value: wb_rid,
        });
        s.commit_plan(plan)?;
    }

    let (cx, cy) = extent_emu.unwrap_or(DEFAULT_EXTENT_EMU);
    let (cx, cy) = (cx.max(1), cy.max(1));
    let flavor = s.flavor();
    let dom = s.package_mut().dom_mut(main)?.expect("main part parsed");
    let doc_pr_id = next_doc_pr_id(dom);
    let ctx = NamespaceContext::from_dom(dom, flavor);
    // `wp` / `r` 借主 part 的声明（缺了就在 `w:drawing` 上补）；`a` / `c` 与 TS 一样就地声明
    let (wp, wp_decl) = prefix_or_decl(&ctx, NsId::Wp, "wp", NS_WP);
    let (r, r_decl) = prefix_or_decl(&ctx, NsId::R, "r", NS_R);
    let para = format!(
        concat!(
            r#"<w:p><w:r><w:drawing{wp_decl}{r_decl}><{wp}:inline distT="0" distB="0" distL="0" distR="0">"#,
            r#"<{wp}:extent cx="{cx}" cy="{cy}"/><{wp}:docPr id="{id}" name="Chart {id}"/>"#,
            r#"<a:graphic xmlns:a="{a}"><a:graphicData uri="{c_ns}">"#,
            r#"<c:chart xmlns:c="{c_ns}" {r}:id="{rid}"/></a:graphicData></a:graphic></{wp}:inline></w:drawing></w:r></w:p>"#
        ),
        wp_decl = wp_decl,
        r_decl = r_decl,
        wp = wp,
        cx = cx,
        cy = cy,
        id = doc_pr_id,
        a = NS_A,
        c_ns = NS_C,
        r = r,
        rid = chart_rid,
    );
    let mut frags = parse_fragment(dom, &para).map_err(|e| {
        Error::edit(DiagCode::EditPlanInvalid, format!("图表绘图段落解析失败: {e}"))
    })?;
    frags.pop().ok_or_else(|| Error::edit(DiagCode::EditPlanInvalid, "图表绘图段落为空"))
}

/// 命名空间在主 part 里的前缀；没声明 → 用缺省前缀并给出要补在 `w:drawing` 上的声明。
fn prefix_or_decl(ctx: &NamespaceContext, ns: NsId, default: &str, uri: &str) -> (String, String) {
    match ctx.prefix_for(ns) {
        Some(p) if !p.is_empty() => (p.to_string(), String::new()),
        _ => (default.to_string(), format!(r#" xmlns:{default}="{uri}""#)),
    }
}

/// `EDIT-06`：主 part 里全部 `wp:docPr/@id` 的最大值 + 1（TS 从 8000 起计数，差分容忍）。
fn next_doc_pr_id(dom: &Dom) -> i64 {
    super::media_ops::next_doc_pr_id(dom)
}

/// JS `String(number)` 的数字写法：整数不带小数点。
fn num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") }
}

/// Excel 列名：A、B、C …（系列 i 在 B 起的第 i 列）。
fn col_letter(i: usize) -> char {
    (b'A' + i as u8) as char
}

fn str_cache(values: &[String], f: &str) -> String {
    let pts: String = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            format!(
                r#"<c:pt idx="{i}"><c:v>{}</c:v></c:pt>"#,
                String::from(FragmentText::from(v.as_str()))
            )
        })
        .collect();
    format!(
        r#"<c:strRef><c:f>{}</c:f><c:strCache><c:ptCount val="{}"/>{pts}</c:strCache></c:strRef>"#,
        String::from(FragmentText::from(f)),
        values.len()
    )
}

fn num_cache(values: &[Option<f64>], f: &str) -> String {
    let pts: String = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|v| format!(r#"<c:pt idx="{i}"><c:v>{}</c:v></c:pt>"#, num(v))))
        .collect();
    format!(
        r#"<c:numRef><c:f>{}</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="{}"/>{pts}</c:numCache></c:numRef>"#,
        String::from(FragmentText::from(f)),
        values.len()
    )
}

/// 整个图表 part（TS `buildChartPartXml`）。`external_data_rid` 是工作簿关系（`c:externalData`）。
pub fn chart_part_xml(chart: &NewChart, external_data_rid: Option<&str>) -> String {
    let rows = chart.categories.len();
    let sers: String = chart
        .series
        .iter()
        .enumerate()
        .map(|(i, ser)| {
            let col = col_letter(i + 1);
            let values: Vec<Option<f64>> = ser.values.iter().copied().take(rows).collect();
            format!(
                r#"<c:ser><c:idx val="{i}"/><c:order val="{i}"/><c:tx>{}</c:tx><c:cat>{}</c:cat><c:val>{}</c:val></c:ser>"#,
                str_cache(std::slice::from_ref(&ser.name), &format!("Sheet1!${col}$1")),
                str_cache(&chart.categories, &format!("Sheet1!$A$2:$A${}", rows + 1)),
                num_cache(&values, &format!("Sheet1!${col}$2:${col}${}", rows + 1)),
            )
        })
        .collect();
    let plot = match chart.kind {
        NewChartKind::Pie => {
            format!(
                r#"<c:pieChart><c:varyColors val="1"/>{sers}<c:firstSliceAng val="0"/></c:pieChart>"#
            )
        }
        kind => {
            let axes = concat!(
                r#"<c:catAx><c:axId val="111111111"/><c:scaling><c:orientation val="minMax"/></c:scaling>"#,
                r#"<c:delete val="0"/><c:axPos val="b"/><c:crossAx val="222222222"/></c:catAx>"#,
                r#"<c:valAx><c:axId val="222222222"/><c:scaling><c:orientation val="minMax"/></c:scaling>"#,
                r#"<c:delete val="0"/><c:axPos val="l"/><c:crossAx val="111111111"/></c:valAx>"#
            );
            let inner = if kind == NewChartKind::Bar {
                format!(
                    r#"<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:varyColors val="0"/>{sers}<c:axId val="111111111"/><c:axId val="222222222"/></c:barChart>"#
                )
            } else {
                format!(
                    r#"<c:lineChart><c:grouping val="standard"/><c:varyColors val="0"/>{sers}<c:marker val="1"/><c:axId val="111111111"/><c:axId val="222222222"/></c:lineChart>"#
                )
            };
            format!("{inner}{axes}")
        }
    };
    let title = chart.title.as_deref().map_or(String::new(), |t| {
        format!(
            r#"<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>{}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val="0"/></c:title><c:autoTitleDeleted val="0"/>"#,
            String::from(FragmentText::from(t))
        )
    });
    let external = external_data_rid.map_or(String::new(), |rid| {
        format!(r#"<c:externalData r:id="{rid}"><c:autoUpdate val="0"/></c:externalData>"#)
    });
    let out = format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
            r#"<c:chartSpace xmlns:c="{c}" xmlns:a="{a}" xmlns:r="{r}">"#,
            r#"<c:chart>{title}<c:plotArea><c:layout/>{plot}</c:plotArea>"#,
            r#"<c:plotVisOnly val="1"/><c:dispBlanksAs val="gap"/></c:chart>{external}</c:chartSpace>"#
        ),
        c = NS_C,
        a = NS_A,
        r = NS_R,
        title = title,
        plot = plot,
        external = external
    );
    out
}

/// 最小但合法的 xlsx（TS `buildChartWorkbookXlsxBase64`）：一张 `Sheet1`，A 列类别、B/C… 列系列，首行系列名，
/// 文本走 `sharedStrings`（按出现顺序去重）。
pub fn workbook_xlsx(categories: &[String], series: &[NewChartSeries]) -> Vec<u8> {
    const DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
    let mut strings: Vec<String> = Vec::new();
    let mut si = |s: &str| -> usize {
        match strings.iter().position(|x| x == s) {
            Some(i) => i,
            None => {
                strings.push(s.to_string());
                strings.len() - 1
            }
        }
    };
    let mut header = format!(r#"<c r="A1" t="s"><v>{}</v></c>"#, si(""));
    for (j, ser) in series.iter().enumerate() {
        header.push_str(&format!(
            r#"<c r="{}1" t="s"><v>{}</v></c>"#,
            col_letter(j + 1),
            si(&ser.name)
        ));
    }
    let mut rows = String::new();
    for (i, cat) in categories.iter().enumerate() {
        let row = i + 2;
        let mut cells = format!(r#"<c r="A{row}" t="s"><v>{}</v></c>"#, si(cat));
        for (j, ser) in series.iter().enumerate() {
            if let Some(Some(v)) = ser.values.get(i) {
                cells.push_str(&format!(
                    r#"<c r="{}{row}"><v>{}</v></c>"#,
                    col_letter(j + 1),
                    num(*v)
                ));
            }
        }
        rows.push_str(&format!(r#"<row r="{row}">{cells}</row>"#));
    }
    let sheet = format!(
        r#"{DECL}<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1">{header}</row>{rows}</sheetData></worksheet>"#
    );
    let sst: String = strings
        .iter()
        .map(|s| format!("<si><t>{}</t></si>", String::from(FragmentText::from(s.as_str()))))
        .collect();
    let shared = format!(
        r#"{DECL}<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{n}" uniqueCount="{n}">{sst}</sst>"#,
        n = strings.len()
    );
    let workbook = format!(
        r#"{DECL}<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="{NS_R}"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let workbook_rels = format!(
        concat!(
            "{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
            r#"<Relationship Id="rId1" Type="{NS_R}/worksheet" Target="worksheets/sheet1.xml"/>"#,
            r#"<Relationship Id="rId2" Type="{NS_R}/sharedStrings" Target="sharedStrings.xml"/>"#,
            "</Relationships>"
        ),
        DECL = DECL,
        NS_R = NS_R
    );
    let top_rels = format!(
        concat!(
            "{DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
            r#"<Relationship Id="rId1" Type="{NS_R}/officeDocument" Target="xl/workbook.xml"/>"#,
            "</Relationships>"
        ),
        DECL = DECL,
        NS_R = NS_R
    );
    let content_types = format!(
        concat!(
            "{DECL}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
            r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
            r#"<Default Extension="xml" ContentType="application/xml"/>"#,
            r#"<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
            r#"<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
            r#"<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>"#,
            "</Types>"
        ),
        DECL = DECL
    );
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in [
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", top_rels),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", workbook_rels),
        ("xl/worksheets/sheet1.xml", sheet),
        ("xl/sharedStrings.xml", shared),
    ] {
        w.start_file(name, opts).expect("zip in memory");
        w.write_all(body.as_bytes()).expect("zip in memory");
    }
    w.finish().expect("zip in memory").into_inner()
}

// ---- 改数据 ---------------------------------------------------------------------------------------

/// `EDIT-03 SetChartData`：按 TS `patchChartPartXml` 的锚定规则改缓存文本。
///
/// 标题：`c:title` 里第一个 `a:t` 改、其余 `a:t` 清空；没有 `a:t` 则 `c:strCache/c:v`；两者都没有（自动标题）→
/// 在 `c:tx/c:rich/a:p` 的 `a:endParaRPr` 之前注入 `a:r/a:t`，`c:tx` 是无缓存 `strRef` → 整个换成 rich body，
/// 没有 `c:tx` → rich body 插为 `c:title` 第一个子元素。系列名 → `c:ser/c:tx` 下第一个 `c:v`；值 → `c:val`
/// 缓存点按 `idx` 改文本，缺的点不补；类别 → **每个**系列的 `c:cat` 都改（缓存按系列各存一份）。
pub(crate) fn set_chart_data(
    s: &mut EditSession,
    part: PartId,
    patch: &ChartPatch,
) -> Result<MutationResult> {
    let cp = s.document().chart_parts.get(&part).ok_or_else(|| {
        Error::edit(
            DiagCode::EditPlanInvalid,
            format!("part#{} 不是主 part 引用的图表 part", part.0),
        )
    })?;
    if cp.chartex {
        // TS 静默 no-op；这里明说（`docs/04` §8）
        return Err(Error::edit(
            DiagCode::EditUnsupported,
            "chartex（cx:chartSpace）图表只有降级显示，SetChartData 不支持",
        ));
    }
    let dom = s.dom_in(Some(part))?;
    let root = dom.root();
    let mut plan = MutationPlan::new(part);
    if let Some(title) = &patch.title {
        match dom.semantic_descendants(root).find(|&n| dom.is(n, c(LocalName::Title))) {
            Some(t) => {
                let mut texts: Vec<NodeId> =
                    dom.semantic_descendants(t).filter(|&n| dom.is(n, a(LocalName::T))).collect();
                if texts.is_empty() {
                    // strRef 标题把文字放在 c:strCache/c:v 里
                    texts = dom
                        .semantic_descendants(t)
                        .filter(|&n| dom.is(n, c(LocalName::V)))
                        .collect();
                }
                if texts.is_empty() {
                    inject_title(dom, t, title, &mut plan);
                } else {
                    for (i, tn) in texts.into_iter().enumerate() {
                        let text = if i == 0 { title.as_str() } else { "" };
                        super::ops::set_segment_text(dom, tn, text, &mut plan);
                    }
                }
            }
            // 整个 `c:title` 都没有（Word 的「无标题」图表就是删掉这个元素，`corpus/real/chart/chart-no-title`）：
            // 按 `CT_Chart` 的顺序建一个插在 `c:chart` 的最前面。TS 在这里什么都不做，请求被静默丢弃（`docs/04` §8）
            None => new_title(dom, root, title, &mut plan),
        }
    }
    let sers: Vec<NodeId> =
        dom.semantic_descendants(root).filter(|&n| dom.is(n, c(LocalName::Ser))).collect();
    for (i, ser) in sers.into_iter().enumerate() {
        let sp = patch.series.as_ref().and_then(|v| v.get(i)).and_then(Option::as_ref);
        if let Some(sp) = sp {
            if let Some(name) = &sp.name
                && let Some(tx) = child(dom, ser, c(LocalName::Tx))
                && let Some(v) = dom.semantic_descendants(tx).find(|&n| dom.is(n, c(LocalName::V)))
            {
                super::ops::set_segment_text(dom, v, name, &mut plan);
            }
            // 散点 / 气泡图把 y 值放在 `c:yVal`（`ChartPart::build` 的读侧同样是 `c:val ?? c:yVal`）：
            // 只认 `c:val` 会让「读得出来的值改不动」（`corpus/real/chart/chart-scatter` 等，`docs/04` §8）
            if let Some(values) = &sp.values
                && let Some(val) = child(dom, ser, c(LocalName::Val))
                    .or_else(|| child(dom, ser, c(LocalName::YVal)))
            {
                let texts: Vec<Option<String>> = values.iter().map(|v| v.map(num)).collect();
                point_edits(dom, val, &texts, &mut plan);
            }
        }
        if let Some(cats) = &patch.categories
            && let Some(cat) = child(dom, ser, c(LocalName::Cat))
        {
            point_edits(dom, cat, cats, &mut plan);
        }
    }
    if plan.node_edits.is_empty() {
        return Ok(MutationResult::default());
    }
    // 图表 part 不在正文投影里：让 `commit_plan` 整体重建，`Document.chart_parts` 才会跟上
    plan.structure_changed = true;
    s.commit_plan(plan)
}

fn child(dom: &Dom, node: NodeId, name: QName) -> Option<NodeId> {
    dom.semantic_children(node).find(|&x| dom.is(x, name))
}

/// 容器（`c:val` / `c:cat`）里每个带 `idx` 的缓存点：`texts[idx]` 有值就改它的第一个 `c:v`。
fn point_edits(dom: &Dom, container: NodeId, texts: &[Option<String>], plan: &mut MutationPlan) {
    for pt in dom.semantic_descendants(container).filter(|&n| dom.is(n, c(LocalName::Pt))) {
        let Some(idx) = dom
            .attr_value(pt, QName::new(NsId::None, LocalName::Idx))
            .and_then(|v| v.trim().parse::<usize>().ok())
        else {
            continue;
        };
        let Some(Some(text)) = texts.get(idx) else { continue };
        if let Some(v) = dom.semantic_descendants(pt).find(|&n| dom.is(n, c(LocalName::V))) {
            super::ops::set_segment_text(dom, v, text, plan);
        }
    }
}

/// 整个 `c:title` 元素都不存在：新建一个带文字的标题，按 `CT_Chart` 的顺序插在 `c:chart` 的最前面。
fn new_title(dom: &Dom, root: NodeId, text: &str, plan: &mut MutationPlan) {
    let Some(chart) = dom.semantic_descendants(root).find(|&n| dom.is(n, c(LocalName::Chart)))
    else {
        return;
    };
    let title = NewElement::new(c(LocalName::Title))
        .with_child(
            NewElement::new(c(LocalName::Tx)).with_child(
                NewElement::new(c(LocalName::Rich))
                    .with_child(NewElement::new(a(LocalName::BodyPr)))
                    .with_child(NewElement::new(a(LocalName::LstStyle)))
                    .with_child(
                        NewElement::new(a(LocalName::P)).with_child(
                            NewElement::new(a(LocalName::R))
                                .with_child(NewElement::new(a(LocalName::T)).with_text(text)),
                        ),
                    ),
            ),
        )
        .with_child(
            NewElement::new(c(LocalName::Overlay))
                .with_attr(QName::new(NsId::None, LocalName::Val), "0"),
        );
    let first = dom.semantic_children(chart).next();
    plan.node_edits.push(NodeEdit::Insert {
        parent: Target::Node(chart),
        before: first,
        node: title,
    });
}

/// 没有文字的标题（自动标题 / 无缓存的 strRef）：给它一个带文字的 run。
fn inject_title(dom: &Dom, title: NodeId, text: &str, plan: &mut MutationPlan) {
    let run = NewElement::new(a(LocalName::R))
        .with_child(NewElement::new(a(LocalName::T)).with_text(text));
    let tx = child(dom, title, c(LocalName::Tx));
    let p = tx.and_then(|tx| dom.semantic_descendants(tx).find(|&n| dom.is(n, a(LocalName::P))));
    if let Some(p) = p {
        // Word 的自动标题带一个只有 a:endParaRPr 的空 c:tx/c:rich 段落：run 按 schema 顺序插在它之前
        let end_pr = child(dom, p, a(LocalName::EndParaRPr));
        plan.node_edits.push(NodeEdit::Insert {
            parent: Target::Node(p),
            before: end_pr,
            node: run,
        });
        return;
    }
    let rich = NewElement::new(c(LocalName::Tx)).with_child(
        NewElement::new(c(LocalName::Rich))
            .with_child(NewElement::new(a(LocalName::BodyPr)))
            .with_child(NewElement::new(a(LocalName::LstStyle)))
            .with_child(NewElement::new(a(LocalName::P)).with_child(run)),
    );
    match tx {
        // 无缓存的 strRef（没有 a:p、没有 c:v）：没地方注入，整个 c:tx 换成 rich body（CT_Tx 是二选一）
        Some(tx) => plan.node_edits.push(NodeEdit::Replace { old: tx, node: rich }),
        None => {
            let first = dom.semantic_children(title).next();
            plan.node_edits.push(NodeEdit::Insert {
                parent: Target::Node(title),
                before: first,
                node: rich,
            });
        }
    }
}
