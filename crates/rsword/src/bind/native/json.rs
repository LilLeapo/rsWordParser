//! `BIND-02` 模型 JSON 投影（`spec/21-bind.md`，任务 8.2）。
//!
//! `document()` 是 `Document`（`MOD-01`）的**投影，不是第三个模型**：字段名 = Rust 字段名的
//! camelCase，枚举值 = `named_enum!` 的 `as_str` 字串，单位按 `spec/00` §0.4 原值（twips /
//! half-points / eighth-points / EMU / UTF-16 code unit；旋转 1/60000 度）。**禁止**在 `model/`
//! 类型上 `derive(Serialize)`（模型持 `NodeId` / `Range<u32>` / interner 句柄），投影是这里的
//! 独立一层：每个模型类型一张 [`model_json!`] 表，同表展开 `impl ToJson`、JSON Schema 片段与
//! 「不丢字段」覆盖测试（门 1）。
//!
//! 约定（`spec/19`「分层决策」与评审裁定）：
//! - 带载荷枚举投成内标签对象 `{"kind": "<camelVariant>", …}`；无字段变体也是 `{"kind": …}`。
//! - 区间与元组一律 `[start, end]` / `[x, y]` 二元数组（同 `BIND-07` 诊断的 `range`）。
//! - id（`nodeId` / `partId` / `spanId` / `fieldId` / `revisionId` / `mediaId`）一律整数，
//!   会话内稳定、跨会话无意义（决策 6）。
//! - 布尔字段为真才写 `true`、缺席即假（`set_if!`）；`Option` 字段有值才写（`set_some!`）。
//! - 显示模型（`MOD-11`）不进默认 JSON；[`DocumentOpts::display`] 为真才投影（决策 4）。
//! - `QName` 含 `Other` / `Unbound`（interner 句柄）时无法脱离所在 part 的 interner 还原，
//!   投影为 `"?"`（已知名不受限）。触发处：`ProtectedKind::Unknown`、`SegmentKind::Other`、
//!   `AtomKind::Other`、`CompatFacts.flags`。
//!
//! `set` / [`set_some!`] / [`set_if!`] / [`display_json!`] 是本投影层的基础宏（原在
//! `bind/compat_ts/json.rs`，8.2 搬到此处，`compat_ts` 反向引用）。

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::ops::Range;

use serde_json::{Map, Value};

use crate::diag::Diagnostic;
use crate::model::{
    Block, ChartPart, Comment, DiagramPart, Document, FontTable, HfPart, InkInfo, Note, Numbering,
    RevisionEntry, SectionInfo, Settings, Source, Styles, Theme,
};
use crate::package::{MediaId, MediaKind, MediaStore, Package, PartId};
use crate::semantic::props::{HexColorOrAuto, Measure, Val};
use crate::span::RangeSpan;
use crate::span::field::FieldSpan;
use crate::xml::NodeId;
use crate::xml::QName;

pub use super::schema::SchemaDefs;
use super::schema::{bool_schema, int_schema, num_schema, str_schema};

pub mod block;
pub mod decl;
pub mod diag;
pub mod display;
pub mod facts;
pub mod hf;
pub mod ink;
pub mod inline;
pub mod notes;
pub mod sdt;
pub mod section;
pub mod sources;
pub mod span;
pub mod table;
pub mod theme;

mod props_gen {
    //! 生成属性表（`PROP-07` 的 30 张表与 `types.toml` 的枚举 / 属性结构体）的 JSON 投影与
    //! schema：由 `build/props.rs` 从同一份 TOML 元数据发射（`$OUT_DIR/props_json.rs`），
    //! 字段永不漂移。
    use crate::semantic::props::*;
    use crate::xml::NodeId;

    include!(concat!(env!("OUT_DIR"), "/props_json.rs"));
}

pub mod revision;

// ---- 投影上下文与入口 --------------------------------------------------------------------------

/// 投影上下文：包（`media[]` 条目用）与 `BIND-02` 决策 4 的显示模型开关。
pub struct ProjCx<'a> {
    pub pkg: &'a Package,
    /// `true` 才投影显示模型（`ChartDisplay` / `VmlDisplay` / `DiagramDisplay` / `AnchorGeom` 等）。
    pub display: bool,
}

/// 完整模型投影选项；`SessionTable::document` 的 JSON 参数另支持 BIND-10 的预算裁剪。
#[derive(Debug, Clone, Copy, Default)]
pub struct DocumentOpts {
    /// 显示模型投影，缺省 `false`（`BIND-02` 决策 4）。
    pub display: bool,
}

/// `document()` 的返回类型（`BIND-02` 顶层形态；投影本体是 `Document` 的 [`model_json!`] 表）。
/// 单向输出包装；门 1 校验投影确定性、重建稳定性与键集严格性（偏差见 `docs/04` §8）。
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentJson(pub Value);

impl std::fmt::Display for DocumentJson {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.to_string())
    }
}

/// 往 JSON 对象写一个键。
pub fn set<T: Into<Value>>(m: &mut Map<String, Value>, k: &str, v: T) {
    m.insert(k.to_string(), v.into());
}

// ---- ToJson 与基础类型的投影 --------------------------------------------------------------------

/// 模型类型 → JSON 的投影（`BIND-02`）与同表生成的 schema 片段。
///
/// 实现由 [`model_json!`] / `build/props.rs` 展开；手写仅限容器与基础类型。
pub trait ToJson {
    /// 投影为 JSON 值。
    fn to_json(&self, cx: &ProjCx<'_>) -> Value;
    /// 该类型的 JSON Schema 片段；命名类型经 [`SchemaDefs::define`] 进 `$defs` 并返回 `$ref`。
    fn schema(defs: &mut SchemaDefs) -> Value;
}

macro_rules! impl_scalar_json {
    ($($t:ty => $schema:expr),+ $(,)?) => {$(
        impl ToJson for $t {
            fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
                Value::from(*self)
            }
            fn schema(_defs: &mut SchemaDefs) -> Value {
                $schema()
            }
        }
    )+};
}

impl_scalar_json! {
    u32 => int_schema,
    i32 => int_schema,
    u64 => int_schema,
    i64 => int_schema,
    u16 => int_schema,
    u8 => int_schema,
    bool => bool_schema,
    f64 => num_schema,
}

impl ToJson for usize {
    fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
        Value::from(*self as u64)
    }
    fn schema(_defs: &mut SchemaDefs) -> Value {
        int_schema()
    }
}

impl ToJson for String {
    fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
        Value::from(self.clone())
    }
    fn schema(_defs: &mut SchemaDefs) -> Value {
        str_schema()
    }
}

