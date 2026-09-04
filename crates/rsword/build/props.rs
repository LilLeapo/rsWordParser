//! 属性表生成器（`PROP-01`、`PROP-07`，任务 1.1）。
//!
//! 输入：`schema/props/types.toml`（枚举与属性结构体）与 `schema/props/*.toml`（每个文件若干张
//! `[[table]]`）。输出 `$OUT_DIR/props.rs`，由 `src/semantic/props/mod.rs` `include!`。
//! 文件格式见 `schema/props/README.md`；生成的 API 形状见 `spec/05-properties.md` `PROP-07`。
//!
//! 生成器只做名字解析与结构校验，不理解 codec 语义：codec 的解析 / 写回在
//! `src/semantic/props/codec.rs` 手写，生成代码只按名字调用。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use serde::Deserialize;

// ---- TOML 结构 ---------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypesFile {
    #[serde(default, rename = "enum")]
    enums: BTreeMap<String, EnumDecl>,
    #[serde(default, rename = "struct")]
    structs: BTreeMap<String, StructDecl>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnumDecl {
    #[serde(default)]
    doc: String,
    values: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StructDecl {
    #[serde(default)]
    doc: String,
    attrs: Vec<AttrDecl>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttrDecl {
    name: String,
    attr: String,
    codec: String,
    /// Transitional 写法（`w:left` 对 `w:start`）；解析两者都接受，生成按 flavor。
    #[serde(default)]
    legacy: Option<String>,
    #[serde(default)]
    doc: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TableFile {
    table: Vec<TableDecl>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TableDecl {
    name: String,
    element: String,
    #[serde(default)]
    change: Option<String>,
    #[serde(default)]
    doc: String,
    /// 容器全部子元素的 schema 顺序（`PROP-05`），含未建模的；`a|b` 表示同义对占同一序号。
    order: Vec<String>,
    /// 容器元素自身的属性（`w:lvl/@ilvl`、`w:style/@styleId`），列与 struct 的 attrs 相同。
    #[serde(default)]
    attrs: Vec<AttrDecl>,
    #[serde(default)]
    field: Vec<FieldDecl>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldDecl {
    name: String,
    element: String,
    codec: String,
    #[serde(default)]
    legacy: Option<String>,
    #[serde(default)]
    multi: bool,
    #[serde(default = "yes")]
    in_change: bool,
    #[serde(default)]
    cs_twin: Option<String>,
    #[serde(default)]
    doc: String,
}

fn yes() -> bool {
    true
}

// ---- 解析后的模型 ------------------------------------------------------------------------------

/// 内建标量 codec：(名字, Rust 值类型)。codec 类型本身在 `codec` 模块同名。
const BUILTIN: &[(&str, &str)] = &[
    ("OnOff", "bool"),
    ("HalfPoints", "Val<u32>"),
    ("SignedHalfPoints", "Val<i32>"),
    ("Twips", "Val<i32>"),
    ("EighthPoints", "Val<u32>"),
    ("HexColorOrAuto", "Val<HexColorOrAuto>"),
    ("Hex2", "Val<u8>"),
    ("Percent", "Val<u32>"),
    ("Str", "String"),
    ("Int", "Val<i32>"),
    ("UInt", "Val<u32>"),
];

#[derive(Clone)]
struct Qn {
    ns: String,
    local: String,
    text: String,
}

impl Qn {
    fn expr(&self) -> String {
        format!("QName::new(NsId::{}, LocalName::{})", self.ns, self.local)
    }

    fn pat(&self) -> String {
        format!("QName {{ ns: NsId::{}, local: LocalName::{} }}", self.ns, self.local)
    }
}

fn opt_qn_expr(q: Option<&Qn>) -> String {
    q.map_or_else(|| "None".to_string(), |q| format!("Some({})", q.expr()))
}

#[derive(Clone)]
enum Kind {
    /// 单 `w:val` 标量；`codec` 是实现 `Codec` 的类型路径，`value` 是其 `Value`。
    Scalar {
        codec: String,
        value: String,
    },
    Struct(String),
    Table(String),
    Raw,
}

struct Field {
    name: String,
    variant: String,
    element: Qn,
    legacy: Option<Qn>,
    kind: Kind,
    multi: bool,
    in_change: bool,
    cs_twin: Option<String>,
    order: u16,
    doc: String,
}

struct Table {
    name: String,
    snake: String,
    element: Qn,
    change: Option<Qn>,
    doc: String,
    order: Vec<Vec<Qn>>,
    attrs: Vec<Attr>,
    fields: Vec<Field>,
}

struct Attr {
    name: String,
    attr: Qn,
    legacy: Option<Qn>,
    codec: String,
    value: String,
    doc: String,
}

struct Names<'a> {
    prefixes: &'a BTreeMap<String, String>,
    locals: &'a BTreeMap<String, String>,
    missing_locals: BTreeSet<String>,
}

impl Names<'_> {
    fn qn(&mut self, text: &str, where_: &str) -> Qn {
        let (prefix, local) = text
            .split_once(':')
            .unwrap_or_else(|| panic!("{where_}: `{text}` 需要 `prefix:local` 形式"));
        let ns = self
            .prefixes
            .get(prefix)
            .unwrap_or_else(|| panic!("{where_}: 前缀 `{prefix}` 不在 schema/namespaces.tsv"))
            .clone();
        let local_variant = match self.locals.get(local) {
            Some(v) => v.clone(),
            None => {
                self.missing_locals.insert(local.to_string());
                String::from("MISSING")
            }
        };
        Qn { ns, local: local_variant, text: text.to_string() }
    }
}

/// 生成 `props.rs` 源码。`prefixes`：前缀 → `NsId` 变体；`locals`：局部名 → `LocalName` 变体。
pub fn generate(
    dir: &Path,
    prefixes: &BTreeMap<String, String>,
    locals: &BTreeMap<String, String>,
) -> String {
    let mut names = Names { prefixes, locals, missing_locals: BTreeSet::new() };

    let types_path = dir.join("types.toml");
    let types: TypesFile = toml::from_str(&fs::read_to_string(&types_path).unwrap())
        .unwrap_or_else(|e| panic!("{}: {e}", types_path.display()));

    let mut table_files: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml") && p != &types_path)
        .collect();
    table_files.sort();
    let mut decls = Vec::new();
    for path in &table_files {
        let f: TableFile = toml::from_str(&fs::read_to_string(path).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        decls.extend(f.table);
    }

    // 名字空间：内建 codec、枚举、结构体、表互不重名。
    let mut seen: BTreeSet<String> = BUILTIN.iter().map(|b| b.0.to_string()).collect();
    seen.insert("Raw".into());
    for n in types.enums.keys().chain(types.structs.keys()).chain(decls.iter().map(|d| &d.name)) {
        assert!(seen.insert(n.clone()), "props: 类型名 `{n}` 重复（或与内建 codec 同名）");
    }
    let table_names: BTreeSet<String> = decls.iter().map(|d| d.name.clone()).collect();

    let resolve = |codec: &str, where_: &str| -> Kind {
        let codec = codec.strip_prefix("Enum<").and_then(|s| s.strip_suffix('>')).unwrap_or(codec);
        if codec == "Raw" {
            return Kind::Raw;
        }
        if let Some((_, value)) = BUILTIN.iter().find(|b| b.0 == codec) {
            return Kind::Scalar { codec: format!("codec::{codec}"), value: (*value).to_string() };
        }
        if types.enums.contains_key(codec) {
            return Kind::Scalar { codec: codec.to_string(), value: format!("Val<{codec}>") };
        }
        if types.structs.contains_key(codec) {
            return Kind::Struct(codec.to_string());
        }
        if table_names.contains(codec) {
            return Kind::Table(codec.to_string());
        }
        panic!("{where_}: 未知 codec `{codec}`");
    };

    let resolve_attrs = |names: &mut Names<'_>,
                         decls: &[AttrDecl],
                         owner: &str,
                         taken: &mut BTreeSet<String>|
     -> Vec<Attr> {
        let mut attrs = Vec::new();
        for a in decls {
            let where_ = format!("{owner}.{}", a.name);
            check_ident(&a.name, &where_);
            assert!(taken.insert(a.name.clone()), "{where_}: 名字重复");
            let Kind::Scalar { codec, value } = resolve(&a.codec, &where_) else {
                panic!("{where_}: 属性 codec 只能是标量");
            };
            attrs.push(Attr {
                name: a.name.clone(),
                attr: names.qn(&a.attr, &where_),
                legacy: a.legacy.as_deref().map(|l| names.qn(l, &where_)),
                codec,
                value,
                doc: a.doc.clone(),
            });
        }
        attrs
    };

    // 结构体
    let mut structs: Vec<(String, &StructDecl, Vec<Attr>)> = Vec::new();
    for (name, decl) in &types.structs {
        let mut taken = BTreeSet::new();
        let attrs = resolve_attrs(&mut names, &decl.attrs, &format!("struct {name}"), &mut taken);
        structs.push((name.clone(), decl, attrs));
    }

    // 表
    let mut tables = Vec::new();
    for d in decls {
        let where_ = format!("table {}", d.name);
        let element = names.qn(&d.element, &where_);
        let change = d.change.as_deref().map(|c| names.qn(c, &where_));
        let order: Vec<Vec<Qn>> = d
            .order
            .iter()
            .map(|entry| entry.split('|').map(|q| names.qn(q.trim(), &where_)).collect())
            .collect();
        let order_of = |q: &Qn| -> Option<u16> {
            order
                .iter()
                .position(|slot| slot.iter().any(|x| x.text == q.text))
                .map(|i| u16::try_from(i).unwrap())
        };
        if let Some(c) = &change {
            assert!(
                order_of(c).is_some(),
                "{where_}: change 元素 `{}` 必须出现在 order 里",
                c.text
            );
        }
        let mut field_names = BTreeSet::new();
        let attrs = resolve_attrs(&mut names, &d.attrs, &where_, &mut field_names);
        let mut fields = Vec::new();
        let mut elements = BTreeSet::new();
        for f in &d.field {
            let where_ = format!("{where_}.{}", f.name);
            check_ident(&f.name, &where_);
            assert!(field_names.insert(f.name.clone()), "{where_}: 字段名重复（或与属性同名）");
            assert!(f.name != "raw_unmodeled", "{where_}: 字段名保留");
            let element = names.qn(&f.element, &where_);
            let legacy = f.legacy.as_deref().map(|l| names.qn(l, &where_));
            assert!(
                elements.insert(element.text.clone()),
                "{where_}: 元素 `{}` 重复建模",
                element.text
            );
            let order = order_of(&element)
                .unwrap_or_else(|| panic!("{where_}: 元素 `{}` 不在 order 列表里", element.text));
            if let Some(l) = &legacy {
                assert!(
                    order_of(l) == Some(order),
                    "{where_}: legacy `{}` 须与 `{}` 同一序号（写成 `a|b`）",
                    l.text,
                    element.text
                );
            }
            let kind = resolve(&f.codec, &where_);
            if matches!(kind, Kind::Raw) {
                assert!(!f.multi, "{where_}: Raw 字段不支持 multi");
            }
            if let Some(t) = &f.cs_twin {
                assert!(
                    d.field.iter().any(|x| &x.name == t),
                    "{where_}: cs_twin `{t}` 不是本表字段"
                );
            }
            fields.push(Field {
                name: f.name.clone(),
                variant: pascal(&f.name),
                element,
                legacy,
                kind,
                multi: f.multi,
                in_change: f.in_change,
                cs_twin: f.cs_twin.clone(),
                order,
                doc: f.doc.clone(),
            });
        }
        // 按 schema 顺序排列：字段枚举序 = FIELDS 索引 = 生成顺序。
        fields.sort_by_key(|f| f.order);
        tables.push(Table {
            snake: snake(&d.name),
            name: d.name,
            element,
            change,
            doc: d.doc,
            order,
            attrs,
            fields,
        });
    }

    assert!(
        names.missing_locals.is_empty(),
        "props: 以下局部名不在 schema/local_names.txt，请补上：\n{}",
        names.missing_locals.iter().cloned().collect::<Vec<_>>().join("\n")
    );

    let mut out = String::new();
    writeln!(out, "// 由 build/props.rs 从 schema/props/*.toml 生成，勿手改。\n").unwrap();
    for (name, decl) in &types.enums {
        gen_enum(&mut out, name, decl);
    }
    for (name, decl, attrs) in &structs {
        gen_struct(&mut out, name, decl, attrs);
    }
    for t in &tables {
        gen_table(&mut out, t);
    }
    gen_index(&mut out, &tables);
    out
}

// ---- 名字工具 ----------------------------------------------------------------------------------

fn check_ident(s: &str, where_: &str) {
    let ok = !s.is_empty()
        && s.chars().all(|c| c == '_' || c.is_ascii_lowercase() || c.is_ascii_digit())
        && !s.starts_with(|c: char| c.is_ascii_digit());
    assert!(ok, "{where_}: 字段名 `{s}` 须为 snake_case 标识符");
    const KEYWORDS: &[&str] =
        &["type", "ref", "match", "mod", "use", "as", "in", "fn", "self", "super"];
    assert!(!KEYWORDS.contains(&s), "{where_}: 字段名 `{s}` 是 Rust 关键字，请换名");
}

fn pascal(snake: &str) -> String {
    let mut s = String::new();
    for part in snake.split('_') {
        let mut c = part.chars();
        if let Some(f) = c.next() {
            s.extend(f.to_uppercase());
            s.push_str(c.as_str());
        }
    }
    s
}

fn snake(pascal: &str) -> String {
    let mut s = String::new();
    for (i, c) in pascal.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                s.push('_');
            }
            s.extend(c.to_lowercase());
        } else {
            s.push(c);
        }
    }
    s
}

fn variant_of_value(v: &str) -> String {
    let mut s = String::new();
    let mut up = true;
    for c in v.chars() {
        if c == '-' || c == '_' || c == '.' {
            up = true;
        } else if up {
            s.extend(c.to_uppercase());
            up = false;
        } else {
            s.push(c);
        }
    }
    if s == "Self" {
        s.push('_');
    }
    s
}

fn doc_attr(out: &mut String, indent: &str, doc: &str) {
    if !doc.is_empty() {
        writeln!(out, "{indent}#[doc = {doc:?}]").unwrap();
    }
}

// ---- 生成：枚举 --------------------------------------------------------------------------------

fn gen_enum(out: &mut String, name: &str, decl: &EnumDecl) {
    let variants: Vec<(String, String)> =
        decl.values.iter().map(|v| (v.clone(), variant_of_value(v))).collect();
    let mut seen = BTreeSet::new();
    for (v, var) in &variants {
        assert!(seen.insert(var.clone()), "enum {name}: 值 `{v}` 的变体名 `{var}` 重复");
    }
    doc_attr(out, "", &decl.doc);
    writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]").unwrap();
    writeln!(out, "pub enum {name} {{").unwrap();
    for (v, var) in &variants {
        writeln!(out, "    /// `{v}`").unwrap();
        writeln!(out, "    {var},").unwrap();
    }
    writeln!(out, "}}\n").unwrap();
    writeln!(out, "impl {name} {{").unwrap();
    write!(out, "    pub const ALL: &[{name}] = &[").unwrap();
    for (_, var) in &variants {
        write!(out, "{name}::{var}, ").unwrap();
    }
    writeln!(out, "];\n").unwrap();
    writeln!(out, "    /// 精确匹配枚举字面；不匹配返回 `None`。").unwrap();
    writeln!(out, "    pub fn parse(s: &str) -> Option<{name}> {{").unwrap();
    writeln!(out, "        match s {{").unwrap();
    for (v, var) in &variants {
        writeln!(out, "            {v:?} => Some({name}::{var}),").unwrap();
    }
    writeln!(out, "            _ => None,").unwrap();
    writeln!(out, "        }}\n    }}\n").unwrap();
    writeln!(out, "    pub const fn as_str(self) -> &'static str {{").unwrap();
    writeln!(out, "        match self {{").unwrap();
    for (v, var) in &variants {
        writeln!(out, "            {name}::{var} => {v:?},").unwrap();
    }
    writeln!(out, "        }}\n    }}\n}}\n").unwrap();
    writeln!(out, "impl codec::Codec for {name} {{").unwrap();
    writeln!(out, "    type Value = Val<{name}>;").unwrap();
    writeln!(out, "    const NAME: &'static str = {name:?};").unwrap();
    writeln!(out, "    fn parse(text: &str, ctx: &mut Ctx<'_>) -> Val<{name}> {{").unwrap();
    writeln!(out, "        codec::parse_enum(text, ctx, Self::NAME, {name}::parse)").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "    fn parse_attr(text: &str, what: &str, ctx: &mut Ctx<'_>) -> Val<{name}> {{")
        .unwrap();
    writeln!(out, "        codec::parse_enum_attr(text, what, ctx, Self::NAME, {name}::parse)")
        .unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "    fn write(v: &Val<{name}>, _flavor: PartFlavor) -> Cow<'_, str> {{").unwrap();
    writeln!(out, "        v.write(|x| Cow::Borrowed(x.as_str()))").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "    fn missing(ctx: &mut Ctx<'_>) -> Val<{name}> {{").unwrap();
    writeln!(out, "        codec::missing_val(ctx, Self::NAME)").unwrap();
    writeln!(out, "    }}\n}}\n").unwrap();
}

