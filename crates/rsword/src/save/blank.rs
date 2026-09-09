//! 空白文档模板（`SAVE-05`，`spec/18` 7.8）：TS `blank.ts` `buildBlankDocx` 的逐字移植。
//!
//! 「新建文档」与「AI 从零生成」的底子。生成层只引用**包里已有**的样式与编号，所以这份模板
//! 必须自带标准那一套：`Normal`、`Heading1`–`6`、`ListParagraph`、`Hyperlink`、`TOC1`–`9`
//! （生成的 TOC 字段要用），外加项目符号（`numId 1`）与十进制（`numId 2`）两条编号定义。
//!
//! 六个 part 与 TS 的输出**逐字节相同**（`fixtures/fieldgen/blank.json` 是 TS 那边导出的对照件），
//! 所以两个引擎新建的文档打开后没有任何差别。TS 的这份模板里**没有** `word/settings.xml`
//! （`spec/18` 7.8 的清单多写了一项）：缺它 Word 照样打开，补一个反而与 TS 有差。

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::error::{Error, Result};

const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n";

const DOC_NS: &str = concat!(
    r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" "#,
    r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" "#,
    r#"xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math""#,
);

/// 模板里项目符号列表的 `w:numId`（TS `BLANK_BULLET_NUM_ID`）。
pub const BLANK_BULLET_NUM_ID: &str = "1";
/// 模板里有序（十进制）列表的 `w:numId`（TS `BLANK_ORDERED_NUM_ID`）。
pub const BLANK_ORDERED_NUM_ID: &str = "2";