impl ToJson for str {
    fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
        Value::from(self)
    }
    fn schema(_defs: &mut SchemaDefs) -> Value {
        str_schema()
    }
}

impl<T: ToJson + ?Sized> ToJson for &T {
    fn to_json(&self, cx: &ProjCx<'_>) -> Value {
        T::to_json(*self, cx)
    }
    fn schema(defs: &mut SchemaDefs) -> Value {
        T::schema(defs)
    }
}

impl<T: ToJson + ?Sized> ToJson for Box<T> {
    fn to_json(&self, cx: &ProjCx<'_>) -> Value {
        T::to_json(self, cx)
    }
    fn schema(defs: &mut SchemaDefs) -> Value {
        T::schema(defs)
    }
}

impl<T: ToJson> ToJson for Option<T> {
    /// `None` → `null`。字段级 `Option` 由 `set_some!` 跳过、不走到这里；
    /// 这里是数组元素（图表缓存 `Vec<Option<f64>>`）与嵌套 `Option` 的形态。
    fn to_json(&self, cx: &ProjCx<'_>) -> Value {
        match self {
            Some(v) => v.to_json(cx),
            None => Value::Null,
        }
    }
    fn schema(defs: &mut SchemaDefs) -> Value {
        crate::bind::native::schema::any_of_schema(vec![
            T::schema(defs),
            crate::bind::native::schema::null_schema(),
        ])
    }
}

impl<T: ToJson> ToJson for Vec<T> {
    fn to_json(&self, cx: &ProjCx<'_>) -> Value {
        Value::Array(self.iter().map(|v| v.to_json(cx)).collect())
    }
    fn schema(defs: &mut SchemaDefs) -> Value {
        crate::bind::native::schema::arr_schema(T::schema(defs))
    }
}

impl<T: ToJson> ToJson for [T] {
    fn to_json(&self, cx: &ProjCx<'_>) -> Value {
        Value::Array(self.iter().map(|v| v.to_json(cx)).collect())
    }
    fn schema(defs: &mut SchemaDefs) -> Value {
        crate::bind::native::schema::arr_schema(T::schema(defs))
    }
}

impl<T: ToJson> ToJson for BTreeMap<String, T> {
    fn to_json(&self, cx: &ProjCx<'_>) -> Value {
        Value::Object(self.iter().map(|(k, v)| (k.clone(), v.to_json(cx))).collect())
    }
    fn schema(defs: &mut SchemaDefs) -> Value {
        crate::bind::native::schema::map_schema(T::schema(defs))
    }
}

/// 区间一律 `[start, end]`（同 `BIND-07` 诊断的 `range`）。
impl ToJson for Range<u32> {
    fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
        Value::Array(vec![Value::from(self.start), Value::from(self.end)])
    }
    fn schema(_defs: &mut SchemaDefs) -> Value {
        crate::bind::native::schema::pair_schema(int_schema(), int_schema())
    }
}

impl ToJson for Range<usize> {
    fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
        Value::Array(vec![Value::from(self.start as u64), Value::from(self.end as u64)])
    }
    fn schema(_defs: &mut SchemaDefs) -> Value {
        crate::bind::native::schema::pair_schema(int_schema(), int_schema())
    }
}

/// 元组（EMU 对、主题脚本表）一律 `[a, b]`。
impl<A: ToJson, B: ToJson> ToJson for (A, B) {
    fn to_json(&self, cx: &ProjCx<'_>) -> Value {
        Value::Array(vec![self.0.to_json(cx), self.1.to_json(cx)])
    }
    fn schema(defs: &mut SchemaDefs) -> Value {
        crate::bind::native::schema::pair_schema(A::schema(defs), B::schema(defs))
    }
}

/// `PROP-09` 的值：能解析的按值投影；`Raw`（解析失败的原文）投成 `{"raw": "原文"}`。
impl<T: ToJson> ToJson for Val<T> {
    fn to_json(&self, cx: &ProjCx<'_>) -> Value {
        match self {
            Val::Value(v) => v.to_json(cx),
            Val::Raw(s) => {
                let mut o = Map::new();
                set(&mut o, "raw", s.as_str());
                Value::Object(o)
            }
        }
    }
    fn schema(defs: &mut SchemaDefs) -> Value {
        crate::bind::native::schema::any_of_schema(vec![
            T::schema(defs),
            crate::bind::native::schema::raw_schema(),
        ])
    }
}

/// sRGB：6 位 hex，无 `#`（`spec/00` §0.4）。
impl ToJson for [u8; 3] {
    fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
        let mut s = String::with_capacity(6);
        for c in self {
            write!(s, "{c:02X}").unwrap();
        }
        Value::from(s)
    }
    fn schema(_defs: &mut SchemaDefs) -> Value {
        str_schema()
    }
}

/// 已知名投 `"w:p"` 形式；`Other` / `Unbound`（interner 句柄）脱离 part 无法还原，投 `"?"`。
impl ToJson for QName {
    fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
        qname_json(self)
    }
    fn schema(_defs: &mut SchemaDefs) -> Value {
        str_schema()
    }
}

/// [`QName`] 的无 interner 投影（模块头有约定说明）。
pub fn qname_json(q: &QName) -> Value {
    match (q.ns.canonical_prefix(), q.local.known_str()) {
        (Some(""), Some(l)) | (None, Some(l)) => Value::from(l),
        (Some(p), Some(l)) => Value::from(format!("{p}:{l}")),
        _ => Value::from("?"),
    }
}

/// `ST_HexColor`：`auto` 或大写 6 位 hex（`spec/00` §0.4）。
impl ToJson for HexColorOrAuto {
    fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
        Value::from(self.to_xml())
    }
    fn schema(_defs: &mut SchemaDefs) -> Value {
        str_schema()
    }
}

/// `ST_MeasurementOrPercent`：`{"number": n}`（twips 等，单位由同元素 `w:type` 决定）或
/// `{"percent": h}`（1/100 百分点）。
impl ToJson for Measure {
    fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
        let mut o = Map::new();
        match self {
            Measure::Number(n) => set(&mut o, "number", *n),
            Measure::Percent(h) => set(&mut o, "percent", *h),
        }
        Value::Object(o)
    }
    fn schema(_defs: &mut SchemaDefs) -> Value {
        let mut np = Map::new();
        np.insert("number".to_string(), int_schema());
        let mut pp = Map::new();
        pp.insert("percent".to_string(), int_schema());
        crate::bind::native::schema::one_of_schema(vec![
            crate::bind::native::schema::obj_schema(np, vec!["number"]),
            crate::bind::native::schema::obj_schema(pp, vec!["percent"]),
        ])
    }
}

