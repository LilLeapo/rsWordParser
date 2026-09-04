//! 构建脚本：
//! - 由 `schema/namespaces.tsv` 与 `schema/local_names.txt` 生成 `NsId` / `LocalName`（XML-05，任务 0.6），
//!   输出 `$OUT_DIR/names.rs`，由 `src/xml/names.rs` include；
//! - 由 `schema/props/*.toml` 生成属性表（PROP-07，任务 1.1），输出 `$OUT_DIR/props.rs`，
//!   由 `src/semantic/props/mod.rs` include（生成器在 `build/props.rs`）。

use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

#[path = "build/props.rs"]
mod props;

struct Ns {
    variant: String,
    prefix: Option<String>,
    transitional: String,
    strict: Option<String>,
}

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let ns_path = Path::new(&manifest).join("schema/namespaces.tsv");
    let ln_path = Path::new(&manifest).join("schema/local_names.txt");
    let props_dir = Path::new(&manifest).join("schema/props");
    println!("cargo:rerun-if-changed={}", ns_path.display());
    println!("cargo:rerun-if-changed={}", ln_path.display());
    println!("cargo:rerun-if-changed={}", props_dir.display());
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/props.rs");

    let namespaces = parse_namespaces(&fs::read_to_string(&ns_path).unwrap());
    let locals = parse_local_names(&fs::read_to_string(&ln_path).unwrap());

    let mut out = String::new();
    gen_ns(&mut out, &namespaces);
    gen_local(&mut out, &locals);

    let out_dir = env::var("OUT_DIR").unwrap();
    fs::write(Path::new(&out_dir).join("names.rs"), out).unwrap();

    let prefixes: BTreeMap<String, String> = namespaces
        .iter()
        .filter_map(|n| n.prefix.clone().map(|p| (p, n.variant.clone())))
        .collect();
    let local_map: BTreeMap<String, String> = locals.iter().cloned().collect();
    let props_src = props::generate(&props_dir, &prefixes, &local_map);
    fs::write(Path::new(&out_dir).join("props.rs"), props_src).unwrap();
}

fn parse_namespaces(text: &str) -> Vec<Ns> {
    let mut v = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        assert!(cols.len() == 4, "namespaces.tsv line {}: expected 4 tab-separated columns", i + 1);
        let opt = |s: &str| if s == "-" { None } else { Some(s.to_string()) };
        v.push(Ns {
            variant: cols[0].to_string(),
            prefix: opt(cols[1]),
            transitional: cols[2].to_string(),
            strict: opt(cols[3]),
        });
    }
    for reserved in ["None", "Unbound", "Other"] {
        assert!(v.iter().all(|n| n.variant != reserved), "NsId::{reserved} is reserved");
    }
    v
}

/// (xml 局部名, Rust 变体名)，按变体名去重校验。
fn parse_local_names(text: &str) -> Vec<(String, String)> {
    let mut by_variant: BTreeMap<String, String> = BTreeMap::new();
    let mut v = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, variant) = match line.split_once('=') {
            Some((n, var)) => (n.trim().to_string(), var.trim().to_string()),
            None => (line.to_string(), default_variant(line)),
        };
        assert!(variant != "Other", "local_names.txt line {}: `Other` is reserved", i + 1);
        if let Some(prev) = by_variant.insert(variant.clone(), name.clone()) {
            panic!(
                "local_names.txt line {}: variant {variant} for `{name}` collides with `{prev}`; use `name=Variant`",
                i + 1
            );
        }
        v.push((name, variant));
    }
    v
}

fn default_variant(name: &str) -> String {
    let mut s = String::new();
    let mut chars = name.chars();
    if let Some(c) = chars.next() {
        s.extend(c.to_uppercase());
    }
    for c in chars {
        s.push(if c == '-' || c == '.' { '_' } else { c });
    }
    if s == "Self" {
        s.push('_');
    }
    s
}