/// `generateTocFieldXml` 引用的目录条目样式（缩进逐级加）。
fn toc_style(level: u32) -> String {
    let ind = if level > 1 {
        format!(r#"<w:ind w:left="{}"/>"#, 220 * (level - 1))
    } else {
        String::new()
    };
    format!(
        concat!(
            r#"<w:style w:type="paragraph" w:styleId="TOC{level}"><w:name w:val="toc {level}"/>"#,
            r#"<w:basedOn w:val="Normal"/><w:next w:val="Normal"/>"#,
            r#"<w:pPr><w:spacing w:after="100"/>{ind}</w:pPr></w:style>"#
        ),
        level = level,
        ind = ind,
    )
}

fn heading_style(level: u32, size_half_points: u32) -> String {
    format!(
        concat!(
            r#"<w:style w:type="paragraph" w:styleId="Heading{level}"><w:name w:val="heading {level}"/>"#,
            r#"<w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/>"#,
            r#"<w:pPr><w:keepNext/><w:spacing w:before="{before}" w:after="120"/>"#,
            r#"<w:outlineLvl w:val="{lvl}"/></w:pPr>"#,
            r#"<w:rPr><w:b/><w:sz w:val="{sz}"/><w:szCs w:val="{sz}"/></w:rPr></w:style>"#
        ),
        level = level,
        before = if level <= 2 { 240 } else { 160 },
        lvl = level - 1,
        sz = size_half_points,
    )
}

fn styles_xml(east_asia_font: Option<&str>) -> String {
    let ea = east_asia_font.map_or(String::new(), |f| format!(r#" w:eastAsia="{f}""#));
    let mut s = String::from(XML_DECL);
    s.push_str(
        r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
    );
    s.push_str("<w:docDefaults><w:rPrDefault><w:rPr>");
    s.push_str(&format!(
        r#"<w:rFonts w:ascii="Calibri"{ea} w:hAnsi="Calibri"/><w:sz w:val="22"/><w:szCs w:val="22"/>"#
    ));
    s.push_str("</w:rPr></w:rPrDefault>");
    s.push_str(concat!(
        r#"<w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="276" w:lineRule="auto"/></w:pPr>"#,
        r#"</w:pPrDefault></w:docDefaults>"#
    ));
    s.push_str(concat!(
        r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal">"#,
        r#"<w:name w:val="Normal"/><w:qFormat/></w:style>"#
    ));
    for (level, size) in [(1, 32), (2, 28), (3, 26), (4, 24), (5, 22), (6, 22)] {
        s.push_str(&heading_style(level, size));
    }
    s.push_str(concat!(
        r#"<w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/>"#,
        r#"<w:basedOn w:val="Normal"/>"#,
        r#"<w:pPr><w:ind w:left="720"/><w:contextualSpacing/></w:pPr></w:style>"#
    ));
    s.push_str(concat!(
        r#"<w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/>"#,
        r#"<w:rPr><w:color w:val="0563C1"/><w:u w:val="single"/></w:rPr></w:style>"#
    ));
    for level in 1..=9 {
        s.push_str(&toc_style(level));
    }
    s.push_str("</w:styles>");
    s
}

/// 项目符号的五级（`&#61623;` 是 Symbol 字体里的实心圆点，原样写进文件）。
fn bullet_levels() -> String {
    (0..5)
        .map(|ilvl| {
            format!(
                concat!(
                    r#"<w:lvl w:ilvl="{ilvl}"><w:start w:val="1"/><w:numFmt w:val="bullet"/>"#,
                    r#"<w:lvlText w:val="&#61623;"/>"#,
                    r#"<w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{left}" w:hanging="360"/></w:pPr>"#,
                    r#"<w:rPr><w:rFonts w:ascii="Symbol" w:hAnsi="Symbol" w:hint="default"/></w:rPr></w:lvl>"#
                ),
                ilvl = ilvl,
                left = 720 * (ilvl + 1),
            )
        })
        .collect()
}

fn decimal_levels() -> String {
    (0..5)
        .map(|ilvl| {
            format!(
                concat!(
                    r#"<w:lvl w:ilvl="{ilvl}"><w:start w:val="1"/><w:numFmt w:val="decimal"/>"#,
                    r#"<w:lvlText w:val="%{n}."/>"#,
                    r#"<w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{left}" w:hanging="360"/></w:pPr></w:lvl>"#
                ),
                ilvl = ilvl,
                n = ilvl + 1,
                left = 720 * (ilvl + 1),
            )
        })
        .collect()
}

/// 模板的 `word/numbering.xml`（TS `BLANK_NUMBERING_XML`）。文档缺这个 part 时也拿它当底子。
pub fn blank_numbering_xml() -> String {
    format!(
        concat!(
            "{decl}",
            r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
            r#"<w:abstractNum w:abstractNumId="0">{bullet}</w:abstractNum>"#,
            r#"<w:abstractNum w:abstractNumId="1">{decimal}</w:abstractNum>"#,
            r#"<w:num w:numId="{bullet_id}"><w:abstractNumId w:val="0"/></w:num>"#,
            r#"<w:num w:numId="{ordered_id}"><w:abstractNumId w:val="1"/></w:num>"#,
            "</w:numbering>"
        ),
        decl = XML_DECL,
        bullet = bullet_levels(),
        decimal = decimal_levels(),
        bullet_id = BLANK_BULLET_NUM_ID,
        ordered_id = BLANK_ORDERED_NUM_ID,
    )
}

/// 空白模板的六个 part（名字 → 内容），按 TS 写进 zip 的顺序。
pub fn blank_parts(east_asia_font: Option<&str>) -> Vec<(&'static str, String)> {
    let content_types = format!(
        concat!(
            "{decl}",
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
            r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
            r#"<Default Extension="xml" ContentType="application/xml"/>"#,
            r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
            r#"<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>"#,
            r#"<Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>"#,
            "</Types>"
        ),
        decl = XML_DECL,
    );
    let root_rels = format!(
        concat!(
            "{decl}",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
            r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>"#,
            "</Relationships>"
        ),
        decl = XML_DECL,
    );
    let doc_rels = format!(
        concat!(
            "{decl}",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
            r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
            r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>"#,
            "</Relationships>"
        ),
        decl = XML_DECL,
    );
    let document = format!(
        concat!(
            "{decl}",
            "<w:document {ns}><w:body><w:p/>",
            r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>"#,
            r#"<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/>"#,
            "</w:sectPr></w:body></w:document>"
        ),
        decl = XML_DECL,
        ns = DOC_NS,
    );
    vec![
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", root_rels),
        ("word/_rels/document.xml.rels", doc_rels),
        ("word/styles.xml", styles_xml(east_asia_font)),
        ("word/numbering.xml", blank_numbering_xml()),
        ("word/document.xml", document),
    ]
}

/// 一份最小可用的 `.docx`：一个空段、A4 竖向、标准样式（TS `buildBlankDocx`）。
///
/// `east_asia_font` 是 `docDefaults` 的 `w:eastAsia`：按界面语言给，免得日语 / 韩语用户一上来
/// 就是简体中文字面。不给就不写这个属性——像 en-US 的 Word 文档那样，出现 CJK 时由 Word
/// 按文种替换。
pub fn blank_docx(east_asia_font: Option<&str>) -> Result<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::with_capacity(8 * 1024)));
    let deflate = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, xml) in blank_parts(east_asia_font) {
        writer.start_file(name, deflate).map_err(|e| Error::Zip(format!("start {name}: {e}")))?;
        writer.write_all(xml.as_bytes()).map_err(|e| Error::Zip(format!("write {name}: {e}")))?;
    }
    let cursor = writer.finish().map_err(|e| Error::Zip(format!("finish: {e}")))?;
    Ok(cursor.into_inner())
}