impl ToJson for serde_json::Value {
    fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
        self.clone()
    }
    fn schema(_defs: &mut SchemaDefs) -> Value {
        Value::Object(Map::new())
    }
}

// ---- id 类型：一律整数（`BIND-02` 决策 6）-------------------------------------------------------

macro_rules! impl_id_json {
    ($($t:ty),+ $(,)?) => {$(
        impl ToJson for $t {
            fn to_json(&self, _cx: &ProjCx<'_>) -> Value {
                Value::from(self.0)
            }
            fn schema(_defs: &mut SchemaDefs) -> Value {
                int_schema()
            }
        }
    )+};
}

impl_id_json! {
    crate::xml::NodeId,
    crate::package::PartId,
    crate::package::MediaId,
    crate::span::SpanId,
    crate::span::FlowId,
    crate::span::FieldId,
    crate::model::RevisionId,
    crate::edit::Utf16Offset,
}

// ---- 小工具 -------------------------------------------------------------------------------------

/// snake_case / PascalCase → lowerCamel（`BIND-02` 的键名规则；覆盖测试用它逐行校验）。
pub fn camel_case(s: &str) -> String {
    if s.contains('_') {
        let mut out = String::with_capacity(s.len());
        for (i, part) in s.split('_').enumerate() {
            if i == 0 {
                out.push_str(part);
            } else {
                let mut c = part.chars();
                if let Some(f) = c.next() {
                    out.extend(f.to_uppercase());
                    out.push_str(c.as_str());
                }
            }
        }
        out
    } else {
        let mut c = s.chars();
        match c.next() {
            Some(f) => f.to_lowercase().chain(c.as_str().chars()).collect(),
            None => String::new(),
        }
    }
}

/// `BTreeMap<PartId, T>` → 以 partId 字串为键的 JSON 对象（`BIND-02` 顶层形态的 `{ [partId]: … }`）。
pub fn id_keyed<T: ToJson>(m: &BTreeMap<PartId, T>, cx: &ProjCx<'_>) -> Value {
    Value::Object(m.iter().map(|(k, v)| (k.0.to_string(), v.to_json(cx))).collect())
}

/// `BIND-05` 的 `media[]` 条目（字节不进 JSON，经 `media(id, mediaId)` 按需取——8.4）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaEntry {
    pub media_id: MediaId,
    pub part_id: PartId,
    pub uri: String,
    pub mime: String,
    pub kind: MediaKind,
}

crate::bind::native::json::model_json! {
    /// `BIND-05` 的 `media[]` 条目。
    struct MediaEntry(cx) test json_fields_cover_media_entry {
        media_id => "mediaId", MediaId = media_id;
        part_id => "partId", PartId = part_id;
        uri => "uri", String = uri;
        mime => "mime", String = mime;
        kind => "kind", MediaKind = kind;
    }
}

crate::bind::native::json::json_str_enum! {
    /// 媒体的显示能力分类（`PKG-11`）。
    MediaKind test json_fields_cover_media_kind {
        Raster => "raster";
        Svg => "svg";
        Metafile => "metafile";
        Tiff => "tiff";
        Other => "other";
    }
}

/// 包内全部媒体 part 的条目（包枚举序 = `MediaId` 分配序；外部目标不是 part，天然不进）。
fn media_entries(pkg: &Package) -> Vec<MediaEntry> {
    let mut store = MediaStore::new();
    let mut v = Vec::new();
    for p in pkg.parts() {
        if p.deleted || pkg.content_types().image_mime(&p.uri).is_none() {
            continue;
        }
        if let Ok(id) = store.intern_part(pkg, p.id) {
            let m = store.get(id);
            v.push(MediaEntry {
                media_id: id,
                part_id: m.part,
                uri: m.uri.as_str().to_string(),
                mime: m.mime.clone(),
                kind: m.kind,
            });
        }
    }
    v
}

// ---- `Document` 顶层（`MOD-01`，`BIND-02` 顶层形态） ------------------------------------------------

crate::bind::native::json::model_json! {
    /// 文档模型（`MOD-01`）的投影：DOM + Span 的语义投影，不是第三个模型。
    struct Document(cx) test json_fields_cover_document {
        main_part => "mainPart", PartId = main_part;
        /// `w:body`；缺失时缺席（`main` 为空，`warnings` 有诊断）。
        opt body => "body", NodeId = body;
        main => "main", Vec<Block> = main;
        opt styles => "styles", Styles = styles;
        opt numbering => "numbering", Numbering = numbering;
        opt theme => "theme", Theme = theme;
        opt settings => "settings", Settings = settings;
        opt font_table => "fontTable", FontTable = font_table;
        sections => "sections", Vec<SectionInfo> = sections;
        /// 页眉页脚 part（含没被任何 `sectPr` 引用的孤儿），键是 `partId`。
        raw hf_parts => "hfParts", BTreeMap<String, HfPart> = id_keyed(hf_parts, cx);
        /// 主 part 关系里的图表 part（含没被引用的）；`display` 载荷只在 `display: true`。
        raw chart_parts => "chartParts", BTreeMap<String, ChartPart> = id_keyed(chart_parts, cx);
        /// 主 part 引用的 SmartArt（按**数据 part**）；`shapes` 只在 `display: true`。
        raw diagram_parts => "diagramParts", BTreeMap<String, DiagramPart> = id_keyed(diagram_parts, cx);
        /// 主 part 里的墨迹批注（`aidocs-ink`），文档序。
        inks => "inks", Vec<InkInfo> = inks;
        /// 主 part 的字段索引（`FLD-02`）。
        raw fields => "fields", Vec<FieldSpan> = fields.fields().to_json(cx);
        /// 主 part 的范围索引（`SPAN-04`）。
        raw spans => "spans", Vec<RangeSpan> = spans.spans().to_json(cx);
        sources => "sources", Vec<Source> = sources;
        opt sources_part => "sourcesPart", PartId = sources_part;
        /// 全包的修订表（`MOD-09`），文档序。
        raw revisions => "revisions", Vec<RevisionEntry> = revisions.entries().to_json(cx);
        raw comments => "comments", Vec<Comment> = comments.items.to_json(cx);
        raw footnotes => "footnotes", Vec<Note> = footnotes.items.to_json(cx);
        raw endnotes => "endnotes", Vec<Note> = endnotes.items.to_json(cx);
        warnings => "warnings", Vec<Diagnostic> = warnings;
        /// `BIND-05`：媒体只给条目，字节经 `media(id, mediaId)` 按需取（8.4）。
        extra "media", Vec<MediaEntry> = media_entries(cx.pkg);
        skip hf_by_rel, "关系反查表，可从 hfParts 键派生（BIND-02 不投影派生值）";
        skip aux_flows, "外部文本框 part 索引；块内容挂在 ShapeDisplay.content（display:true 才投影）";
        skip chart_by_rel, "关系反查表，可从 chartParts 键派生";
        skip diagram_by_rel, "关系反查表，可从 diagramParts 键派生";
        skip flows, "主 part 的 FlowMap（SPAN-01 内部索引），投影无意义";
    }
}

