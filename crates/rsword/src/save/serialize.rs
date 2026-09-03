//! 无损序列化（`XML-13`、`XML-14`、`SAVE-03`）。迭代实现，深度不受栈限制。
//!
//! ```text
//! Clean           → 拷 lex.range
//! Deleted         → 空
//! DescendantDirty → 拷 lex.open；子节点逐个；拷 lex.close（原为自闭合而现在有子节点：去掉 "/>" 改写 ">" 与 "</name>"）
//! SelfDirty / New → 重建开标签（属性原序、原引号、lex_name 优先；无原始写法的名字按作用域生成前缀）；子节点逐个；重建闭标签
//! ```
//!
//! `XML-14`：`New` 子树根上先声明子树里所有未绑定的命名空间；其余缺失前缀在首次使用处内联声明。
//! 声明只写进输出，不改 DOM；编辑引擎应在提交时调用 `Dom::declare_for_new_subtree` 让 DOM 也看到它们。
//! `SAVE-03`：`New`/`SelfDirty` 的 `w:t`/`w:delText`/`w:instrText` 一律带 `xml:space="preserve"`。

use std::fmt;
use std::ops::Range;

use crate::package::ns_context::NamespaceContext;
use crate::xml::Dirty;
use crate::xml::dom::{Attr, AttrValue, Dom, Element, NodeId, NodeKind, TextValue};
use crate::xml::entities;
use crate::xml::lex::urange;
use crate::xml::names::{LocalName, NsId, QName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SerializeError {
    /// 名字所在命名空间是未绑定前缀（`NsId::Unbound`），无法为它生成声明。
    UnboundNamespace { node: NodeId },
    /// 非 `New` 节点缺少 `lex`：引擎不变式被破坏。
    MissingLex { node: NodeId },
}

impl fmt::Display for SerializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnboundNamespace { node } => {
                write!(f, "node {} uses an unbound namespace prefix", node.0)
            }
            Self::MissingLex { node } => write!(f, "node {} is not New but has no lex", node.0),
        }
    }
}

impl std::error::Error for SerializeError {}

/// 整个 part：序言 + 根 + 尾声。
pub fn serialize(dom: &Dom) -> Result<Vec<u8>, SerializeError> {
    serialize_with(dom, None)
}

/// 同 [`serialize`]，生成前缀时优先用 part 的惯用前缀（`PKG-09`）。
pub fn serialize_with(
    dom: &Dom,
    ctx: Option<&NamespaceContext>,
) -> Result<Vec<u8>, SerializeError> {
    let src = dom.src_bytes();
    let mut out = Vec::with_capacity(src.len() + 64);
    out.extend_from_slice(&src[urange(&dom.prolog())]);
    let mut w = Writer { dom, ctx, out: &mut out, scope: WScope::default(), gen_counter: 0 };
    w.run(dom.root())?;
    out.extend_from_slice(&src[urange(&dom.epilog())]);
    Ok(out)
}

/// 序列化一棵子树到 `out`（作用域取自其祖先）。
pub fn serialize_subtree(dom: &Dom, root: NodeId, out: &mut Vec<u8>) -> Result<(), SerializeError> {
    let mut w = Writer { dom, ctx: None, out, scope: WScope::default(), gen_counter: 0 };
    if let Some(parent) = dom.parent(root) {
        let scope = dom.namespace_scope(parent);
        for (p, ns) in scope.effective().into_iter().rev() {
            w.scope.push(p.map(|i| dom.interner().resolve(i).to_string()), ns);
        }
    }
    w.run(root)
}

/// 序列化器自己的作用域：前缀用字符串，因为生成的前缀（`ns1`）不在 DOM 的驻留表里。
#[derive(Default)]
struct WScope {
    bindings: Vec<(Option<String>, NsId)>,
}

impl WScope {
    fn push(&mut self, prefix: Option<String>, ns: NsId) {
        self.bindings.push((prefix, ns));
    }

    fn lookup(&self, prefix: Option<&str>) -> Option<NsId> {
        self.bindings.iter().rev().find(|(p, _)| p.as_deref() == prefix).map(|(_, ns)| *ns)
    }

