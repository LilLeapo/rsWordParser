//! BIND-11：成文稳定面、源码中的定义/固有 impl、审计注解三者双向锁定。
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const CONTRACT: &str = include_str!("../../../docs/13-public-api.md");
const AUDIT: &str = "cfg_attr(rsword_api_docs,deny(missing_docs))";

fn section(name: &str) -> BTreeSet<String> {
    let marker = format!("```{name}\n");
    let body = CONTRACT.split_once(&marker).expect("缺少成文清单").1.split_once("```").unwrap().0;
    let lines: Vec<_> = body.lines().filter(|s| !s.is_empty()).map(str::to_owned).collect();
    let set = lines.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(lines.len(), set.len(), "清单不许重复");
    set
}

#[derive(Debug)]
struct Item {
    kind: &'static str,
    name: String,
    audit: bool,
    extensible: bool,
    attrs: Vec<String>,
}

fn word(token: &TokenTree, expected: &str) -> bool {
    matches!(token, TokenTree::Ident(id) if id == expected)
}

// 只读取声明头，不解释字段/方法体。tokenizer 把注释、字符串隔离；宏模板的 $table
// 也保留为 token。显式工作栈同时扫描普通源码和宏定义，不能漏掉新加的固有 impl。
fn items(source: &str) -> Vec<Item> {
    let mut work = vec![source.parse::<TokenStream>().expect("Rust token 化失败")];
    let mut result = Vec::new();
    while let Some(stream) = work.pop() {
        let tokens: Vec<_> = stream.into_iter().collect();
        let mut attrs = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            if matches!(&tokens[i], TokenTree::Punct(p) if p.as_char() == '#')
                && let Some(TokenTree::Group(group)) = tokens.get(i + 1)
                && group.delimiter() == Delimiter::Bracket
            {
                attrs.push(group.stream().to_string().split_whitespace().collect::<String>());
                i += 2;
                continue;
            }
            if word(&tokens[i], "pub") {
                i += 1;
                if matches!(tokens.get(i), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis)
                {
                    i += 1;
                }
                continue;
            }
            let kind = ["struct", "enum", "trait", "type", "impl", "mod"]
                .into_iter()
                .find(|kind| word(&tokens[i], kind));
            if let Some(kind) = kind {
                let tail = &tokens[i + 1..];
                // model_json! 的 `struct T(cx) test ...` 是已有模型的投影表，不是类型定义。
                let projection = matches!(tail.get(1), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis)
                    && tail.get(2).is_some_and(|t| word(t, "test"));
                let name = if projection {
                    None
                } else if kind == "impl" {
                    let header: Vec<_> = tail.iter().take_while(|t|
                        !matches!(t, TokenTree::Group(g) if g.delimiter() == Delimiter::Brace)
                    ).collect();
                    if header.iter().any(|t| word(t, "for")) {
                        None // trait impl 的文档由 trait 定义提供，不能新增固有方法。
                    } else {
                        let mut depth = 0;
                        let mut name = String::new();
                        for token in header {
                            match token {
                                TokenTree::Punct(p) if p.as_char() == '<' => {
                                    if !name.is_empty() {
                                        break;
                                    }
                                    depth += 1;
                                }
                                TokenTree::Punct(p) if p.as_char() == '>' => depth -= 1,
                                TokenTree::Ident(id) if depth == 0 => name = id.to_string(),
                                TokenTree::Punct(p) if p.as_char() == '$' && depth == 0 => {
                                    name.push('$')
                                }
                                _ => {}
                            }
                        }
                        // 唯一以元变量为目标的稳定 impl 模板是 bind_export 的 sessions 分支。
                        if tail
                            .first()
                            .is_some_and(|t| matches!(t, TokenTree::Punct(p) if p.as_char() == '$'))
                        {
                            name.insert(0, '$');
                        }
                        (!name.is_empty()).then_some(name)
                    }
                } else {
                    tail.first().and_then(|t| match t {
                        TokenTree::Ident(id) => Some(id.to_string()),
                        _ => None,
                    })
                };
                if let Some(name) = name {
                    result.push(Item {
                        kind,
                        name,
                        audit: attrs.iter().any(|a| a == AUDIT),
                        extensible: attrs.iter().any(|a| a == "non_exhaustive"),
                        attrs: attrs.clone(),
                    });
                }
            }
            if let TokenTree::Group(group) = &tokens[i] {
                work.push(group.stream());
            }
            attrs.clear();
            i += 1;
        }
    }
    result
}