/// `Document` 的模型 JSON 投影（`BIND-02`）。
pub fn document_json(pkg: &Package, doc: &Document, opts: DocumentOpts) -> DocumentJson {
    let cx = ProjCx { pkg, display: opts.display };
    DocumentJson(doc.to_json(&cx))
}

// ---- 宏 -----------------------------------------------------------------------------------------

/// 有值才写。
///
/// ```ignore
/// set_some!(o,
///     "widthPx" => ext.map(|e| px(e.cx)),
///     "rotDeg" => s.rot_60k.filter(|&r| r != 0),
/// );
/// ```
macro_rules! set_some {
    ($o:expr, $($key:literal => $val:expr),+ $(,)?) => {$(
        if let ::core::option::Option::Some(v) = $val {
            $crate::bind::native::json::set($o, $key, v);
        }
    )+};
}

/// 条件为真才写，写进去的值恒为 `true`——投影里的布尔字段一律「要么是 true，要么不出现」。
///
/// ```ignore
/// set_if!(o, "floating" => grouped, "behind" => a.behind_doc);
/// ```
macro_rules! set_if {
    ($o:expr, $($key:literal => $cond:expr),+ $(,)?) => {$(
        if $cond {
            $crate::bind::native::json::set($o, $key, true);
        }
    )+};
}

/// 「模型结构体 → TS JSON 对象」的字段表（`spec/17` 的 `display_json!`）：每行一个键，展开成一个
/// `Map<String, Value>`。三种行：
///
/// - `"key" => expr` 恒写；
/// - `opt "key" => expr` 的 `expr` 是 `Option`，有值才写（`set_some!`）；
/// - `flag "key" => expr` 的 `expr` 是 `bool`，为真才写 `true`（[`set_if!`]）。
///
/// `compat_ts` 的 TS 形态投影专用（8.2 随 `set_some!` / `set_if!` 一起搬到本模块）。
macro_rules! display_json {
    (@field $o:ident, opt $key:literal, $val:expr) => {
        $crate::bind::native::json::set_some!(&mut $o, $key => $val)
    };
    (@field $o:ident, flag $key:literal, $val:expr) => {
        $crate::bind::native::json::set_if!(&mut $o, $key => $val)
    };
    (@field $o:ident, $key:literal, $val:expr) => {
        $crate::bind::native::json::set(&mut $o, $key, $val)
    };
    ($($($tag:ident)? $key:literal => $val:expr),+ $(,)?) => {{
        let mut o = ::serde_json::Map::new();
        $( $crate::bind::native::json::display_json!(@field o, $($tag)? $key, $val); )+
        o
    }};
}