    /// 有效绑定到 `ns` 的前缀；`want_prefixed` 时不接受默认命名空间（属性需要真前缀）。
    fn prefix_for(&self, ns: NsId, want_prefixed: bool) -> Option<Option<&str>> {
        let mut shadowed: Vec<Option<&str>> = Vec::new();
        for (p, n) in self.bindings.iter().rev() {
            let p = p.as_deref();
            if shadowed.contains(&p) {
                continue;
            }
            shadowed.push(p);
            if *n == ns && !(want_prefixed && p.is_none()) {
                return Some(p);
            }
        }
        None
    }
}

enum Close {
    /// 拷原闭标签字节。
    Bytes(Range<u32>),
    /// 写 `</name>`。
    Name(Vec<u8>),
}

enum Step {
    Enter(NodeId),
    Exit { scope_len: usize, close: Close },
}

struct Writer<'a> {
    dom: &'a Dom,
    ctx: Option<&'a NamespaceContext>,
    out: &'a mut Vec<u8>,
    scope: WScope,
    gen_counter: u32,
}

impl Writer<'_> {
    fn run(&mut self, root: NodeId) -> Result<(), SerializeError> {
        let src = self.dom.src_bytes();
        let mut stack = vec![Step::Enter(root)];
        while let Some(step) = stack.pop() {
            match step {
                Step::Exit { scope_len, close } => {
                    self.scope.bindings.truncate(scope_len);
                    match close {
                        Close::Bytes(r) => self.out.extend_from_slice(&src[urange(&r)]),
                        Close::Name(name) => {
                            self.out.extend_from_slice(b"</");
                            self.out.extend_from_slice(&name);
                            self.out.push(b'>');
                        }
                    }
                }
                Step::Enter(id) => {
                    let node = self.dom.node(id);
                    match node.dirty {
                        Dirty::Deleted => {}
                        Dirty::Clean => {
                            let lex =
                                node.lex.as_ref().ok_or(SerializeError::MissingLex { node: id })?;
                            self.out.extend_from_slice(&src[urange(&lex.range)]);
                        }
                        Dirty::DescendantDirty => match &node.kind {
                            NodeKind::Element(e) => {
                                let lex = node
                                    .lex
                                    .as_ref()
                                    .ok_or(SerializeError::MissingLex { node: id })?;
                                let scope_len = self.scope.bindings.len();
                                self.push_element_decls(e);
                                let live = self.has_live_children(e);
                                if lex.is_self_closing() {
                                    if live {
                                        // `<c …/>` → `<c …>` … `</c>`
                                        let open = &src[urange(&lex.open)];
                                        let body = open.strip_suffix(b"/>").unwrap_or(open);
                                        self.out.extend_from_slice(body.trim_ascii_end());
                                        self.out.push(b'>');
                                        let name = self.element_name(id, e)?;
                                        stack.push(Step::Exit {
                                            scope_len,
                                            close: Close::Name(name),
                                        });
                                        stack.extend(
                                            e.children.iter().rev().map(|&c| Step::Enter(c)),
                                        );
                                    } else {
                                        self.out.extend_from_slice(&src[urange(&lex.range)]);
                                        self.scope.bindings.truncate(scope_len);
                                    }
                                } else {
                                    self.out.extend_from_slice(&src[urange(&lex.open)]);
                                    stack.push(Step::Exit {
                                        scope_len,
                                        close: Close::Bytes(lex.close.clone()),
                                    });
                                    stack.extend(e.children.iter().rev().map(|&c| Step::Enter(c)));
                                }
                            }
                            _ => {
                                let lex = node
                                    .lex
                                    .as_ref()
                                    .ok_or(SerializeError::MissingLex { node: id })?;
                                self.out.extend_from_slice(&src[urange(&lex.range)]);
                            }
                        },
                        Dirty::SelfDirty | Dirty::New => match &node.kind {
                            NodeKind::Element(e) => {
                                let scope_len = self.scope.bindings.len();
                                let live = self.has_live_children(e);
                                let name = self.write_open_tag(id, e, live)?;
                                if live {
                                    stack.push(Step::Exit { scope_len, close: Close::Name(name) });
                                    stack.extend(e.children.iter().rev().map(|&c| Step::Enter(c)));
                                } else {
                                    self.scope.bindings.truncate(scope_len);
                                }
                            }
                            NodeKind::Text(TextValue::Raw(r)) => {
                                self.out.extend_from_slice(&src[urange(r)])
                            }
                            NodeKind::Text(TextValue::Owned(s)) => {
                                entities::escape_text(s, self.out)
                            }
                            NodeKind::Opaque => {
                                let lex = node
                                    .lex
                                    .as_ref()
                                    .ok_or(SerializeError::MissingLex { node: id })?;
                                self.out.extend_from_slice(&src[urange(&lex.range)]);
                            }
                        },
                    }
                }
            }
        }
        Ok(())
    }

    fn has_live_children(&self, e: &Element) -> bool {
        e.children.iter().any(|&c| self.dom.node(c).dirty != Dirty::Deleted)
    }

    /// 把元素上的 `xmlns` 属性压入作用域。
    fn push_element_decls(&mut self, e: &Element) {
        for a in &e.attrs {
            if a.name.ns != NsId::Xmlns {
                continue;
            }
            let uri = self.dom.attr_str(a);
            let ns = if uri.is_empty() {
                NsId::None
            } else {
                match NsId::from_uri(&uri) {
                    Some((id, _)) => id,
                    None => match self.dom.interner().get(&uri) {
                        Some(i) => NsId::Other(i),
                        None => continue,
                    },
                }
            };
            let prefix = if a.name.local == LocalName::Xmlns {
                None
            } else {
                Some(a.name.local.as_str(self.dom.interner()).to_string())
            };
            self.scope.push(prefix, ns);
        }
    }

    /// 元素名字节：原始写法，或按作用域生成（可能触发新声明，声明由调用方写出）。
    fn element_name(&mut self, id: NodeId, e: &Element) -> Result<Vec<u8>, SerializeError> {
        if let Some(r) = &e.lex_name {
            return Ok(self.dom.lex_bytes(r).to_vec());
        }
        let local = e.name.local.as_str(self.dom.interner());
        Ok(match self.prefix_for(id, e.name.ns, false)? {
            Some(p) if !p.is_empty() => format!("{p}:{local}").into_bytes(),
            _ => local.as_bytes().to_vec(),
        })
    }

    /// 为 `ns` 找到或分配前缀。返回 `None`（默认命名空间）或 `Some(prefix)`；新分配的前缀记入
    /// `self.pending_decls` 供开标签写出。
    fn prefix_for(
        &mut self,
        id: NodeId,
        ns: NsId,
        want_prefixed: bool,
    ) -> Result<Option<String>, SerializeError> {
        match ns {
            NsId::None => return Ok(None),
            NsId::Xml => return Ok(Some("xml".into())),
            NsId::Unbound(_) => return Err(SerializeError::UnboundNamespace { node: id }),
            _ => {}
        }
        if let Some(p) = self.scope.prefix_for(ns, want_prefixed) {
            return Ok(p.map(str::to_string));
        }
        // 需要新声明
        let preferred = self
            .ctx
            .and_then(|c| c.prefix_for(ns))
            .or_else(|| ns.canonical_prefix())
            .filter(|p| !p.is_empty())
            .map(str::to_string);
        let mut candidate = preferred.unwrap_or_else(|| self.next_generated());
        while self.scope.lookup(Some(&candidate)).is_some_and(|bound| bound != ns) {
            candidate = self.next_generated();
        }
        let uri = self
            .dom
            .namespace_uri(ns, self.dom.flavor())
            .ok_or(SerializeError::UnboundNamespace { node: id })?;
        self.out.extend_from_slice(b" xmlns:");
        self.out.extend_from_slice(candidate.as_bytes());
        self.out.extend_from_slice(b"=\"");
        entities::escape_attr(&uri, b'"', self.out);
        self.out.push(b'"');
        self.scope.push(Some(candidate.clone()), ns);
        Ok(Some(candidate))
    }

    fn next_generated(&mut self) -> String {
        self.gen_counter += 1;
        format!("ns{}", self.gen_counter)
    }

    fn attr_name(&mut self, id: NodeId, a: &Attr) -> Result<Vec<u8>, SerializeError> {
        if let Some(r) = &a.lex_name {
            return Ok(self.dom.lex_bytes(r).to_vec());
        }
        let local = a.name.local.as_str(self.dom.interner());
        Ok(match a.name.ns {
            NsId::None => local.as_bytes().to_vec(),
            NsId::Xmlns => {
                if a.name.local == LocalName::Xmlns {
                    b"xmlns".to_vec()
                } else {
                    format!("xmlns:{local}").into_bytes()
                }
            }
            ns => match self.prefix_for(id, ns, true)? {
                Some(p) => format!("{p}:{local}").into_bytes(),
                None => local.as_bytes().to_vec(),
            },
        })
    }

    /// 写开标签，返回闭标签要用的名字。`XML-14`：`New` 子树根先为整棵子树声明缺失的命名空间。
    fn write_open_tag(
        &mut self,
        id: NodeId,
        e: &Element,
        live: bool,
    ) -> Result<Vec<u8>, SerializeError> {
        self.push_element_decls(e);
        self.out.push(b'<');
        // 元素名（可能就地声明）
        let name_pos = self.out.len();
        let name = self.element_name(id, e)?;
        // element_name 可能已经向 out 写了 " xmlns:…"（在名字之前）——把名字插到前面
        let decl_bytes: Vec<u8> = self.out.drain(name_pos..).collect();
        self.out.extend_from_slice(&name);
        self.out.extend_from_slice(&decl_bytes);
        // 子树根：为后代中无原始写法的名字预先声明
        let is_new_root = self.dom.node(id).dirty == Dirty::New
            && self.dom.parent(id).is_none_or(|p| self.dom.node(p).dirty != Dirty::New);
        if is_new_root {
            for ns in self.dom.namespaces_needing_prefix(id) {
                if ns != e.name.ns {
                    self.prefix_for(id, ns, false)?;
                }
            }
        }
        // 属性
        for a in &e.attrs {
            let name_bytes = self.attr_name(id, a)?;
            self.out.push(b' ');
            self.out.extend_from_slice(&name_bytes);
            self.out.push(b'=');
            self.out.push(a.quote);
            match &a.value {
                AttrValue::Raw(r) => self.out.extend_from_slice(self.dom.lex_bytes(r)),
                AttrValue::Owned(s) => entities::escape_attr(s, a.quote, self.out),
            }
            self.out.push(a.quote);
        }
        // SAVE-03：文本容器一律 preserve
        if e.name.ns == NsId::W
            && matches!(e.name.local, LocalName::T | LocalName::DelText | LocalName::InstrText)
            && !e.attrs.iter().any(|a| a.name == QName::new(NsId::Xml, LocalName::Space))
        {
            self.out.extend_from_slice(b" xml:space=\"preserve\"");
        }
        if live {
            self.out.push(b'>');
        } else {
            self.out.extend_from_slice(b"/>");
        }
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{PartFlavor, PartId};
    use crate::xml::Dirty;

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const W_STRICT: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
    const WPS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";

    fn parse(s: &str) -> Dom {
        Dom::parse(PartId(0), s.as_bytes()).unwrap()
    }

    fn out(dom: &Dom) -> String {
        String::from_utf8(serialize(dom).unwrap()).unwrap()
    }

    #[test]
    fn xml_14_new_subtree_declares_prefix_at_its_root_only() {
        let mut dom = parse(&format!("<w:document xmlns:w=\"{W}\"><w:body/></w:document>"));
        let body = dom.children(dom.root())[0];
        let wsp = dom.new_element(QName::new(NsId::Wps, LocalName::Wsp));
        let inner = dom.new_element(QName::new(NsId::Wps, LocalName::CNvSpPr));
        dom.append_child(wsp, inner);
        let p = dom.new_element(QName::w(LocalName::P));
        dom.append_child(wsp, p);
        dom.append_child(body, wsp);
        assert_eq!(
            out(&dom),
            format!(
                "<w:document xmlns:w=\"{W}\"><w:body><wps:wsp xmlns:wps=\"{WPS}\"><wps:cNvSpPr/><w:p/></wps:wsp></w:body></w:document>"
            )
        );
        assert_eq!(dom.node(dom.root()).dirty, Dirty::DescendantDirty, "part root untouched");
        // DOM 侧显式声明后，作用域能看到它，输出不重复声明
        let decls = dom.declare_for_new_subtree(wsp);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].ns, NsId::Wps);
        let wps_i = dom.interner().get("wps").unwrap();
        assert_eq!(dom.namespace_scope(inner).lookup(Some(wps_i)), Some(NsId::Wps));
        assert_eq!(
            out(&dom),
            format!(
                "<w:document xmlns:w=\"{W}\"><w:body><wps:wsp xmlns:wps=\"{WPS}\"><wps:cNvSpPr/><w:p/></wps:wsp></w:body></w:document>"
            )
        );
        assert!(dom.declare_for_new_subtree(wsp).is_empty(), "idempotent");
    }

    #[test]
    fn xml_14_default_namespace_and_unprefixed_attrs() {
        let ct = "http://schemas.openxmlformats.org/package/2006/content-types";
        let mut dom = parse(&format!(
            "<Types xmlns=\"{ct}\"><Default Extension=\"xml\" ContentType=\"application/xml\"/></Types>"
        ));
        let root = dom.root();
        let ov = dom.new_element(QName::new(NsId::Ct, LocalName::Override));
        dom.set_attr(ov, QName::new(NsId::None, LocalName::PartName), "/word/header1.xml");
        dom.set_attr(ov, QName::new(NsId::None, LocalName::ContentType), "a+xml");
        dom.append_child(root, ov);
        assert_eq!(dom.node(ov).dirty, Dirty::New, "set_attr keeps New");
        assert_eq!(
            out(&dom),
            format!(
                "<Types xmlns=\"{ct}\"><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/header1.xml\" ContentType=\"a+xml\"/></Types>"
            )
        );
    }

    #[test]
    fn xml_14_strict_part_declares_strict_uris() {
        let mut dom = parse(&format!("<w:document xmlns:w=\"{W_STRICT}\"><w:body/></w:document>"));
        assert_eq!(dom.flavor(), PartFlavor::Strict);
        let body = dom.children(dom.root())[0];
        let pic = dom.new_element(QName::new(NsId::Pic, LocalName::Pic));
        dom.append_child(body, pic);
        assert_eq!(
            out(&dom),
            format!(
                "<w:document xmlns:w=\"{W_STRICT}\"><w:body><pic:pic xmlns:pic=\"http://purl.oclc.org/ooxml/drawingml/picture\"/></w:body></w:document>"
            )
        );
    }

    #[test]
    fn save_03_text_containers_get_xml_space_preserve() {
        let mut dom = parse(&format!(
            "<w:p xmlns:w=\"{W}\"><w:r><w:t xml:space=\"preserve\"> a</w:t></w:r></w:p>"
        ));
        let r = dom.children(dom.root())[0];
        let t = dom.children(r)[0];
        // 已有 preserve 的 SelfDirty w:t 不重复
        dom.set_attr(t, QName::new(NsId::None, LocalName::Val), "x");
        assert!(
            out(&dom).contains("<w:t xml:space=\"preserve\" val=\"x\"> a</w:t>"),
            "{}",
            out(&dom)
        );
        // New w:t 自动加
        let r2 = dom.new_element(QName::w(LocalName::R));
        let t2 = dom.new_element(QName::w(LocalName::T));
        let txt = dom.new_text("b<c");
        dom.append_child(t2, txt);
        dom.append_child(r2, t2);
        dom.append_child(dom.root(), r2);
        assert!(
            out(&dom).ends_with("<w:r><w:t xml:space=\"preserve\">b&lt;c</w:t></w:r></w:p>"),
            "{}",
            out(&dom)
        );
        // 空的 New w:t 自闭合
        let t3 = dom.new_element(QName::w(LocalName::DelText));
        dom.append_child(r2, t3);
        assert!(out(&dom).contains("<w:delText xml:space=\"preserve\"/>"));
    }

    #[test]
    fn xml_14_prefix_conflict_generates_a_fresh_prefix() {
        let mut dom = parse("<a xmlns:w=\"urn:other\"><b/></a>");
        let b = dom.children(dom.root())[0];
        let p = dom.new_element(QName::w(LocalName::P));
        dom.set_attr(p, QName::w(LocalName::RsidR), "00A");
        dom.append_child(b, p);
        assert_eq!(
            out(&dom),
            format!(
                "<a xmlns:w=\"urn:other\"><b><ns1:p xmlns:ns1=\"{W}\" ns1:rsidR=\"00A\"/></b></a>"
            )
        );
    }

    #[test]
    fn xml_14_attribute_needs_a_real_prefix_even_if_default_namespace_matches() {
        let mut dom = parse("<a xmlns=\"urn:d\"><b/></a>");
        let b = dom.children(dom.root())[0];
        let d = NsId::Other(dom.interner().get("urn:d").unwrap());
        dom.set_attr(b, QName::new(d, LocalName::X), "1");
        assert_eq!(out(&dom), "<a xmlns=\"urn:d\"><b xmlns:ns1=\"urn:d\" ns1:x=\"1\"/></a>");
        // 元素本身可以用默认命名空间
        let c = dom.new_element(QName::new(d, LocalName::Col));
        dom.append_child(dom.root(), c);
        assert!(out(&dom).ends_with("<col/></a>"));
    }

    #[test]
    fn xml_14_namespace_context_preference_wins_over_canonical() {
        let mut dom = parse(&format!("<w:document xmlns:w=\"{W}\"><w:body/></w:document>"));
        let body = dom.children(dom.root())[0];
        let glow = dom.new_element(QName::new(NsId::W14, LocalName::Glow));
        dom.append_child(body, glow);
        let mut ctx = NamespaceContext::from_dom(&dom, PartFlavor::Transitional);
        ctx.preferred.insert(NsId::W14, "w14x".into());
        let s = String::from_utf8(serialize_with(&dom, Some(&ctx)).unwrap()).unwrap();
        assert!(
            s.contains(
                "<w14x:glow xmlns:w14x=\"http://schemas.microsoft.com/office/word/2010/wordml\"/>"
            ),
            "{s}"
        );
        assert!(out(&dom).contains("<w14:glow xmlns:w14="), "canonical without context");
    }

    #[test]
    fn xml_13_serialize_subtree_uses_ancestor_scope() {
        let mut dom =
            parse(&format!("<w:document xmlns:w=\"{W}\"><w:body><w:p/></w:body></w:document>"));
        let body = dom.children(dom.root())[0];
        let p = dom.children(body)[0];
        let r = dom.new_element(QName::w(LocalName::R));
        dom.append_child(p, r);
        let mut buf = Vec::new();
        serialize_subtree(&dom, p, &mut buf).unwrap();
        assert_eq!(buf, b"<w:p><w:r/></w:p>", "w resolves through the ancestors, no redeclaration");
        let mut buf2 = Vec::new();
        serialize_subtree(&dom, r, &mut buf2).unwrap();
        assert_eq!(buf2, b"<w:r/>");
    }

    #[test]
    fn xml_13_self_closing_parent_gaining_children_is_reopened() {
        let mut dom = parse("<a><b x=\"1\" /><c/></a>");
        let b = dom.children(dom.root())[0];
        let c = dom.children(dom.root())[1];
        let n = dom.new_text("t");
        dom.append_child(b, n);
        assert_eq!(out(&dom), "<a><b x=\"1\">t</b><c/></a>");
        let n2 = dom.new_text("u");
        dom.append_child(c, n2);
        dom.delete(n2);
        assert_eq!(out(&dom), "<a><b x=\"1\">t</b><c/></a>", "no live children: original bytes");
    }

    #[test]
    fn xml_13_unbound_namespace_is_an_error() {
        let dom = parse("<a><q:b/></a>");
        let b = dom.children(dom.root())[0];
        let mut dom2 = dom.clone();
        dom2.rename_element(b, dom.name(b).unwrap());
        assert_eq!(serialize(&dom2).unwrap_err(), SerializeError::UnboundNamespace { node: b });
        assert_eq!(out(&dom), "<a><q:b/></a>", "clean unbound prefix still roundtrips");
    }
}