// ---- 生成：属性结构体 --------------------------------------------------------------------------

fn gen_struct(out: &mut String, name: &str, decl: &StructDecl, attrs: &[Attr]) {
    doc_attr(out, "", &decl.doc);
    writeln!(out, "#[derive(Debug, Clone, Default, PartialEq, Eq)]").unwrap();
    writeln!(out, "pub struct {name} {{").unwrap();
    for a in attrs {
        let doc = if a.doc.is_empty() {
            format!("`@{}`", a.attr.text)
        } else {
            format!("`@{}`：{}", a.attr.text, a.doc)
        };
        doc_attr(out, "    ", &doc);
        writeln!(out, "    pub {}: Option<{}>,", a.name, a.value).unwrap();
    }
    writeln!(out, "}}\n").unwrap();

    writeln!(out, "impl {name} {{").unwrap();
    writeln!(out, "    pub const ATTRS: &[AttrInfo] = &[").unwrap();
    for a in attrs {
        writeln!(
            out,
            "        AttrInfo {{ name: {:?}, attr: {}, legacy: {} }},",
            a.name,
            a.attr.expr(),
            opt_qn_expr(a.legacy.as_ref())
        )
        .unwrap();
    }
    writeln!(out, "    ];\n").unwrap();

    writeln!(out, "    /// 从元素属性读取；缺席的属性为 `None`。").unwrap();
    writeln!(out, "    pub fn read(node: NodeId, ctx: &mut Ctx<'_>) -> Self {{").unwrap();
    writeln!(out, "        Self {{").unwrap();
    for a in attrs {
        writeln!(
            out,
            "            {}: read_attr::<{}>(node, {}, {}, ctx),",
            a.name,
            a.codec,
            a.attr.expr(),
            opt_qn_expr(a.legacy.as_ref())
        )
        .unwrap();
    }
    writeln!(out, "        }}\n    }}\n").unwrap();

    writeln!(out, "    /// 生成元素 `name`，只写 `Some` 的属性，拼写按 flavor。").unwrap();
    writeln!(out, "    pub fn emit(&self, name: QName, flavor: PartFlavor) -> NewElement {{")
        .unwrap();
    writeln!(out, "        let mut e = NewElement::new(name);").unwrap();
    for a in attrs {
        writeln!(out, "        if let Some(v) = &self.{} {{", a.name).unwrap();
        writeln!(
            out,
            "            e.push_attr(spell(flavor, {}, {}), <{} as codec::Codec>::write(v, flavor));",
            a.attr.expr(),
            opt_qn_expr(a.legacy.as_ref()),
            a.codec
        )
        .unwrap();
        writeln!(out, "        }}").unwrap();
    }
    writeln!(out, "        e\n    }}\n").unwrap();

    writeln!(out, "    /// 逐属性覆盖：`over` 里 `Some` 的属性覆盖 `self`（resolve 的层叠合并）。")
        .unwrap();
    writeln!(out, "    #[allow(clippy::clone_on_copy)]").unwrap();
    writeln!(out, "    pub fn merge(&mut self, over: &Self) {{").unwrap();
    for a in attrs {
        writeln!(
            out,
            "        if over.{n}.is_some() {{ self.{n} = over.{n}.clone(); }}",
            n = a.name
        )
        .unwrap();
    }
    writeln!(out, "    }}\n").unwrap();

    writeln!(out, "    /// 没有任何属性。").unwrap();
    writeln!(out, "    pub fn is_empty(&self) -> bool {{").unwrap();
    let clauses: Vec<String> = attrs.iter().map(|a| format!("self.{}.is_none()", a.name)).collect();
    write_and_chain(out, &clauses);
    writeln!(out, "    }}\n}}\n").unwrap();
}