/// 「Rust 模型类型 → 原生 JSON」的字段表（`BIND-02`，`spec/19`「实现约定」的 `model_json!`）。
/// 同一张表展开三样：
///
/// 1. `impl ToJson`——`to_json` 开头对结构体做**完整解构**（Rust 结构体加字段 → 编译失败，
///    「不丢字段」不靠人记）；非 `raw` 行有行表达式类型与声明类型的编译期比对；
/// 2. `ToJson::schema`——该类型的 JSON Schema 片段（命名类型进 `$defs`）；
/// 3. `#[test]`——逐行校验 JSON 键 == camelCase(Rust 字段名)（`~` 改名行除外）、schema 键集
///    与表一致、必填集与恒写行一致、`skip` 字段确实缺席。
///
/// hygiene 约定（宏生成代码与调用处 `self` / `cx` 语法上下文不同）：
///
/// - 表头 `(cx)` 声明投影上下文参数名，行表达式用它引用 [`ProjCx`]；
/// - 行表达式用**解构出的字段绑定**（与字段同名的引用），不写 `self.field`；
/// - 枚举结构变体的行表达式用变体绑定（同样是引用）。
///
/// 行形态（`;` 结束；行首可写文档注释）：
///
/// ```ignore
/// model_json! {
///     /// `MOD-06` 的物理 `w:r`。
///     struct Run(cx) test json_fields_cover_run {
///         node      => "nodeId", NodeId = node;                  // 恒写
///         opt link  => "link", Link = link;                      // Option，有值才写
///         flag vanish => "vanish" = vanish;                      // bool，为真才写 true
///         raw spans => "spans", Vec<RangeSpan> = spans.spans().to_json(cx);
///         raw opt display => "display", Display = display.as_ref().filter(|_| cx.display).map(|d| d.to_json(cx));
///         ~ kind => "breakKind", BreakKind = kind, "与变体标签键 kind 撞名";
///         extra "media", Vec<MediaEntry> = media_entries(cx.pkg);
///         skip raw_unmodeled, "未建模子元素属原字节引用（BIND-02 禁止项）";
///     }
/// }
/// ```
///
/// `struct Name(cx) @partial test …`：类型有私有字段（`SectionInfo.end_offset` 等）时解构带
/// `..`，被跳过字段的理由写在表内注释里。
///
/// 枚举形态：变体四种——`V => "v";`（无载荷）、`V(T) => "v";`（载荷对象平铺）、
/// `V(T) as "key" => "v";`（载荷放在 `key` 下）、
/// `V { a, b } => "v" { a => "a", TA = a; … };`（结构变体，行内表达式直接用绑定名）。
macro_rules! model_json {
    // ================================ 入口：结构体 ===============================================
    ($(#[$meta:meta])* struct $name:ident ($cx:ident) $(@$partial:ident)? test $tfn:ident { $($rows:tt)* }) => {
        $(#[$meta])*
        impl $crate::bind::native::json::ToJson for $name {
            #[allow(unused_variables)]
            fn to_json(&self, $cx: &$crate::bind::native::json::ProjCx<'_>) -> ::serde_json::Value {
                $crate::bind::native::json::model_json!(@destructure $name, self, {}, [$($partial)?], $($rows)*);
                let mut o = ::serde_json::Map::new();
                $crate::bind::native::json::model_json!(@w o $cx $($rows)*);
                ::serde_json::Value::Object(o)
            }
            fn schema(defs: &mut $crate::bind::native::json::SchemaDefs) -> ::serde_json::Value {
                defs.define(stringify!($name), |defs| {
                    let mut p = ::serde_json::Map::new();
                    #[allow(unused_mut)]
            let mut r: ::std::vec::Vec<&'static str> = ::std::vec::Vec::new();
                    $crate::bind::native::json::model_json!(@s p r defs $($rows)*);
                    $crate::bind::native::schema::obj_schema(p, r)
                })
            }
        }
        $crate::bind::native::json::model_json!(@cover $name, $tfn, $($rows)*);
    };

    // ================================ 入口：枚举 =================================================
    ($(#[$meta:meta])* enum $name:ident ($cx:ident) test $tfn:ident { $($variants:tt)* }) => {
        $(#[$meta])*
        impl $crate::bind::native::json::ToJson for $name {
            #[allow(unused_variables)]
            fn to_json(&self, $cx: &$crate::bind::native::json::ProjCx<'_>) -> ::serde_json::Value {
                $crate::bind::native::json::model_json!(@mkmatch $cx, self, {}, $($variants)*)
            }
            fn schema(defs: &mut $crate::bind::native::json::SchemaDefs) -> ::serde_json::Value {
                defs.define(stringify!($name), |defs| {
                    $crate::bind::native::json::model_json!(@eschema defs, {}, $($variants)*)
                })
            }
        }
        $crate::bind::native::json::model_json!(@ecover $name, $tfn, $($variants)*);
    };

    // ---- @destructure：完整解构（累进器拼装整个 `let`；宏在模式内部不能嵌套调用） -----------------
    (@destructure $name:ident, $slf:ident, {$($acc:tt)*}, [],) => {
        #[allow(unused_variables)]
        let $name { $($acc)* } = $slf;
    };
    (@destructure $name:ident, $slf:ident, {$($acc:tt)*}, [partial],) => {
        #[allow(unused_variables)]
        let $name { $($acc)* .. } = $slf;
    };
    (@destructure $name:ident, $slf:ident, {$($acc:tt)*}, [$($partial:ident)?], $(#[$rm:meta])* raw opt $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@destructure $name, $slf, {$($acc)* $f,}, [$($partial)?], $($rest)*)
    };
    (@destructure $name:ident, $slf:ident, {$($acc:tt)*}, [$($partial:ident)?], $(#[$rm:meta])* raw $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@destructure $name, $slf, {$($acc)* $f,}, [$($partial)?], $($rest)*)
    };
    (@destructure $name:ident, $slf:ident, {$($acc:tt)*}, [$($partial:ident)?], $(#[$rm:meta])* ~ opt $f:ident => $k:literal, $ty:ty = $e:expr, $reason:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@destructure $name, $slf, {$($acc)* $f,}, [$($partial)?], $($rest)*)
    };
    (@destructure $name:ident, $slf:ident, {$($acc:tt)*}, [$($partial:ident)?], $(#[$rm:meta])* ~ $f:ident => $k:literal, $ty:ty = $e:expr, $reason:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@destructure $name, $slf, {$($acc)* $f,}, [$($partial)?], $($rest)*)
    };
    (@destructure $name:ident, $slf:ident, {$($acc:tt)*}, [$($partial:ident)?], $(#[$rm:meta])* opt $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@destructure $name, $slf, {$($acc)* $f,}, [$($partial)?], $($rest)*)
    };
    (@destructure $name:ident, $slf:ident, {$($acc:tt)*}, [$($partial:ident)?], $(#[$rm:meta])* flag $f:ident => $k:literal = $e:expr ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@destructure $name, $slf, {$($acc)* $f,}, [$($partial)?], $($rest)*)
    };
    (@destructure $name:ident, $slf:ident, {$($acc:tt)*}, [$($partial:ident)?], $(#[$rm:meta])* extra $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@destructure $name, $slf, {$($acc)*}, [$($partial)?], $($rest)*)
    };
    (@destructure $name:ident, $slf:ident, {$($acc:tt)*}, [$($partial:ident)?], $(#[$rm:meta])* skip $f:ident, $reason:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@destructure $name, $slf, {$($acc)* $f,}, [$($partial)?], $($rest)*)
    };
    (@destructure $name:ident, $slf:ident, {$($acc:tt)*}, [$($partial:ident)?], $(#[$rm:meta])* $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@destructure $name, $slf, {$($acc)* $f,}, [$($partial)?], $($rest)*)
    };

    // ---- @mkmatch：枚举 to_json（累进器拼装整个 `match`；宏不能展开为 match 臂列表） ----------------
    (@mkmatch $cx:ident, $slf:ident, {$($arms:tt)*},) => {
        match $slf { $($arms)* }
    };
    (@mkmatch $cx:ident, $slf:ident, {$($arms:tt)*}, $(#[$vm:meta])* $v:ident { $($b:ident),+ } => $kind:literal { $($rows:tt)* } ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@mkmatch $cx, $slf, {$($arms)*
            Self::$v { $($b),+ } => {
                let mut o = ::serde_json::Map::new();
                $crate::bind::native::json::set(&mut o, "kind", $kind);
                $crate::bind::native::json::model_json!(@w o $cx $($rows)*);
                ::serde_json::Value::Object(o)
            },
        }, $($rest)*)
    };
    (@mkmatch $cx:ident, $slf:ident, {$($arms:tt)*}, $(#[$vm:meta])* $v:ident { $($b:ident),+ } => $kind:literal { $($rows:tt)* } $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@mkmatch $cx, $slf, {$($arms)*
            Self::$v { $($b),+ } => {
                let mut o = ::serde_json::Map::new();
                $crate::bind::native::json::set(&mut o, "kind", $kind);
                $crate::bind::native::json::model_json!(@w o $cx $($rows)*);
                ::serde_json::Value::Object(o)
            },
        }, $($rest)*)
    };
    (@mkmatch $cx:ident, $slf:ident, {$($arms:tt)*}, $(#[$vm:meta])* $v:ident($p:ty) as $pk:literal => $kind:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@mkmatch $cx, $slf, {$($arms)*
            Self::$v(v) => {
                let mut o = ::serde_json::Map::new();
                $crate::bind::native::json::set(&mut o, "kind", $kind);
                $crate::bind::native::json::set(&mut o, $pk, $crate::bind::native::json::ToJson::to_json(v, $cx));
                ::serde_json::Value::Object(o)
            },
        }, $($rest)*)
    };
    (@mkmatch $cx:ident, $slf:ident, {$($arms:tt)*}, $(#[$vm:meta])* $v:ident($p:ty) => $kind:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@mkmatch $cx, $slf, {$($arms)*
            Self::$v(v) => $crate::bind::native::schema::flatten_variant_json(
                $crate::bind::native::json::ToJson::to_json(v, $cx), $kind),
        }, $($rest)*)
    };
    (@mkmatch $cx:ident, $slf:ident, {$($arms:tt)*}, $(#[$vm:meta])* $v:ident => $kind:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@mkmatch $cx, $slf, {$($arms)*
            Self::$v => {
                let mut o = ::serde_json::Map::new();
                $crate::bind::native::json::set(&mut o, "kind", $kind);
                ::serde_json::Value::Object(o)
            },
        }, $($rest)*)
    };

    // ---- @eschema：枚举 schema（累进器拼装 `oneOf` 列表；表达式列表同理不能靠嵌套宏） ---------------
    (@eschema $defs:ident, {$($acc:expr),*},) => {
        $crate::bind::native::schema::one_of_schema(::std::vec![$($acc),*])
    };
    (@eschema $defs:ident, {$($acc:expr),*}, $(#[$vm:meta])* $v:ident { $($b:ident),+ } => $kind:literal { $($rows:tt)* } ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@eschema $defs, {$($acc,)* {
            let mut p = ::serde_json::Map::new();
            #[allow(unused_mut)]
            let mut r: ::std::vec::Vec<&'static str> = ::std::vec::Vec::new();
            $crate::bind::native::json::model_json!(@s p r $defs $($rows)*);
            $crate::bind::native::schema::variant_schema($kind, p, r)
        }}, $($rest)*)
    };
    (@eschema $defs:ident, {$($acc:expr),*}, $(#[$vm:meta])* $v:ident { $($b:ident),+ } => $kind:literal { $($rows:tt)* } $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@eschema $defs, {$($acc,)* {
            let mut p = ::serde_json::Map::new();
            #[allow(unused_mut)]
            let mut r: ::std::vec::Vec<&'static str> = ::std::vec::Vec::new();
            $crate::bind::native::json::model_json!(@s p r $defs $($rows)*);
            $crate::bind::native::schema::variant_schema($kind, p, r)
        }}, $($rest)*)
    };
    (@eschema $defs:ident, {$($acc:expr),*}, $(#[$vm:meta])* $v:ident($p:ty) as $pk:literal => $kind:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@eschema $defs, {$($acc,)* {
            let mut p = ::serde_json::Map::new();
            p.insert($pk.into(), <$p as $crate::bind::native::json::ToJson>::schema($defs));
            $crate::bind::native::schema::variant_schema($kind, p, ::std::vec![$pk])
        }}, $($rest)*)
    };
    (@eschema $defs:ident, {$($acc:expr),*}, $(#[$vm:meta])* $v:ident($p:ty) => $kind:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@eschema $defs, {$($acc,)*
            $crate::bind::native::schema::flatten_variant_schema($kind, <$p as $crate::bind::native::json::ToJson>::schema($defs))
        }, $($rest)*)
    };
    (@eschema $defs:ident, {$($acc:expr),*}, $(#[$vm:meta])* $v:ident => $kind:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@eschema $defs, {$($acc,)*
            $crate::bind::native::schema::variant_schema($kind, ::serde_json::Map::new(), ::std::vec::Vec::new())
        }, $($rest)*)
    };

    // ---- @w：字段行的写出（表达式是绑定，本身已是引用；`extra` 行的表达式是值） ----------------------
    (@w $o:ident $cx:ident) => {};
    (@w $o:ident $cx:ident $(#[$rm:meta])* raw opt $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $crate::bind::native::json::set_some!(&mut $o, $k => $e);
        $crate::bind::native::json::model_json!(@w $o $cx $($rest)*);
    };
    (@w $o:ident $cx:ident $(#[$rm:meta])* raw $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $crate::bind::native::json::set(&mut $o, $k, $e);
        $crate::bind::native::json::model_json!(@w $o $cx $($rest)*);
    };
    (@w $o:ident $cx:ident $(#[$rm:meta])* ~ opt $f:ident => $k:literal, $ty:ty = $e:expr, $reason:literal ; $($rest:tt)*) => {
        let _: &Option<$ty> = $e;
        $crate::bind::native::json::set_some!(&mut $o, $k => ($e).as_ref().map(|v| $crate::bind::native::json::ToJson::to_json(v, $cx)));
        $crate::bind::native::json::model_json!(@w $o $cx $($rest)*);
    };
    (@w $o:ident $cx:ident $(#[$rm:meta])* ~ $f:ident => $k:literal, $ty:ty = $e:expr, $reason:literal ; $($rest:tt)*) => {
        let _: &$ty = $e;
        $crate::bind::native::json::set(&mut $o, $k, $crate::bind::native::json::ToJson::to_json($e, $cx));
        $crate::bind::native::json::model_json!(@w $o $cx $($rest)*);
    };
    (@w $o:ident $cx:ident $(#[$rm:meta])* opt $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        let _: &Option<$ty> = $e;
        $crate::bind::native::json::set_some!(&mut $o, $k => ($e).as_ref().map(|v| $crate::bind::native::json::ToJson::to_json(v, $cx)));
        $crate::bind::native::json::model_json!(@w $o $cx $($rest)*);
    };
    (@w $o:ident $cx:ident $(#[$rm:meta])* flag $f:ident => $k:literal = $e:expr ; $($rest:tt)*) => {
        let _: &bool = $e;
        $crate::bind::native::json::set_if!(&mut $o, $k => *$e);
        $crate::bind::native::json::model_json!(@w $o $cx $($rest)*);
    };
    (@w $o:ident $cx:ident $(#[$rm:meta])* extra $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        let _: &$ty = &$e;
        $crate::bind::native::json::set(&mut $o, $k, $crate::bind::native::json::ToJson::to_json(&$e, $cx));
        $crate::bind::native::json::model_json!(@w $o $cx $($rest)*);
    };
    (@w $o:ident $cx:ident $(#[$rm:meta])* skip $f:ident, $reason:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@w $o $cx $($rest)*);
    };
    (@w $o:ident $cx:ident $(#[$rm:meta])* $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        let _: &$ty = $e;
        $crate::bind::native::json::set(&mut $o, $k, $crate::bind::native::json::ToJson::to_json($e, $cx));
        $crate::bind::native::json::model_json!(@w $o $cx $($rest)*);
    };

    // ---- @s：schema 片段（`opt` 不进 `required`；`flag` 恒 boolean） ------------------------------
    (@s $p:ident $r:ident $defs:ident) => {};
    (@s $p:ident $r:ident $defs:ident $(#[$rm:meta])* raw opt $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $p.insert($k.into(), <$ty as $crate::bind::native::json::ToJson>::schema($defs));
        $crate::bind::native::json::model_json!(@s $p $r $defs $($rest)*);
    };
    (@s $p:ident $r:ident $defs:ident $(#[$rm:meta])* raw $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $p.insert($k.into(), <$ty as $crate::bind::native::json::ToJson>::schema($defs));
        $r.push($k);
        $crate::bind::native::json::model_json!(@s $p $r $defs $($rest)*);
    };
    (@s $p:ident $r:ident $defs:ident $(#[$rm:meta])* ~ opt $f:ident => $k:literal, $ty:ty = $e:expr, $reason:literal ; $($rest:tt)*) => {
        $p.insert($k.into(), <$ty as $crate::bind::native::json::ToJson>::schema($defs));
        $crate::bind::native::json::model_json!(@s $p $r $defs $($rest)*);
    };
    (@s $p:ident $r:ident $defs:ident $(#[$rm:meta])* ~ $f:ident => $k:literal, $ty:ty = $e:expr, $reason:literal ; $($rest:tt)*) => {
        $p.insert($k.into(), <$ty as $crate::bind::native::json::ToJson>::schema($defs));
        $r.push($k);
        $crate::bind::native::json::model_json!(@s $p $r $defs $($rest)*);
    };
    (@s $p:ident $r:ident $defs:ident $(#[$rm:meta])* opt $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $p.insert($k.into(), <$ty as $crate::bind::native::json::ToJson>::schema($defs));
        $crate::bind::native::json::model_json!(@s $p $r $defs $($rest)*);
    };
    (@s $p:ident $r:ident $defs:ident $(#[$rm:meta])* flag $f:ident => $k:literal = $e:expr ; $($rest:tt)*) => {
        $p.insert($k.into(), <bool as $crate::bind::native::json::ToJson>::schema($defs));
        $crate::bind::native::json::model_json!(@s $p $r $defs $($rest)*);
    };
    (@s $p:ident $r:ident $defs:ident $(#[$rm:meta])* extra $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $p.insert($k.into(), <$ty as $crate::bind::native::json::ToJson>::schema($defs));
        $r.push($k);
        $crate::bind::native::json::model_json!(@s $p $r $defs $($rest)*);
    };
    (@s $p:ident $r:ident $defs:ident $(#[$rm:meta])* skip $f:ident, $reason:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@s $p $r $defs $($rest)*);
    };
    (@s $p:ident $r:ident $defs:ident $(#[$rm:meta])* $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $p.insert($k.into(), <$ty as $crate::bind::native::json::ToJson>::schema($defs));
        $r.push($k);
        $crate::bind::native::json::model_json!(@s $p $r $defs $($rest)*);
    };

    // ---- @tk：覆盖测试的键集收集 ------------------------------------------------------------------
    (@tk $keys:ident $req:ident $skips:ident) => {};
    (@tk $keys:ident $req:ident $skips:ident $(#[$rm:meta])* raw opt $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $keys.push($k);
        $crate::bind::native::json::model_json!(@tk $keys $req $skips $($rest)*);
    };
    (@tk $keys:ident $req:ident $skips:ident $(#[$rm:meta])* raw $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $keys.push($k); $req.push($k);
        $crate::bind::native::json::model_json!(@tk $keys $req $skips $($rest)*);
    };
    (@tk $keys:ident $req:ident $skips:ident $(#[$rm:meta])* ~ opt $f:ident => $k:literal, $ty:ty = $e:expr, $reason:literal ; $($rest:tt)*) => {
        $keys.push($k);
        $crate::bind::native::json::model_json!(@tk $keys $req $skips $($rest)*);
    };
    (@tk $keys:ident $req:ident $skips:ident $(#[$rm:meta])* ~ $f:ident => $k:literal, $ty:ty = $e:expr, $reason:literal ; $($rest:tt)*) => {
        $keys.push($k); $req.push($k);
        $crate::bind::native::json::model_json!(@tk $keys $req $skips $($rest)*);
    };
    (@tk $keys:ident $req:ident $skips:ident $(#[$rm:meta])* opt $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $keys.push($k);
        $crate::bind::native::json::model_json!(@tk $keys $req $skips $($rest)*);
    };
    (@tk $keys:ident $req:ident $skips:ident $(#[$rm:meta])* flag $f:ident => $k:literal = $e:expr ; $($rest:tt)*) => {
        $keys.push($k);
        $crate::bind::native::json::model_json!(@tk $keys $req $skips $($rest)*);
    };
    (@tk $keys:ident $req:ident $skips:ident $(#[$rm:meta])* extra $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $keys.push($k); $req.push($k);
        $crate::bind::native::json::model_json!(@tk $keys $req $skips $($rest)*);
    };
    (@tk $keys:ident $req:ident $skips:ident $(#[$rm:meta])* skip $f:ident, $reason:literal ; $($rest:tt)*) => {
        $skips.push($crate::bind::native::json::camel_case(stringify!($f)));
        $crate::bind::native::json::model_json!(@tk $keys $req $skips $($rest)*);
    };
    (@tk $keys:ident $req:ident $skips:ident $(#[$rm:meta])* $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $keys.push($k); $req.push($k);
        $crate::bind::native::json::model_json!(@tk $keys $req $skips $($rest)*);
    };

    // ---- @tc：camelCase 校验（`~` 改名行与 `extra` / `skip` 行不查） ------------------------------
    (@tc) => {};
    (@tc $(#[$rm:meta])* raw opt $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@camel $f, $k);
        $crate::bind::native::json::model_json!(@tc $($rest)*);
    };
    (@tc $(#[$rm:meta])* raw $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@camel $f, $k);
        $crate::bind::native::json::model_json!(@tc $($rest)*);
    };
    (@tc $(#[$rm:meta])* ~ opt $f:ident => $k:literal, $ty:ty = $e:expr, $reason:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@tc $($rest)*);
    };
    (@tc $(#[$rm:meta])* ~ $f:ident => $k:literal, $ty:ty = $e:expr, $reason:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@tc $($rest)*);
    };
    (@tc $(#[$rm:meta])* opt $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@camel $f, $k);
        $crate::bind::native::json::model_json!(@tc $($rest)*);
    };
    (@tc $(#[$rm:meta])* flag $f:ident => $k:literal = $e:expr ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@camel $f, $k);
        $crate::bind::native::json::model_json!(@tc $($rest)*);
    };
    (@tc $(#[$rm:meta])* extra $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@tc $($rest)*);
    };
    (@tc $(#[$rm:meta])* skip $f:ident, $reason:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@tc $($rest)*);
    };
    (@tc $(#[$rm:meta])* $f:ident => $k:literal, $ty:ty = $e:expr ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@camel $f, $k);
        $crate::bind::native::json::model_json!(@tc $($rest)*);
    };
    (@camel $f:ident, $k:literal) => {
        assert_eq!(
            $crate::bind::native::json::camel_case(stringify!($f)),
            $k,
            "BIND-02：JSON 键须是 Rust 字段名的 camelCase（{} -> {:?}；确需改名用 `~` 行并写理由）",
            stringify!($f),
            $k,
        );
    };

    // ---- @cover：结构体的覆盖测试 -----------------------------------------------------------------
    (@cover $name:ident, $tfn:ident, $($rows:tt)*) => {
        #[cfg(test)]
        #[test]
        fn $tfn() {
            let mut keys: ::std::vec::Vec<&'static str> = ::std::vec::Vec::new();
            let mut required: ::std::vec::Vec<&'static str> = ::std::vec::Vec::new();
            #[allow(unused_mut)]
            let mut skips: ::std::vec::Vec<::std::string::String> = ::std::vec::Vec::new();
            $crate::bind::native::json::model_json!(@tk keys required skips $($rows)*);
            $crate::bind::native::json::model_json!(@tc $($rows)*);
            let mut defs = $crate::bind::native::json::SchemaDefs::default();
            let _ = <$name as $crate::bind::native::json::ToJson>::schema(&mut defs);
            let def = defs.get(stringify!($name)).unwrap_or_else(|| ::core::panic!("{}: schema 未注册", stringify!($name)));
            let props = def.get("properties").and_then(|p| p.as_object()).expect("object schema");
            let mut got: ::std::vec::Vec<&str> = props.keys().map(::std::string::String::as_str).collect();
            got.sort_unstable();
            keys.sort_unstable();
            assert_eq!(got, keys, "{}: schema 键集与 model_json! 表不一致", stringify!($name));
            let mut req: ::std::vec::Vec<&str> = def
                .get("required")
                .and_then(|r| r.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            req.sort_unstable();
            required.sort_unstable();
            assert_eq!(req, required, "{}: schema 必填集与表的恒写行不一致", stringify!($name));
            for s in &skips {
                assert!(!props.contains_key(s.as_str()), "{}: skip 字段 `{}` 进了 schema", stringify!($name), s);
            }
        }
    };

    // ---- @et：枚举覆盖测试（kind 字串 camel 校验 + 结构变体行 camel 校验） --------------------------
    (@et $kinds:ident) => {};
    (@et $kinds:ident $(#[$vm:meta])* $v:ident { $($b:ident),+ } => $kind:literal { $($rows:tt)* } ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@camel $v, $kind);
        $kinds.push($kind);
        $crate::bind::native::json::model_json!(@tc $($rows)*);
        $crate::bind::native::json::model_json!(@et $kinds $($rest)*);
    };
    (@et $kinds:ident $(#[$vm:meta])* $v:ident { $($b:ident),+ } => $kind:literal { $($rows:tt)* } $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@camel $v, $kind);
        $kinds.push($kind);
        $crate::bind::native::json::model_json!(@tc $($rows)*);
        $crate::bind::native::json::model_json!(@et $kinds $($rest)*);
    };
    (@et $kinds:ident $(#[$vm:meta])* $v:ident($p:ty) as $pk:literal => $kind:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@camel $v, $kind);
        $kinds.push($kind);
        $crate::bind::native::json::model_json!(@et $kinds $($rest)*);
    };
    (@et $kinds:ident $(#[$vm:meta])* $v:ident($p:ty) => $kind:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@camel $v, $kind);
        $kinds.push($kind);
        $crate::bind::native::json::model_json!(@et $kinds $($rest)*);
    };
    (@et $kinds:ident $(#[$vm:meta])* $v:ident => $kind:literal ; $($rest:tt)*) => {
        $crate::bind::native::json::model_json!(@camel $v, $kind);
        $kinds.push($kind);
        $crate::bind::native::json::model_json!(@et $kinds $($rest)*);
    };

    // ---- @ecover：枚举的覆盖测试 --------------------------------------------------------------------
    (@ecover $name:ident, $tfn:ident, $($variants:tt)*) => {
        #[cfg(test)]
        #[test]
        fn $tfn() {
            let mut kinds: ::std::vec::Vec<&'static str> = ::std::vec::Vec::new();
            $crate::bind::native::json::model_json!(@et kinds $($variants)*);
            let n = kinds.len();
            kinds.sort_unstable();
            kinds.dedup();
            assert_eq!(kinds.len(), n, "{}: 变体 kind 字串重复", stringify!($name));
            let mut defs = $crate::bind::native::json::SchemaDefs::default();
            let _ = <$name as $crate::bind::native::json::ToJson>::schema(&mut defs);
            let def = defs.get(stringify!($name)).unwrap_or_else(|| ::core::panic!("{}: schema 未注册", stringify!($name)));
            let arms = def.get("oneOf").and_then(|v| v.as_array()).expect("oneOf schema");
            assert_eq!(arms.len(), n, "{}: oneOf 分支数与变体数不一致", stringify!($name));
        }
    };
}

/// 无载荷枚举 → 字串（无 `as_str` 的枚举用的名字表；有 `as_str` 的用 [`as_str_json!`]）。
macro_rules! json_str_enum {
    ($(#[$meta:meta])* $name:ident test $tfn:ident { $( $(#[$vm:meta])* $v:ident => $s:literal ; )+ }) => {
        $(#[$meta])*
        impl $crate::bind::native::json::ToJson for $name {
            fn to_json(&self, _cx: &$crate::bind::native::json::ProjCx<'_>) -> ::serde_json::Value {
                ::serde_json::Value::from(match self { $( Self::$v => $s, )+ })
            }
            fn schema(_defs: &mut $crate::bind::native::json::SchemaDefs) -> ::serde_json::Value {
                $crate::bind::native::schema::enum_str_schema(&[$( $s ),+])
            }
        }
        #[cfg(test)]
        #[test]
        fn $tfn() {
            $( $crate::bind::native::json::model_json!(@camel $v, $s); )+
        }
    };
}

/// `named_enum!` / `sdt_enum!` 生成的枚举：`as_str` 字串即 JSON 值（`BIND-02`）。
macro_rules! as_str_json {
    ($($name:ty),+ $(,)?) => {$(
        impl $crate::bind::native::json::ToJson for $name {
            fn to_json(&self, _cx: &$crate::bind::native::json::ProjCx<'_>) -> ::serde_json::Value {
                ::serde_json::Value::from(self.as_str())
            }
            fn schema(_defs: &mut $crate::bind::native::json::SchemaDefs) -> ::serde_json::Value {
                $crate::bind::native::schema::str_schema()
            }
        }
    )+};
}

pub(crate) use {as_str_json, display_json, json_str_enum, model_json, set_if, set_some};