#[test]
fn bind_11_audit_annotations_match_written_contract() {
    let types = section("bind-11-types");
    let names: BTreeSet<_> =
        types.iter().map(|line| line.split_whitespace().nth(1).unwrap()).collect();
    let values = section("bind-11-values");
    assert!(values.is_subset(&types), "值对象豁免必须属于稳定类型");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ancestors = section("bind-11-ancestors");
    let mut checked_ancestors = BTreeSet::new();
    for item in items(&std::fs::read_to_string(root.join("src/lib.rs")).unwrap()) {
        if item.kind == "mod" && ancestors.contains(&item.name) {
            assert_eq!(
                item.attrs,
                [
                    "cfg_attr(not(rsword_api_docs),doc(hidden))",
                    "cfg_attr(rsword_api_docs,allow(missing_docs))",
                ],
                "祖先 {} 的审计可见性漂移",
                item.name
            );
            checked_ancestors.insert(item.name);
        }
    }
    assert_eq!(checked_ancestors, ancestors);
    let mut work = vec![root.join("src")];
    let mut actual = BTreeSet::new();
    let mut annotated = BTreeSet::new();
    let mut declared = BTreeSet::new();
    while let Some(path) = work.pop() {
        if path.is_dir() {
            work.extend(std::fs::read_dir(path).unwrap().map(|e| e.unwrap().path()));
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let file = path.strip_prefix(root).unwrap().to_str().unwrap();
        let mut ordinals = BTreeMap::<String, usize>::new();
        for item in items(&std::fs::read_to_string(&path).unwrap()) {
            let type_key = format!("{file} {}", item.name);
            let stable = if item.kind == "impl" {
                names.contains(item.name.as_str()) || item.name == "$table"
            } else {
                types.contains(&type_key)
            };
            if !stable && !item.audit {
                continue;
            }
            assert!(
                !item.attrs.iter().any(|attr| attr.contains("doc(hidden)")),
                "稳定定义/固有 impl 自身不许隐藏，否则会压掉文档审计：{type_key}"
            );
            let key = if item.kind == "impl" {
                let ordinal = ordinals.entry(item.name.clone()).or_default();
                *ordinal += 1;
                format!("{file} impl:{}:{ordinal}", item.name)
            } else {
                if stable {
                    assert!(declared.insert(type_key.clone()), "重复定义 {type_key}");
                    if matches!(item.kind, "struct" | "enum") {
                        assert!(
                            item.extensible || values.contains(&type_key),
                            "缺少 non_exhaustive 或成文豁免：{type_key}"
                        );
                    }
                }
                format!("{file} type:{}", item.name)
            };
            if stable {
                assert!(actual.insert(key.clone()), "重复位置 {key}");
            }
            if item.audit {
                assert!(annotated.insert(key), "重复注解位置");
            }
        }
    }
    assert_eq!(declared, types, "稳定类型定义与成文清单漂移");
    assert_eq!(actual, annotated, "稳定定义/固有 impl 与审计注解漂移");
    assert_eq!(annotated, section("bind-11-audit"), "审计注解与成文位置清单漂移");
}

#[test]
fn bind_11_audit_scanner_sees_unannotated_impls_and_macro_templates() {
    let source = r#"
        // impl Fake {}
        const TEXT: &str = "impl Fake {}";
        impl crate::EditSession { pub fn undocumented() {} }
        impl Clone for EditSession { fn clone(&self) -> Self { todo!() } }
        macro_rules! generated { () => {
            #[cfg_attr(rsword_api_docs, deny(missing_docs))]
            impl $table { pub fn undocumented() {} }
        } }
    "#;
    let found: Vec<_> = items(source).into_iter().filter(|i| i.kind == "impl").collect();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].name, "EditSession");
    assert!(!found[0].audit);
    assert_eq!(found[1].name, "$table");
    assert!(found[1].audit);
}