/// 把若干条件用 " && " 连成一个布尔表达式并写出（空列表退化为 `true`）。
/// 不写引导的 `true &&`，否则生成代码会触发 clippy::nonminimal_bool。
fn write_and_chain(out: &mut impl std::fmt::Write, clauses: &[String]) {
    if clauses.is_empty() {
        writeln!(out, "        true").unwrap();
    } else {
        writeln!(out, "        {}", clauses.join(" && ")).unwrap();
    }
}

// ---- 生成：表 ----------------------------------------------------------------------------------

fn field_ty(f: &Field) -> String {
    let inner = match &f.kind {
        Kind::Scalar { value, .. } => value.clone(),
        Kind::Struct(s) | Kind::Table(s) => s.clone(),
        Kind::Raw => "NodeId".to_string(),
    };
    if f.multi { format!("Vec<{inner}>") } else { format!("Option<{inner}>") }
}

fn patch_ty(f: &Field) -> String {
    match &f.kind {
        Kind::Table(t) if !f.multi => format!("TableChange<{t}, {t}Patch>"),
        _ => {
            let inner = match &f.kind {
                Kind::Scalar { value, .. } => value.clone(),
                Kind::Struct(s) | Kind::Table(s) => s.clone(),
                Kind::Raw => "NodeId".to_string(),
            };
            if f.multi { format!("Change<Vec<{inner}>>") } else { format!("Change<{inner}>") }
        }
    }
}