fn gen_ns(out: &mut String, ns: &[Ns]) {
    writeln!(out, "/// 命名空间身份（`XML-05`）。Strict 与 Transitional 同族 URI 映射到同一变体。")
        .unwrap();
    writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]").unwrap();
    writeln!(out, "pub enum NsId {{").unwrap();
    for n in ns {
        writeln!(out, "    /// `{}`", n.transitional).unwrap();
        writeln!(out, "    {},", n.variant).unwrap();
    }
    writeln!(out, "    /// 无命名空间（无前缀的属性）。").unwrap();
    writeln!(out, "    None,").unwrap();
    writeln!(out, "    /// 前缀未绑定（`XML_UNBOUND_PREFIX`）；载荷是前缀。").unwrap();
    writeln!(out, "    Unbound(Interned),").unwrap();
    writeln!(out, "    /// 表外命名空间；载荷是 URI。").unwrap();
    writeln!(out, "    Other(Interned),").unwrap();
    writeln!(out, "}}\n").unwrap();

    writeln!(out, "impl NsId {{").unwrap();
    writeln!(out, "    /// 表内全部命名空间。").unwrap();
    write!(out, "    pub const KNOWN: &[NsId] = &[").unwrap();
    for n in ns {
        write!(out, "NsId::{}, ", n.variant).unwrap();
    }
    writeln!(out, "];\n").unwrap();

    writeln!(out, "    /// 规范前缀（`PKG-09`）；默认命名空间（Content_Types、Relationships）为 `Some(\"\")`。").unwrap();
    writeln!(out, "    pub const fn canonical_prefix(self) -> Option<&'static str> {{").unwrap();
    writeln!(out, "        match self {{").unwrap();
    for n in ns {
        writeln!(
            out,
            "            NsId::{} => Some(\"{}\"),",
            n.variant,
            n.prefix.clone().unwrap_or_default()
        )
        .unwrap();
    }
    writeln!(out, "            NsId::None | NsId::Unbound(_) | NsId::Other(_) => None,").unwrap();
    writeln!(out, "        }}\n    }}\n").unwrap();

    writeln!(
        out,
        "    /// 该族的 URI。没有 Strict 变体的命名空间（包级、厂商扩展）两族返回同一 URI。"
    )
    .unwrap();
    writeln!(out, "    pub const fn uri(self, flavor: PartFlavor) -> Option<&'static str> {{")
        .unwrap();
    writeln!(out, "        match (self, flavor) {{").unwrap();
    for n in ns {
        if let Some(s) = &n.strict {
            writeln!(
                out,
                "            (NsId::{}, PartFlavor::Strict) => Some(\"{}\"),",
                n.variant, s
            )
            .unwrap();
            writeln!(
                out,
                "            (NsId::{}, PartFlavor::Transitional) => Some(\"{}\"),",
                n.variant, n.transitional
            )
            .unwrap();
        } else {
            writeln!(out, "            (NsId::{}, _) => Some(\"{}\"),", n.variant, n.transitional)
                .unwrap();
        }
    }
    writeln!(out, "            (NsId::None | NsId::Unbound(_) | NsId::Other(_), _) => None,")
        .unwrap();
    writeln!(out, "        }}\n    }}\n").unwrap();

    writeln!(out, "    /// URI → 身份与族别。表外 URI 返回 `None`（调用方 intern 为 `Other`）。")
        .unwrap();
    writeln!(out, "    pub fn from_uri(uri: &str) -> Option<(NsId, PartFlavor)> {{").unwrap();
    writeln!(out, "        match uri {{").unwrap();
    for n in ns {
        writeln!(
            out,
            "            \"{}\" => Some((NsId::{}, PartFlavor::Transitional)),",
            n.transitional, n.variant
        )
        .unwrap();
        if let Some(s) = &n.strict {
            writeln!(
                out,
                "            \"{}\" => Some((NsId::{}, PartFlavor::Strict)),",
                s, n.variant
            )
            .unwrap();
        }
    }
    writeln!(out, "            _ => None,").unwrap();
    writeln!(out, "        }}\n    }}\n").unwrap();

    writeln!(out, "    /// 该命名空间是否有独立的 Strict URI（即 flavor 会改变输出）。").unwrap();
    writeln!(out, "    pub const fn has_strict_uri(self) -> bool {{").unwrap();
    write!(out, "        matches!(self").unwrap();
    let with_strict: Vec<&Ns> = ns.iter().filter(|n| n.strict.is_some()).collect();
    if with_strict.is_empty() {
        write!(out, ", NsId::None if false").unwrap();
    } else {
        write!(out, ", ").unwrap();
        for (i, n) in with_strict.iter().enumerate() {
            if i > 0 {
                write!(out, " | ").unwrap();
            }
            write!(out, "NsId::{}", n.variant).unwrap();
        }
    }
    writeln!(out, ")\n    }}").unwrap();
    writeln!(out, "}}\n").unwrap();
}

fn gen_local(out: &mut String, locals: &[(String, String)]) {
    writeln!(out, "/// 局部名（`XML-05`）。表外名字落到 `Other`。").unwrap();
    writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]").unwrap();
    writeln!(out, "pub enum LocalName {{").unwrap();
    for (name, variant) in locals {
        writeln!(out, "    /// `{name}`").unwrap();
        writeln!(out, "    {variant},").unwrap();
    }
    writeln!(out, "    /// 表外局部名。").unwrap();
    writeln!(out, "    Other(Interned),").unwrap();
    writeln!(out, "}}\n").unwrap();

    writeln!(out, "impl LocalName {{").unwrap();
    writeln!(out, "    /// 表内名字数。").unwrap();
    writeln!(out, "    pub const KNOWN_COUNT: usize = {};\n", locals.len()).unwrap();
    writeln!(out, "    /// 表内查找；表外返回 `None`。").unwrap();
    writeln!(out, "    pub fn known(s: &str) -> Option<LocalName> {{").unwrap();
    writeln!(out, "        match s {{").unwrap();
    for (name, variant) in locals {
        writeln!(out, "            \"{name}\" => Some(LocalName::{variant}),").unwrap();
    }
    writeln!(out, "            _ => None,").unwrap();
    writeln!(out, "        }}\n    }}\n").unwrap();
    writeln!(out, "    /// 表内名字的原文；`Other` 返回 `None`（需经 `Interner` 解析）。").unwrap();
    writeln!(out, "    pub const fn known_str(self) -> Option<&'static str> {{").unwrap();
    writeln!(out, "        match self {{").unwrap();
    for (name, variant) in locals {
        writeln!(out, "            LocalName::{variant} => Some(\"{name}\"),").unwrap();
    }
    writeln!(out, "            LocalName::Other(_) => None,").unwrap();
    writeln!(out, "        }}\n    }}").unwrap();
    writeln!(out, "}}").unwrap();
}