/// 读取一个子元素为字段值的表达式（`child` 为节点变量）。
fn read_expr(f: &Field) -> String {
    match &f.kind {
        Kind::Scalar { codec, .. } => format!("read_val::<{codec}>(child, ctx)"),
        Kind::Struct(s) => format!("{s}::read(child, ctx)"),
        Kind::Table(t) => format!("read_{}_in(Some(child), ctx)", snake(t)),
        Kind::Raw => "child".to_string(),
    }
}

/// 把值 `x`（引用）生成为新元素的表达式。
fn emit_expr(f: &Field, x: &str) -> String {
    let name = format!("spell(flavor, {}, {})", f.element.expr(), opt_qn_expr(f.legacy.as_ref()));
    match &f.kind {
        Kind::Scalar { codec, .. } => format!("emit_val::<{codec}>({name}, {x}, flavor)"),
        Kind::Struct(_) => format!("{x}.emit({name}, flavor)"),
        Kind::Table(t) => format!("emit_{}({x}, flavor)", snake(t)),
        Kind::Raw => unreachable!(),
    }
}

fn gen_table(out: &mut String, t: &Table) {
    let name = &t.name;
    let snake = &t.snake;
    let upper = snake.to_ascii_uppercase();
    let field_enum = format!("{name}Field");
    let patch = format!("{name}Patch");

    // 结构体
    let doc = if t.doc.is_empty() {
        format!("`{}` 的建模字段（`PROP-08`）。", t.element.text)
    } else {
        format!("`{}`：{}", t.element.text, t.doc)
    };
    doc_attr(out, "", &doc);
    writeln!(out, "#[derive(Debug, Clone, Default)]").unwrap();
    writeln!(out, "pub struct {name} {{").unwrap();
    for a in &t.attrs {
        let d = if a.doc.is_empty() {
            format!("容器属性 `@{}`", a.attr.text)
        } else {
            format!("容器属性 `@{}`：{}", a.attr.text, a.doc)
        };
        doc_attr(out, "    ", &d);
        writeln!(out, "    pub {}: Option<{}>,", a.name, a.value).unwrap();
    }
    for f in &t.fields {
        let mut d = format!("`{}`", f.element.text);
        if let Some(l) = &f.legacy {
            write!(d, "（Transitional 写法 `{}`）", l.text).unwrap();
        }
        if !f.doc.is_empty() {
            write!(d, "：{}", f.doc).unwrap();
        }
        doc_attr(out, "    ", &d);
        writeln!(out, "    pub {}: {},", f.name, field_ty(f)).unwrap();
    }
    writeln!(out, "    /// 未建模的子元素（含重复出现的建模元素）：原位保留，不参与比较。")
        .unwrap();
    writeln!(out, "    pub raw_unmodeled: Vec<NodeId>,").unwrap();
    writeln!(out, "}}\n").unwrap();

    writeln!(out, "impl PartialEq for {name} {{").unwrap();
    writeln!(out, "    fn eq(&self, o: &Self) -> bool {{").unwrap();
    let clauses: Vec<String> = t
        .attrs
        .iter()
        .map(|a| a.name.as_str())
        .chain(t.fields.iter().map(|f| f.name.as_str()))
        .map(|n| format!("self.{n} == o.{n}"))
        .collect();
    write_and_chain(out, &clauses);
    writeln!(out, "    }}\n}}\n").unwrap();
    writeln!(out, "impl Eq for {name} {{}}\n").unwrap();

    // 字段枚举
    writeln!(out, "/// [`{name}`] 的字段标识，按 schema 顺序排列（`PROP-05`）。").unwrap();
    writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]").unwrap();
    writeln!(out, "pub enum {field_enum} {{").unwrap();
    for f in &t.fields {
        writeln!(out, "    {},", f.variant).unwrap();
    }
    writeln!(out, "}}\n").unwrap();
    writeln!(out, "impl {field_enum} {{").unwrap();
    write!(out, "    pub const ALL: &[{field_enum}] = &[").unwrap();
    for f in &t.fields {
        write!(out, "{field_enum}::{}, ", f.variant).unwrap();
    }
    writeln!(out, "];\n").unwrap();
    writeln!(out, "    pub const fn info(self) -> &'static FieldInfo {{").unwrap();
    writeln!(out, "        &{upper}_FIELDS[self as usize]").unwrap();
    writeln!(out, "    }}\n}}\n").unwrap();

    // 元数据
    writeln!(out, "/// [`{name}`] 每个字段的元数据，索引与 [`{field_enum}`] 一致。").unwrap();
    writeln!(out, "pub const {upper}_FIELDS: &[FieldInfo] = &[").unwrap();
    for f in &t.fields {
        let kind = match &f.kind {
            Kind::Scalar { .. } => "Scalar",
            Kind::Struct(_) => "Struct",
            Kind::Table(_) => "Table",
            Kind::Raw => "Raw",
        };
        writeln!(
            out,
            "    FieldInfo {{ name: {:?}, element: {}, legacy: {}, order: {}, kind: FieldKind::{kind}, multi: {}, in_change: {}, cs_twin: {} }},",
            f.name,
            f.element.expr(),
            opt_qn_expr(f.legacy.as_ref()),
            f.order,
            f.multi,
            f.in_change,
            f.cs_twin.as_ref().map_or_else(|| "None".to_string(), |s| format!("Some({s:?})")),
        )
        .unwrap();
    }
    writeln!(out, "];\n").unwrap();

    writeln!(out, "/// [`{name}`] 容器元素自身的属性。").unwrap();
    writeln!(out, "pub const {upper}_ATTRS: &[AttrInfo] = &[").unwrap();
    for a in &t.attrs {
        writeln!(
            out,
            "    AttrInfo {{ name: {:?}, attr: {}, legacy: {} }},",
            a.name,
            a.attr.expr(),
            opt_qn_expr(a.legacy.as_ref())
        )
        .unwrap();
    }
    writeln!(out, "];\n").unwrap();

    writeln!(out, "/// [`{name}`] 的表信息（容器元素、修订快照元素、字段、顺序）。").unwrap();
    writeln!(out, "pub const {upper}: TableInfo = TableInfo {{").unwrap();
    writeln!(out, "    name: {name:?},").unwrap();
    writeln!(out, "    element: {},", t.element.expr()).unwrap();
    writeln!(out, "    change: {},", opt_qn_expr(t.change.as_ref())).unwrap();
    writeln!(out, "    attrs: {upper}_ATTRS,").unwrap();
    writeln!(out, "    fields: {upper}_FIELDS,").unwrap();
    writeln!(out, "    order_index: order_index_{snake},").unwrap();
    writeln!(out, "}};\n").unwrap();

    // 顺序
    writeln!(
        out,
        "/// `{}` 子元素的 schema 序号（`PROP-05`）；表外元素为 `None`。",
        t.element.text
    )
    .unwrap();
    writeln!(out, "pub fn order_index_{snake}(name: QName) -> Option<u16> {{").unwrap();
    writeln!(out, "    match name {{").unwrap();
    for (i, slot) in t.order.iter().enumerate() {
        let pats: Vec<String> = slot.iter().map(Qn::pat).collect();
        writeln!(out, "        {} => Some({i}),", pats.join(" | ")).unwrap();
    }
    writeln!(out, "        _ => None,").unwrap();
    writeln!(out, "    }}\n}}\n").unwrap();

    // 读取
    writeln!(
        out,
        "/// 读取容器 `{}`（`PROP-07`）；`container` 为 `None` 时返回全空。",
        t.element.text
    )
    .unwrap();
    writeln!(
        out,
        "pub fn read_{snake}(dom: &Dom, container: Option<NodeId>, diags: &mut Vec<Diagnostic>) -> {name} {{"
    )
    .unwrap();
    writeln!(out, "    let mut ctx = Ctx::new(dom, diags);").unwrap();
    writeln!(out, "    read_{snake}_in(container, &mut ctx)\n}}\n").unwrap();

    writeln!(
        out,
        "pub(crate) fn read_{snake}_in(container: Option<NodeId>, ctx: &mut Ctx<'_>) -> {name} {{"
    )
    .unwrap();
    writeln!(out, "    let mut out = {name}::default();").unwrap();
    writeln!(out, "    let Some(c) = container else {{ return out }};").unwrap();
    writeln!(out, "    let dom = ctx.dom();").unwrap();
    if !t.attrs.is_empty() {
        writeln!(out, "    ctx.enter(c);").unwrap();
        for a in &t.attrs {
            writeln!(
                out,
                "    out.{} = read_attr::<{}>(c, {}, {}, ctx);",
                a.name,
                a.codec,
                a.attr.expr(),
                opt_qn_expr(a.legacy.as_ref())
            )
            .unwrap();
        }
    }
    writeln!(out, "    for child in dom.semantic_children(c) {{").unwrap();
    writeln!(out, "        let Some(name) = dom.name(child) else {{ continue }};").unwrap();
    writeln!(out, "        ctx.enter(child);").unwrap();
    writeln!(out, "        match name {{").unwrap();
    for f in &t.fields {
        let mut pats = vec![f.element.pat()];
        if let Some(l) = &f.legacy {
            pats.push(l.pat());
        }
        writeln!(out, "            {} => {{", pats.join(" | ")).unwrap();
        if f.multi {
            writeln!(out, "                out.{}.push({});", f.name, read_expr(f)).unwrap();
        } else {
            writeln!(out, "                if out.{}.is_none() {{", f.name).unwrap();
            writeln!(out, "                    out.{} = Some({});", f.name, read_expr(f)).unwrap();
            writeln!(out, "                }} else {{").unwrap();
            writeln!(out, "                    out.raw_unmodeled.push(child);").unwrap();
            writeln!(out, "                }}").unwrap();
        }
        writeln!(out, "            }}").unwrap();
    }
    writeln!(out, "            _ => out.raw_unmodeled.push(child),").unwrap();
    writeln!(out, "        }}\n    }}\n    out\n}}\n").unwrap();

    if let Some(change) = &t.change {
        writeln!(
            out,
            "/// `{}` 中的旧值快照（`PROP-06`）：返回快照元素与其中的属性容器内容。",
            change.text
        )
        .unwrap();
        writeln!(
            out,
            "pub fn read_{snake}_change(dom: &Dom, container: Option<NodeId>, diags: &mut Vec<Diagnostic>) -> Option<(NodeId, {name})> {{"
        )
        .unwrap();
        writeln!(out, "    let c = container?;").unwrap();
        writeln!(
            out,
            "    let change = dom.semantic_children(c).find(|&n| dom.is(n, {}))?;",
            change.expr()
        )
        .unwrap();
        writeln!(
            out,
            "    let inner = dom.semantic_children(change).find(|&n| dom.is(n, {}));",
            t.element.expr()
        )
        .unwrap();
        writeln!(out, "    let mut ctx = Ctx::new(dom, diags);").unwrap();
        writeln!(out, "    Some((change, read_{snake}_in(inner, &mut ctx)))\n}}\n").unwrap();
    }

    // Patch
    writeln!(
        out,
        "/// [`{name}`] 的变更集（`PROP-06`）：每个字段 `Keep | Unset | Set`，嵌套表另有 `Patch`。"
    )
    .unwrap();
    writeln!(out, "#[derive(Debug, Clone, Default, PartialEq, Eq)]").unwrap();
    writeln!(out, "pub struct {patch} {{").unwrap();
    for a in &t.attrs {
        writeln!(out, "    pub {}: Change<{}>,", a.name, a.value).unwrap();
    }
    for f in &t.fields {
        writeln!(out, "    pub {}: {},", f.name, patch_ty(f)).unwrap();
    }
    writeln!(out, "}}\n").unwrap();

    writeln!(out, "impl PropsPatch for {patch} {{").unwrap();
    writeln!(out, "    fn is_empty(&self) -> bool {{").unwrap();
    // 逐项 `is_keep()` 用 " && " 连接；不写引导的 `true &&`，否则生成代码会触发
    // clippy::nonminimal_bool（表为空时才退化成单独的 `true`）
    let clauses: Vec<String> = t
        .attrs
        .iter()
        .map(|a| a.name.as_str())
        .chain(t.fields.iter().map(|f| f.name.as_str()))
        .map(|n| format!("self.{n}.is_keep()"))
        .collect();
    write_and_chain(out, &clauses);
    writeln!(out, "    }}\n}}\n").unwrap();

    writeln!(out, "impl {patch} {{").unwrap();
    writeln!(out, "    pub fn kind(&self, field: {field_enum}) -> ChangeKind {{").unwrap();
    writeln!(out, "        match field {{").unwrap();
    for f in &t.fields {
        writeln!(out, "            {field_enum}::{} => self.{}.kind(),", f.variant, f.name)
            .unwrap();
    }
    writeln!(out, "        }}\n    }}\n").unwrap();

    writeln!(
        out,
        "    /// 字段 `Set` 值对应的新元素；非 `Set`（含嵌套 `Patch`、`Raw` 字段）返回空。"
    )
    .unwrap();
    writeln!(out, "    pub fn emit_field(&self, field: {field_enum}, flavor: PartFlavor) -> Vec<NewElement> {{").unwrap();
    writeln!(out, "        match field {{").unwrap();
    for f in &t.fields {
        write!(out, "            {field_enum}::{} => ", f.variant).unwrap();
        match (&f.kind, f.multi) {
            (Kind::Raw, _) => writeln!(out, "Vec::new(),").unwrap(),
            (Kind::Table(_), false) => writeln!(
                out,
                "match &self.{} {{ TableChange::Set(x) => vec![{}], _ => Vec::new() }},",
                f.name,
                emit_expr(f, "x")
            )
            .unwrap(),
            (_, true) => writeln!(
                out,
                "match &self.{} {{ Change::Set(xs) => xs.iter().map(|x| {}).collect(), _ => Vec::new() }},",
                f.name,
                emit_expr(f, "x")
            )
            .unwrap(),
            (_, false) => writeln!(
                out,
                "match &self.{} {{ Change::Set(x) => vec![{}], _ => Vec::new() }},",
                f.name,
                emit_expr(f, "x")
            )
            .unwrap(),
        }
    }
    writeln!(out, "        }}\n    }}\n}}\n").unwrap();

    // diff
    writeln!(out, "/// `a` → `b` 的变更集：相等 `Keep`，`b` 缺席 `Unset`，否则 `Set`（嵌套表两侧都有时为 `Patch`）。").unwrap();
    writeln!(out, "pub fn diff_{snake}(a: &{name}, b: &{name}) -> {patch} {{").unwrap();
    writeln!(out, "    {patch} {{").unwrap();
    for a in &t.attrs {
        writeln!(out, "        {n}: Change::diff(&a.{n}, &b.{n}),", n = a.name).unwrap();
    }
    for f in &t.fields {
        let expr = match (&f.kind, f.multi) {
            (Kind::Table(t2), false) => {
                format!("TableChange::diff(&a.{n}, &b.{n}, diff_{})", self::snake(t2), n = f.name)
            }
            (_, true) => format!("Change::diff_multi(&a.{n}, &b.{n})", n = f.name),
            (_, false) => format!("Change::diff(&a.{n}, &b.{n})", n = f.name),
        };
        writeln!(out, "        {}: {expr},", f.name).unwrap();
    }
    writeln!(out, "    }}\n}}\n").unwrap();

    // emit 整容器 / 单字段值
    writeln!(
        out,
        "/// 按 codec 生成整个 `{}`（只含建模字段，按 schema 顺序；`raw_unmodeled` 不在其中）。",
        t.element.text
    )
    .unwrap();
    writeln!(out, "pub fn emit_{snake}(v: &{name}, flavor: PartFlavor) -> NewElement {{").unwrap();
    writeln!(out, "    let mut e = NewElement::new({});", t.element.expr()).unwrap();
    for a in &t.attrs {
        writeln!(out, "    if let Some(x) = &v.{} {{", a.name).unwrap();
        writeln!(
            out,
            "        e.push_attr(spell(flavor, {}, {}), <{} as codec::Codec>::write(x, flavor));",
            a.attr.expr(),
            opt_qn_expr(a.legacy.as_ref()),
            a.codec
        )
        .unwrap();
        writeln!(out, "    }}").unwrap();
    }
    writeln!(out, "    for f in {field_enum}::ALL {{").unwrap();
    writeln!(out, "        for c in emit_{snake}_value(v, *f, flavor) {{").unwrap();
    writeln!(out, "            e.push_child(c);").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}\n    e\n}}\n").unwrap();

    writeln!(out, "/// 字段当前值对应的新元素（`None` / 空列表 / `Raw` 字段 → 空）。").unwrap();
    writeln!(out, "pub fn emit_{snake}_value(v: &{name}, field: {field_enum}, flavor: PartFlavor) -> Vec<NewElement> {{").unwrap();
    writeln!(out, "    match field {{").unwrap();
    for f in &t.fields {
        write!(out, "        {field_enum}::{} => ", f.variant).unwrap();
        if matches!(f.kind, Kind::Raw) {
            writeln!(out, "Vec::new(),").unwrap();
        } else {
            writeln!(out, "v.{}.iter().map(|x| {}).collect(),", f.name, emit_expr(f, "x")).unwrap();
        }
    }
    writeln!(out, "    }}\n}}\n").unwrap();

    gen_plan(out, t);
    gen_merge(out, t);
}

/// `merge_x(base, over) -> Vec<XField>`：层叠合并（`RES-03` "None 不覆盖"）。标量整字段覆盖；
/// struct 逐属性合并；嵌套表递归；multi 非空整表覆盖；Raw 覆盖。返回被 `over` 改动的字段。
fn gen_merge(out: &mut String, t: &Table) {
    let name = &t.name;
    let snake = &t.snake;
    let field_enum = format!("{name}Field");
    writeln!(out, "/// 层叠合并：`over` 中已声明的字段覆盖 `base`（`None` 不覆盖；struct 逐属性、嵌套表递归、multi 整表）。").unwrap();
    writeln!(out, "/// 返回 `over` 覆盖到的字段。").unwrap();
    writeln!(out, "#[allow(clippy::clone_on_copy)]").unwrap();
    writeln!(out, "pub fn merge_{snake}(base: &mut {name}, over: &{name}) -> Vec<{field_enum}> {{")
        .unwrap();
    writeln!(out, "    let mut touched = Vec::new();").unwrap();
    for a in &t.attrs {
        writeln!(out, "    if over.{n}.is_some() {{ base.{n} = over.{n}.clone(); }}", n = a.name)
            .unwrap();
    }
    for f in &t.fields {
        let n = &f.name;
        let v = &f.variant;
        match (&f.kind, f.multi) {
            (_, true) => {
                writeln!(out, "    if !over.{n}.is_empty() {{ base.{n} = over.{n}.clone(); touched.push({field_enum}::{v}); }}").unwrap();
            }
            (Kind::Struct(_), false) => {
                writeln!(out, "    if let Some(o) = &over.{n} {{").unwrap();
                writeln!(out, "        match &mut base.{n} {{ Some(b) => b.merge(o), None => base.{n} = Some(o.clone()) }}").unwrap();
                writeln!(out, "        touched.push({field_enum}::{v});").unwrap();
                writeln!(out, "    }}").unwrap();
            }
            (Kind::Table(t2), false) => {
                writeln!(out, "    if let Some(o) = &over.{n} {{").unwrap();
                writeln!(out, "        match &mut base.{n} {{ Some(b) => {{ merge_{}(b, o); }}, None => base.{n} = Some(o.clone()) }}", self::snake(t2)).unwrap();
                writeln!(out, "        touched.push({field_enum}::{v});").unwrap();
                writeln!(out, "    }}").unwrap();
            }
            _ => {
                writeln!(out, "    if over.{n}.is_some() {{ base.{n} = over.{n}.clone(); touched.push({field_enum}::{v}); }}").unwrap();
            }
        }
    }
    writeln!(out, "    touched\n}}\n").unwrap();
}

/// `apply_*_patch`（patch 施加到值）与 `plan_apply_*`（PROP-06 合并写回，只产出 `NodeEdit`）。
fn gen_plan(out: &mut String, t: &Table) {
    let name = &t.name;
    let snake = &t.snake;
    let field_enum = format!("{name}Field");
    let patch = format!("{name}Patch");

    writeln!(
        out,
        "/// 把 patch 施加到值上：`Keep` 不动、`Unset` 清空、`Set` 替换、嵌套 `Patch` 递归。"
    )
    .unwrap();
    writeln!(out, "#[allow(clippy::clone_on_copy)]").unwrap();
    writeln!(out, "pub fn apply_{snake}_patch(v: &mut {name}, p: &{patch}) {{").unwrap();
    for a in &t.attrs {
        let n = &a.name;
        writeln!(
            out,
            "    match &p.{n} {{ Change::Keep => {{}}, Change::Unset => v.{n} = None, Change::Set(x) => v.{n} = Some(x.clone()) }}"
        )
        .unwrap();
    }
    for f in &t.fields {
        let n = &f.name;
        let line = match (&f.kind, f.multi) {
            (Kind::Table(t2), false) => format!(
                "    match &p.{n} {{ TableChange::Keep => {{}}, TableChange::Unset => v.{n} = None, TableChange::Set(x) => v.{n} = Some(x.clone()), TableChange::Patch(sp) => apply_{}_patch(v.{n}.get_or_insert_with(Default::default), sp) }}",
                self::snake(t2)
            ),
            (_, true) => format!(
                "    match &p.{n} {{ Change::Keep => {{}}, Change::Unset => v.{n}.clear(), Change::Set(x) => v.{n} = x.clone() }}"
            ),
            (_, false) => format!(
                "    match &p.{n} {{ Change::Keep => {{}}, Change::Unset => v.{n} = None, Change::Set(x) => v.{n} = Some(x.clone()) }}"
            ),
        };
        writeln!(out, "{line}").unwrap();
    }
    writeln!(out, "}}\n").unwrap();

    writeln!(out, "/// `PROP-06` 合并写回：只产出计划，不改 DOM。先把 patch 施加到当前值再 diff，所以 `Set` 同值是空计划。").unwrap();
    writeln!(
        out,
        "/// `container` 缺席且有效变更非空 → 新容器插为 `parent` 的第一个语义子节点之前。"
    )
    .unwrap();
    writeln!(
        out,
        "pub fn plan_apply_{snake}(dom: &Dom, parent: NodeId, container: Option<NodeId>, patch: &{patch}, flavor: PartFlavor) -> Vec<NodeEdit> {{"
    )
    .unwrap();
    writeln!(out, "    let mut out = Vec::new();").unwrap();
    writeln!(out, "    let before = dom.semantic_children(parent).next();").unwrap();
    writeln!(out, "    plan_apply_{snake}_at(dom, Target::Node(parent), container, before, patch, flavor, &mut out);").unwrap();
    writeln!(out, "    out\n}}\n").unwrap();

    writeln!(out, "#[allow(clippy::too_many_arguments)]").unwrap();
    writeln!(
        out,
        "pub(crate) fn plan_apply_{snake}_at(dom: &Dom, parent: Target, container: Option<NodeId>, before: Option<NodeId>, patch: &{patch}, flavor: PartFlavor, out: &mut Vec<NodeEdit>) {{"
    )
    .unwrap();
    writeln!(out, "    let mut sink = Vec::new();").unwrap();
    writeln!(out, "    let current = read_{snake}_in(container, &mut Ctx::new(dom, &mut sink));")
        .unwrap();
    writeln!(out, "    let mut desired = current.clone();").unwrap();
    writeln!(out, "    apply_{snake}_patch(&mut desired, patch);").unwrap();
    writeln!(out, "    let eff = diff_{snake}(&current, &desired);").unwrap();
    writeln!(out, "    if eff.is_empty() {{\n        return;\n    }}").unwrap();
    writeln!(out, "    let Some(c) = container else {{").unwrap();
    let raw_fields: Vec<&Field> = t.fields.iter().filter(|f| matches!(f.kind, Kind::Raw)).collect();
    if raw_fields.is_empty() {
        writeln!(out, "        out.push(NodeEdit::Insert {{ parent, before, node: emit_{snake}(&desired, flavor) }});").unwrap();
    } else {
        writeln!(out, "        let k = out.len();").unwrap();
        writeln!(out, "        out.push(NodeEdit::Insert {{ parent, before, node: emit_{snake}(&desired, flavor) }});").unwrap();
        for f in &raw_fields {
            writeln!(out, "        if let Some(src) = desired.{} {{", f.name).unwrap();
            writeln!(out, "            out.push(NodeEdit::InsertClone {{ parent: Target::New(k), before: None, source: src }});").unwrap();
            writeln!(out, "        }}").unwrap();
        }
    }
    writeln!(out, "        return;\n    }};").unwrap();
    for a in &t.attrs {
        let n = &a.name;
        writeln!(out, "    match &eff.{n} {{").unwrap();
        writeln!(out, "        Change::Keep => {{}}").unwrap();
        writeln!(out, "        Change::Unset => {{").unwrap();
        writeln!(
            out,
            "            out.push(NodeEdit::RemoveAttr {{ node: Target::Node(c), name: {} }});",
            a.attr.expr()
        )
        .unwrap();
        if let Some(l) = &a.legacy {
            writeln!(
                out,
                "            out.push(NodeEdit::RemoveAttr {{ node: Target::Node(c), name: {} }});",
                l.expr()
            )
            .unwrap();
        }
        writeln!(out, "        }}").unwrap();
        writeln!(out, "        Change::Set(x) => {{").unwrap();
        if let Some(l) = &a.legacy {
            writeln!(out, "            if dom.attr(c, {}).is_some() {{", l.expr()).unwrap();
            writeln!(out, "                out.push(NodeEdit::RemoveAttr {{ node: Target::Node(c), name: {} }});", l.expr()).unwrap();
            writeln!(out, "            }}").unwrap();
            writeln!(out, "            if dom.attr(c, {}).is_some() {{", a.attr.expr()).unwrap();
            writeln!(out, "                out.push(NodeEdit::RemoveAttr {{ node: Target::Node(c), name: {} }});", a.attr.expr()).unwrap();
            writeln!(out, "            }}").unwrap();
        }
        writeln!(
            out,
            "            out.push(NodeEdit::SetAttr {{ node: Target::Node(c), name: spell(flavor, {}, {}), value: <{} as codec::Codec>::write(x, flavor).into_owned() }});",
            a.attr.expr(),
            opt_qn_expr(a.legacy.as_ref()),
            a.codec
        )
        .unwrap();
        writeln!(out, "        }}\n    }}").unwrap();
    }
    writeln!(out, "    let kids: Vec<(NodeId, QName)> = dom.semantic_children(c).filter_map(|n| dom.name(n).map(|q| (n, q))).collect();").unwrap();
    writeln!(out, "    let anchor = |order: u16| kids.iter().find(|(_, q)| order_index_{snake}(*q).is_some_and(|i| i > order)).map(|(n, _)| *n);").unwrap();
    writeln!(out, "    for f in {field_enum}::ALL {{").unwrap();
    writeln!(out, "        let info = f.info();").unwrap();
    writeln!(out, "        let existing: Vec<NodeId> = kids.iter().filter(|(_, q)| *q == info.element || Some(*q) == info.legacy).map(|(n, _)| *n).collect();").unwrap();
    writeln!(out, "        match eff.kind(*f) {{").unwrap();
    writeln!(out, "            ChangeKind::Keep => {{}}").unwrap();
    writeln!(out, "            ChangeKind::Unset => out.extend(existing.iter().map(|&n| NodeEdit::Delete(n))),").unwrap();
    writeln!(out, "            ChangeKind::Set | ChangeKind::Patch => match f {{").unwrap();
    for f in &t.fields {
        let v = &f.variant;
        let line = match (&f.kind, f.multi) {
            (Kind::Raw, _) => format!(
                "                {field_enum}::{v} => plan_raw(&existing, anchor(info.order), desired.{}, c, out),",
                f.name
            ),
            (Kind::Table(t2), false) => {
                let s2 = self::snake(t2);
                format!(
                    "                {field_enum}::{v} => match &eff.{n} {{ TableChange::Set(x) => plan_apply_{s2}_at(dom, Target::Node(c), existing.first().copied(), anchor(info.order), &diff_{s2}(&Default::default(), x), flavor, out), TableChange::Patch(sp) => plan_apply_{s2}_at(dom, Target::Node(c), existing.first().copied(), anchor(info.order), sp, flavor, out), _ => {{}} }},",
                    n = f.name
                )
            }
            (_, true) => format!(
                "                {field_enum}::{v} => plan_multi(&existing, anchor(info.order), eff.emit_field(*f, flavor), c, out),"
            ),
            (_, false) => format!(
                "                {field_enum}::{v} => plan_single(&existing, anchor(info.order), eff.emit_field(*f, flavor), c, out),"
            ),
        };
        writeln!(out, "{line}").unwrap();
    }
    writeln!(out, "            }},").unwrap();
    writeln!(out, "        }}\n    }}\n}}\n").unwrap();
}

fn gen_index(out: &mut String, tables: &[Table]) {
    writeln!(out, "/// 全部属性表。").unwrap();
    write!(out, "pub const TABLES: &[&TableInfo] = &[").unwrap();
    for t in tables {
        write!(out, "&{}, ", t.snake.to_ascii_uppercase()).unwrap();
    }
    writeln!(out, "];").unwrap();
}
