//! tokenizer → DOM（`XML-01`–`XML-08`）。迭代实现，显式栈，深度上限 [`MAX_DEPTH`]。
//!
//! 自写而不用 `quick-xml`：需要属性名/属性值的字节区间、引号风格、原始限定名、序言/尾声区间、
//! 重复属性容忍与不 trim 的文本，这些 `quick-xml` 不公开（`docs/04` §2.1）。
//! 覆盖的是 OOXML 子集：无 DTD 内部实体声明（`<!DOCTYPE>` 只跳过），CDATA / 注释 / PI 作 `Opaque`。

use std::collections::HashSet;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use memchr::{memchr, memmem};

use crate::diag::{DiagCode, Diagnostic};
use crate::package::PartId;
use crate::xml::dom::{Attr, AttrValue, Dom, Element, Mce, Node, NodeId, NodeKind, TextValue};
use crate::xml::entities;
use crate::xml::interner::{Interned, Interner};
use crate::xml::lex::{Lex, r32};
use crate::xml::names::{LocalName, NsId, QName};
use crate::xml::{Dirty, MAX_DEPTH};

/// part 级解析失败（`XML-08`）。非主 part 降级为 `Opaque`，主 part 整体 `Err`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlError {
    /// `XmlMalformed` 或 `XmlTooDeep`。
    pub code: DiagCode,
    /// 相对（转码后）part 字节的偏移。
    pub offset: u32,
    pub message: String,
}

impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}: {}", self.code, self.offset, self.message)
    }
}

impl std::error::Error for XmlError {}

impl Dom {
    /// 解析一个 XML part。成功即良构（本子集意义下）；诊断在 [`Dom::diagnostics`]。
    pub fn parse(part: PartId, bytes: &[u8]) -> Result<Dom, XmlError> {
        let (src, transcoded, start) = decode_input(bytes)?;
        let mut p = Parser {
            part,
            src: &src,
            b: src.as_bytes(),
            pos: start,
            nodes: Vec::new(),
            interner: Interner::new(),
            diags: Vec::new(),
            ns_stack: Vec::new(),
            ns_frames: Vec::new(),
            stack: Vec::new(),
            unbound_reported: HashSet::new(),
            raw_attrs: Vec::new(),
        };
        if transcoded {
            p.diags.push(Diagnostic::pre_existing(
                part,
                Some(0..0),
                DiagCode::XmlTranscoded,
                "part was not UTF-8; transcoded to UTF-8, byte fidelity is not possible for this part",
            ));
        }
        let (root, prolog, epilog) = p.run()?;
        let Parser { nodes, diags, interner, .. } = p;
        Ok(Dom { part, src, nodes, root, prolog, epilog, transcoded, diagnostics: diags, interner })
    }
}

/// `XML-01`：UTF-8（可带 BOM）直接使用；UTF-16（BOM 或裸 `<\0?\0`）转码。
/// 返回 (字节, 是否转码, 扫描起点)。
fn decode_input(bytes: &[u8]) -> Result<(Arc<str>, bool, usize), XmlError> {
    if bytes.len() > u32::MAX as usize {
        return Err(malformed(0, "part larger than 4 GiB"));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return match std::str::from_utf8(rest) {
            Ok(_) => Ok((Arc::from(std::str::from_utf8(bytes).unwrap_or_default()), false, 3)),
            Err(e) => Err(malformed(3 + e.valid_up_to(), "invalid UTF-8")),
        };
    }
    let utf16 = if bytes.starts_with(&[0xFF, 0xFE]) {
        Some((true, 2))
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        Some((false, 2))
    } else if bytes.len() >= 4 && bytes[0] == b'<' && bytes[1] == 0 && bytes[3] == 0 {
        Some((true, 0))
    } else if bytes.len() >= 4 && bytes[0] == 0 && bytes[1] == b'<' && bytes[2] == 0 {
        Some((false, 0))
    } else {
        None
    };
    if let Some((le, skip)) = utf16 {
        let (pairs, _) = bytes[skip..].as_chunks::<2>();
        let units =
            pairs.iter().map(|c| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) });
        let s: String = char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect();
        return Ok((Arc::from(s), true, 0));
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok((Arc::from(s), false, 0)),
        Err(e) => Err(malformed(e.valid_up_to(), "invalid UTF-8")),
    }
}

fn malformed(offset: usize, message: impl Into<String>) -> XmlError {
    XmlError { code: DiagCode::XmlMalformed, offset: r32(offset), message: message.into() }
}

struct Parser<'a> {
    part: PartId,
    src: &'a str,
    b: &'a [u8],
    pos: usize,
    nodes: Vec<Node>,
    interner: Interner,
    diags: Vec<Diagnostic>,
    /// `(前缀, 命名空间)`；前缀 `None` 表示默认命名空间。内层遮蔽外层。
    ns_stack: Vec<(Option<Interned>, NsId)>,
    /// 每个打开元素进入时 `ns_stack` 的长度。
    ns_frames: Vec<usize>,
    /// 打开的元素。
    stack: Vec<NodeId>,
    unbound_reported: HashSet<Interned>,
    /// 复用的属性缓冲：(名字区间, 值区间, 引号)。
    raw_attrs: Vec<(Range<usize>, Range<usize>, u8)>,
}

#[inline]
fn is_ws(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}

#[inline]
fn is_name_delim(c: u8) -> bool {
    is_ws(c) || matches!(c, b'/' | b'>' | b'<' | b'=' | b'"' | b'\'')
}

impl<'a> Parser<'a> {
    fn run(&mut self) -> Result<(NodeId, Range<u32>, Range<u32>), XmlError> {
        self.skip_prolog()?;
        let prolog = 0..r32(self.pos);
        let root = self.parse_start_tag(None)?;
        while let Some(&top) = self.stack.last() {
            if self.pos >= self.b.len() {
                let open = self.nodes[top.idx()].lex.as_ref().map_or(0, |l| l.range.start as usize);
                return Err(malformed(
                    open,
                    format!("unclosed element <{}>", self.element_lex_name(top)),
                ));
            }
            if self.b[self.pos] == b'<' {
                if self.starts_with(b"</") {
                    self.parse_end_tag(top)?;
                } else if self.starts_with(b"<!--") {
                    self.parse_opaque(top, b"-->", "unterminated comment")?;
                } else if self.starts_with(b"<![CDATA[") {
                    self.parse_opaque(top, b"]]>", "unterminated CDATA section")?;
                } else if self.starts_with(b"<?") {
                    self.parse_opaque(top, b"?>", "unterminated processing instruction")?;
                } else if self.starts_with(b"<!") {
                    return Err(malformed(
                        self.pos,
                        "unexpected markup declaration inside root element",
                    ));
                } else {
                    self.parse_start_tag(Some(top))?;
                }
            } else {
                self.parse_text(top)?;
            }
        }
        let root_end = self.pos;
        self.skip_epilog()?;
        Ok((root, prolog, r32(root_end)..r32(self.b.len())))
    }

    #[inline]
    fn starts_with(&self, pat: &[u8]) -> bool {
        self.b[self.pos..].starts_with(pat)
    }

    fn skip_ws(&mut self) {
        while self.pos < self.b.len() && is_ws(self.b[self.pos]) {
            self.pos += 1;
        }
    }

    fn skip_to(&mut self, marker: &[u8], what: &str) -> Result<(), XmlError> {
        match memmem::find(&self.b[self.pos..], marker) {
            Some(rel) => {
                self.pos += rel + marker.len();
                Ok(())
            }
            None => Err(malformed(self.pos, what)),
        }
    }

    fn skip_prolog(&mut self) -> Result<(), XmlError> {
        loop {
            self.skip_ws();
            if self.pos >= self.b.len() {
                return Err(malformed(self.pos, "no root element"));
            }
            if self.b[self.pos] != b'<' {
                return Err(malformed(self.pos, "text before root element"));
            }
            if self.starts_with(b"<?") {
                self.skip_to(b"?>", "unterminated processing instruction")?;
            } else if self.starts_with(b"<!--") {
                self.skip_to(b"-->", "unterminated comment")?;
            } else if self.starts_with(b"<!DOCTYPE") {
                self.skip_doctype()?;
            } else if self.starts_with(b"<!") || self.starts_with(b"</") {
                return Err(malformed(self.pos, "unexpected markup before root element"));
            } else {
                return Ok(());
            }
        }
    }

    fn skip_doctype(&mut self) -> Result<(), XmlError> {
        let start = self.pos;
        self.pos += b"<!DOCTYPE".len();
        let mut depth = 0i32;
        while self.pos < self.b.len() {
            match self.b[self.pos] {
                q @ (b'"' | b'\'') => {
                    self.pos += 1;
                    match memchr(q, &self.b[self.pos..]) {
                        Some(rel) => self.pos += rel + 1,
                        None => return Err(malformed(start, "unterminated DOCTYPE")),
                    }
                }
                b'[' => {
                    depth += 1;
                    self.pos += 1;
                }
                b']' => {
                    depth -= 1;
                    self.pos += 1;
                }
                b'>' if depth <= 0 => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => self.pos += 1,
            }
        }
        Err(malformed(start, "unterminated DOCTYPE"))
    }

    fn skip_epilog(&mut self) -> Result<(), XmlError> {
        loop {
            self.skip_ws();
            if self.pos >= self.b.len() {
                return Ok(());
            }
            if self.starts_with(b"<?") {
                self.skip_to(b"?>", "unterminated processing instruction")?;
            } else if self.starts_with(b"<!--") {
                self.skip_to(b"-->", "unterminated comment")?;
            } else {
                return Err(malformed(self.pos, "content after root element"));
            }
        }
    }

    fn scan_name(&mut self) -> Range<usize> {
        let start = self.pos;
        while self.pos < self.b.len() && !is_name_delim(self.b[self.pos]) {
            self.pos += 1;
        }
        start..self.pos
    }

    fn push_node(&mut self, node: Node, parent: Option<NodeId>) -> NodeId {
        let id = NodeId(r32(self.nodes.len()));
        self.nodes.push(node);
        if let Some(p) = parent
            && let NodeKind::Element(e) = &mut self.nodes[p.idx()].kind
        {
            e.children.push(id);
        }
        id
    }

    fn element_lex_name(&self, id: NodeId) -> &'a str {
        match &self.nodes[id.idx()].kind {
            NodeKind::Element(e) => {
                e.lex_name.as_ref().map_or("", |r| &self.src[r.start as usize..r.end as usize])
            }
            _ => "",
        }
    }

    fn diag(&mut self, range: Range<usize>, code: DiagCode, message: impl Into<String>) {
        self.diags.push(Diagnostic::pre_existing(
            self.part,
            Some(r32(range.start)..r32(range.end)),
            code,
            message,
        ));
    }

    fn parse_start_tag(&mut self, parent: Option<NodeId>) -> Result<NodeId, XmlError> {
        let start = self.pos;
        self.pos += 1; // '<'
        let name = self.scan_name();
        if name.is_empty() {
            return Err(malformed(start, "empty element name"));
        }
        self.raw_attrs.clear();
        let self_closing = loop {
            self.skip_ws();
            if self.pos >= self.b.len() {
                return Err(malformed(
                    start,
                    format!("unterminated start tag <{}>", &self.src[name.clone()]),
                ));
            }
            match self.b[self.pos] {
                b'/' => {
                    if self.b.get(self.pos + 1) == Some(&b'>') {
                        self.pos += 2;
                        break true;
                    }
                    return Err(malformed(self.pos, "expected '/>'"));
                }
                b'>' => {
                    self.pos += 1;
                    break false;
                }
                _ => {
                    let an = self.scan_name();
                    if an.is_empty() {
                        return Err(malformed(self.pos, "malformed attribute"));
                    }
                    self.skip_ws();
                    if self.b.get(self.pos) != Some(&b'=') {
                        return Err(malformed(
                            self.pos,
                            format!("expected '=' after attribute {}", &self.src[an]),
                        ));
                    }
                    self.pos += 1;
                    self.skip_ws();
                    let q = match self.b.get(self.pos) {
                        Some(&q @ (b'"' | b'\'')) => q,
                        _ => return Err(malformed(self.pos, "attribute value must be quoted")),
                    };
                    self.pos += 1;
                    let v_start = self.pos;
                    let Some(rel) = memchr(q, &self.b[self.pos..]) else {
                        return Err(malformed(v_start, "unterminated attribute value"));
                    };
                    let v_end = self.pos + rel;
                    self.pos = v_end + 1;
                    self.raw_attrs.push((an, v_start..v_end, q));
                }
            }
        };
        let open_end = self.pos;

        if self.stack.len() >= MAX_DEPTH as usize {
            return Err(XmlError {
                code: DiagCode::XmlTooDeep,
                offset: r32(start),
                message: format!("element nesting deeper than {MAX_DEPTH}"),
            });
        }

        // 命名空间声明先入栈（元素自身的声明对自身有效）
        self.ns_frames.push(self.ns_stack.len());
        let raw_attrs = std::mem::take(&mut self.raw_attrs);
        for (an, v, _) in &raw_attrs {
            let an_s = &self.src[an.clone()];
            if an_s == "xmlns" {
                self.declare_ns(None, v.clone());
            } else if let Some(prefix) = an_s.strip_prefix("xmlns:") {
                let p = self.interner.intern(prefix);
                self.declare_ns(Some(p), v.clone());
            }
        }

        let qname = self.resolve_element_name(name.clone())?;
        let mut attrs = Vec::with_capacity(raw_attrs.len());
        for (an, v, q) in &raw_attrs {
            let aname = self.resolve_attr_name(an.clone())?;
            attrs.push(Attr {
                name: aname,
                lex_name: Some(r32(an.start)..r32(an.end)),
                value: AttrValue::Raw(r32(v.start)..r32(v.end)),
                quote: *q,
            });
        }
        for i in 1..attrs.len() {
            if attrs[..i].iter().any(|a| a.name == attrs[i].name) {
                let (an, ..) = &raw_attrs[i];
                self.diag(
                    an.clone(),
                    DiagCode::XmlDupAttr,
                    format!("duplicate attribute {}", &self.src[an.clone()]),
                );
            }
        }
        for (_, v, _) in &raw_attrs {
            if let Some(bad) = entities::first_bad(&self.src[v.clone()]) {
                let at = v.start + bad.offset;
                self.diag(
                    at..at + bad.raw.len(),
                    DiagCode::XmlBadEntity,
                    format!("bad entity {}", bad.raw),
                );
            }
        }
        self.raw_attrs = raw_attrs;

        let lex = Lex {
            range: r32(start)..r32(open_end),
            open: r32(start)..r32(open_end),
            close: r32(open_end)..r32(open_end),
        };
        let node = Node {
            kind: NodeKind::Element(Element {
                name: qname,
                lex_name: Some(r32(name.start)..r32(name.end)),
                attrs,
                children: Vec::new(),
                mce: Mce::default(),
            }),
            parent,
            lex: Some(lex),
            dirty: Dirty::Clean,
        };
        let id = self.push_node(node, parent);
        if self_closing {
            self.pop_ns_frame();
        } else {
            self.stack.push(id);
        }
        Ok(id)
    }

    fn declare_ns(&mut self, prefix: Option<Interned>, value: Range<usize>) {
        let uri = entities::decode(&self.src[value]);
        let ns =
            if uri.is_empty() { NsId::None } else { NsId::intern_uri(&uri, &mut self.interner).0 };
        self.ns_stack.push((prefix, ns));
    }

    fn pop_ns_frame(&mut self) {
        if let Some(len) = self.ns_frames.pop() {
            self.ns_stack.truncate(len);
        }
    }

    fn lookup_prefix(&self, prefix: Option<Interned>) -> Option<NsId> {
        self.ns_stack.iter().rev().find(|(p, _)| *p == prefix).map(|(_, ns)| *ns)
    }

    fn resolve_prefix(&mut self, prefix: &str, at: Range<usize>) -> NsId {
        if prefix == "xml" {
            return NsId::Xml;
        }
        let p = self.interner.intern(prefix);
        match self.lookup_prefix(Some(p)) {
            Some(ns) => ns,
            None => {
                if self.unbound_reported.insert(p) {
                    self.diag(
                        at,
                        DiagCode::XmlUnboundPrefix,
                        format!("prefix `{prefix}` is not bound"),
                    );
                }
                NsId::Unbound(p)
            }
        }
    }

    fn resolve_element_name(&mut self, name: Range<usize>) -> Result<QName, XmlError> {
        let s = &self.src[name.clone()];
        match s.find(':') {
            None => {
                let ns = self.lookup_prefix(None).unwrap_or(NsId::None);
                Ok(QName::new(ns, LocalName::intern(s, &mut self.interner)))
            }
            Some(i) => {
                let (prefix, local) = (&s[..i], &s[i + 1..]);
                if prefix.is_empty() || local.is_empty() || local.contains(':') {
                    return Err(malformed(name.start, format!("bad qualified name `{s}`")));
                }
                let ns = self.resolve_prefix(prefix, name.start..name.start + i);
                Ok(QName::new(ns, LocalName::intern(local, &mut self.interner)))
            }
        }
    }

    fn resolve_attr_name(&mut self, name: Range<usize>) -> Result<QName, XmlError> {
        let s = &self.src[name.clone()];
        if s == "xmlns" {
            return Ok(QName::new(NsId::Xmlns, LocalName::Xmlns));
        }
        match s.find(':') {
            None => Ok(QName::new(NsId::None, LocalName::intern(s, &mut self.interner))),
            Some(i) => {
                let (prefix, local) = (&s[..i], &s[i + 1..]);
                if prefix.is_empty() || local.is_empty() || local.contains(':') {
                    return Err(malformed(
                        name.start,
                        format!("bad qualified attribute name `{s}`"),
                    ));
                }
                if prefix == "xmlns" {
                    return Ok(QName::new(
                        NsId::Xmlns,
                        LocalName::intern(local, &mut self.interner),
                    ));
                }
                let ns = self.resolve_prefix(prefix, name.start..name.start + i);
                Ok(QName::new(ns, LocalName::intern(local, &mut self.interner)))
            }
        }
    }

    fn parse_end_tag(&mut self, top: NodeId) -> Result<(), XmlError> {
        let start = self.pos;
        self.pos += 2; // "</"
        let name = self.scan_name();
        self.skip_ws();
        if self.b.get(self.pos) != Some(&b'>') {
            return Err(malformed(start, "malformed end tag"));
        }
        self.pos += 1;
        let open_name = self.element_lex_name(top);
        let close_name = &self.src[name.clone()];
        if open_name != close_name {
            return Err(malformed(
                start,
                format!("mismatched end tag </{close_name}> for <{open_name}>"),
            ));
        }
        if let Some(lex) = &mut self.nodes[top.idx()].lex {
            lex.close = r32(start)..r32(self.pos);
            lex.range.end = r32(self.pos);
        }
        self.stack.pop();
        self.pop_ns_frame();
        Ok(())
    }

    fn parse_opaque(&mut self, parent: NodeId, end: &[u8], what: &str) -> Result<(), XmlError> {
        let start = self.pos;
        self.skip_to(end, what).map_err(|e| malformed(start, e.message))?;
        let node = Node {
            kind: NodeKind::Opaque,
            parent: Some(parent),
            lex: Some(Lex::leaf(r32(start)..r32(self.pos))),
            dirty: Dirty::Clean,
        };
        self.push_node(node, Some(parent));
        Ok(())
    }

    fn parse_text(&mut self, parent: NodeId) -> Result<(), XmlError> {
        let start = self.pos;
        let Some(rel) = memchr(b'<', &self.b[self.pos..]) else {
            return Err(malformed(
                start,
                format!("unclosed element <{}>", self.element_lex_name(parent)),
            ));
        };
        self.pos += rel;
        if let Some(bad) = entities::first_bad(&self.src[start..self.pos]) {
            let at = start + bad.offset;
            self.diag(
                at..at + bad.raw.len(),
                DiagCode::XmlBadEntity,
                format!("bad entity {}", bad.raw),
            );
        }
        let node = Node {
            kind: NodeKind::Text(TextValue::Raw(r32(start)..r32(self.pos))),
            parent: Some(parent),
            lex: Some(Lex::leaf(r32(start)..r32(self.pos))),
            dirty: Dirty::Clean,
        };
        self.push_node(node, Some(parent));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::serialize;
    use crate::xml::names::LocalName;

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    fn parse(s: &str) -> Dom {
        Dom::parse(PartId(0), s.as_bytes()).unwrap_or_else(|e| panic!("parse failed: {e}\n{s}"))
    }

    fn codes(dom: &Dom) -> Vec<DiagCode> {
        dom.diagnostics().iter().map(|d| d.code).collect()
    }

    #[test]
    fn xml_01_prolog_epilog_and_bom() {
        let src = "\u{FEFF}<?xml version=\"1.0\"?>\n<!-- c -->\n<a/>\n<!-- tail -->\n";
        let dom = parse(src);
        assert_eq!(dom.lex_str(&dom.prolog()), "\u{FEFF}<?xml version=\"1.0\"?>\n<!-- c -->\n");
        assert_eq!(dom.lex_str(&dom.epilog()), "\n<!-- tail -->\n");
        assert!(!dom.transcoded());
        assert_eq!(serialize(&dom).unwrap(), src.as_bytes());
    }

    #[test]
    fn xml_01_utf16_is_transcoded_with_diagnostic() {
        let text = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:style w:styleId=\"N\"/></w:styles>";
        let mut bytes = vec![0xFF, 0xFE];
        for u in text.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let dom = Dom::parse(PartId(3), &bytes).unwrap();
        assert!(dom.transcoded());
        assert_eq!(codes(&dom), vec![DiagCode::XmlTranscoded]);
        assert_eq!(dom.src(), text);
        assert_eq!(dom.name(dom.root()), Some(QName::w(LocalName::Styles)));
        // 裸 UTF-16LE（无 BOM）
        let dom2 = Dom::parse(PartId(3), &bytes[2..]).unwrap();
        assert!(dom2.transcoded());
        assert_eq!(dom2.src(), text);
    }

    #[test]
    fn xml_03_lex_ranges_and_whitespace_text_nodes() {
        let src = "<r:a xmlns:r=\"urn:x\">\n  <r:b k=\"v\"/>text<r:c></r:c>\n</r:a>";
        let dom = parse(src);
        let root = dom.root();
        let lex = dom.node(root).lex.clone().unwrap();
        assert_eq!(dom.lex_str(&lex.open), "<r:a xmlns:r=\"urn:x\">");
        assert_eq!(dom.lex_str(&lex.close), "</r:a>");
        assert_eq!(lex.range, 0..r32(src.len()));
        let kids = dom.children(root).to_vec();
        assert_eq!(kids.len(), 5, "ws, b, text, c, ws");
        assert_eq!(dom.text(kids[0]).as_deref(), Some("\n  "));
        let b = dom.node(kids[1]).lex.clone().unwrap();
        assert!(b.is_self_closing());
        assert_eq!(dom.lex_str(&b.range), "<r:b k=\"v\"/>");
        assert_eq!(dom.text(kids[2]).as_deref(), Some("text"));
        let c = dom.node(kids[3]).lex.clone().unwrap();
        assert_eq!(dom.lex_str(&c.open), "<r:c>");
        assert_eq!(dom.lex_str(&c.close), "</r:c>");
        // 子区间有序、不重叠、落在父内容区间内
        let mut prev = lex.content().start;
        for k in &kids {
            let kl = dom.node(*k).lex.clone().unwrap();
            assert!(kl.range.start >= prev && kl.range.end <= lex.content().end);
            prev = kl.range.end;
        }
        assert_eq!(serialize(&dom).unwrap(), src.as_bytes());
    }

    #[test]
    fn xml_04_quotes_gt_and_duplicate_attrs() {
        let src = "<x val='1' b=\"x>y\" val=\"2\" xmlns:w='urn:w'/>";
        let dom = parse(src);
        let e = dom.element(dom.root()).unwrap();
        assert_eq!(e.attrs.len(), 4, "duplicates are preserved");
        assert_eq!(e.attrs[0].quote, b'\'');
        assert_eq!(e.attrs[1].quote, b'"');
        assert_eq!(dom.attr_str(&e.attrs[1]), "x>y");
        let a = QName::new(NsId::None, LocalName::Val);
        assert_eq!(
            dom.attr_value(dom.root(), a).as_deref(),
            Some("1"),
            "semantic read takes the first"
        );
        assert_eq!(codes(&dom), vec![DiagCode::XmlDupAttr]);
        // xmlns 声明也是 Attr（NsId::Xmlns）
        assert_eq!(e.attrs[3].name.ns, NsId::Xmlns);
        assert_eq!(e.attrs[3].name.local, LocalName::W);
        assert_eq!(serialize(&dom).unwrap(), src.as_bytes());
    }

    #[test]
    fn xml_05_prefix_resolution_by_scope() {
        let src = format!(
            "<x:document xmlns:x=\"{W}\" xmlns=\"urn:d\"><x:body><p xml:space=\"preserve\" q:z=\"1\"><x:p/></p></x:body></x:document>"
        );
        let dom = parse(&src);
        let root = dom.root();
        assert_eq!(
            dom.name(root),
            Some(QName::w(LocalName::Document)),
            "non-w prefix bound to W URI"
        );
        let body = dom.children(root)[0];
        assert_eq!(dom.name(body), Some(QName::w(LocalName::Body)));
        let p = dom.children(body)[0];
        let pname = dom.name(p).unwrap();
        assert!(
            matches!(pname.ns, NsId::Other(_)),
            "unprefixed element takes the default namespace"
        );
        assert_eq!(pname.local, LocalName::P);
        let e = dom.element(p).unwrap();
        assert_eq!(e.attrs[0].name, QName::new(NsId::Xml, LocalName::Space));
        assert!(matches!(e.attrs[1].name.ns, NsId::Unbound(_)));
        assert_eq!(codes(&dom), vec![DiagCode::XmlUnboundPrefix]);
        assert_eq!(dom.name(dom.children(p)[0]), Some(QName::w(LocalName::P)));
        assert_eq!(dom.lex_name(p), Some("p"));
        assert_eq!(dom.lex_name(root), Some("x:document"));
    }

    #[test]
    fn xml_05_default_namespace_undeclared_and_shadowed() {
        let src = "<a xmlns=\"urn:one\" xmlns:p=\"urn:one\"><b xmlns=\"\"><c/></b><p:d xmlns:p=\"urn:two\"/><p:e/></a>";
        let dom = parse(src);
        let root = dom.root();
        let a_ns = dom.name(root).unwrap().ns;
        let b = dom.children(root)[0];
        assert_eq!(dom.name(b).unwrap().ns, NsId::None);
        assert_eq!(dom.name(dom.children(b)[0]).unwrap().ns, NsId::None);
        let d = dom.children(root)[1];
        let e = dom.children(root)[2];
        assert_ne!(dom.name(d).unwrap().ns, a_ns, "inner xmlns:p shadows outer");
        assert_eq!(dom.name(e).unwrap().ns, a_ns, "shadowing ends with the element");
        assert!(codes(&dom).is_empty());
    }

    #[test]
    fn xml_06_entities_decode_once_and_bad_entity_diag() {
        let src = "<t val=\"&amp;lt;\">a&amp;lt;b &bogus; &#x1F600;</t>";
        let dom = parse(src);
        let root = dom.root();
        let text = dom.children(root)[0];
        assert_eq!(dom.text(text).as_deref(), Some("a&lt;b &bogus; 😀"));
        assert_eq!(
            dom.attr_value(root, QName::new(NsId::None, LocalName::Val)).as_deref(),
            Some("&lt;")
        );
        let d = &dom.diagnostics()[0];
        assert_eq!(d.code, DiagCode::XmlBadEntity);
        assert_eq!(dom.lex_str(d.range.as_ref().unwrap()), "&bogus;");
        assert_eq!(serialize(&dom).unwrap(), src.as_bytes());
    }

    #[test]
    fn xml_07_text_is_not_trimmed() {
        let dom = parse("<w:t xmlns:w=\"urn:w\">  x  </w:t>");
        assert_eq!(dom.text(dom.children(dom.root())[0]).as_deref(), Some("  x  "));
    }

    #[test]
    fn xml_08_deep_nesting_parses_iteratively() {
        for (depth, tag) in [(3000usize, "w:smartTag"), (5000, "w:tbl")] {
            let mut s = String::from("<w:document xmlns:w=\"urn:w\">");
            for _ in 0..depth {
                s.push('<');
                s.push_str(tag);
                s.push('>');
            }
            s.push_str("<w:t>deep</w:t>");
            for _ in 0..depth {
                s.push_str("</");
                s.push_str(tag);
                s.push('>');
            }
            s.push_str("</w:document>");
            let dom = parse(&s);
            assert_eq!(dom.node_count(), depth + 3);
            assert_eq!(serialize(&dom).unwrap(), s.as_bytes());
            assert_eq!(dom.descendants(dom.root()).count(), depth + 3);
        }
    }

    #[test]
    fn xml_08_depth_limit() {
        let deep = |n: usize| format!("{}{}", "<a>".repeat(n), "</a>".repeat(n));
        assert!(Dom::parse(PartId(0), deep(MAX_DEPTH as usize).as_bytes()).is_ok());
        let err = Dom::parse(PartId(0), deep(MAX_DEPTH as usize + 1).as_bytes()).unwrap_err();
        assert_eq!(err.code, DiagCode::XmlTooDeep);
    }

    #[test]
    fn xml_08_malformed_inputs_are_errors() {
        let cases: &[(&str, &str)] = &[
            ("<a><b></a>", "mismatched end tag"),
            ("<a>", "unclosed element"),
            ("<a></a><b/>", "content after root"),
            ("junk<a/>", "text before root"),
            ("<a x></a>", "expected '='"),
            ("<a x=1></a>", "must be quoted"),
            ("<a x=\"1></a>", "unterminated attribute"),
            ("<a><!-- x</a>", "unterminated comment"),
            ("", "no root element"),
            ("<a>&#x1;</a><", "content after root"),
            ("<a:></a:>", "bad qualified name"),
        ];
        for (src, msg) in cases {
            let err = Dom::parse(PartId(0), src.as_bytes()).unwrap_err();
            assert_eq!(err.code, DiagCode::XmlMalformed, "{src}");
            assert!(err.message.contains(msg), "{src}: got {}", err.message);
        }
        let err =
            Dom::parse(PartId(0), &[b'<', b'a', b'>', 0xFF, b'<', b'/', b'a', b'>']).unwrap_err();
        assert!(err.message.contains("UTF-8"));
    }

    #[test]
    fn xml_03_opaque_nodes_and_doctype_prolog() {
        let src =
            "<!DOCTYPE x [ <!ENTITY e \"v\"> ]><a><!-- c --><?pi data?><![CDATA[<raw>]]>t</a>";
        let dom = parse(src);
        let kids = dom.children(dom.root());
        assert_eq!(kids.len(), 4);
        for k in &kids[..3] {
            assert!(matches!(dom.node(*k).kind, NodeKind::Opaque));
        }
        assert_eq!(dom.lex_str(&dom.node(kids[2]).lex.clone().unwrap().range), "<![CDATA[<raw>]]>");
        assert_eq!(dom.text(kids[3]).as_deref(), Some("t"));
        assert_eq!(serialize(&dom).unwrap(), src.as_bytes());
    }

    #[test]
    fn xml_13_owned_text_and_attr_are_escaped_and_dirty_states_serialize() {
        let src = "<a x=\"1\"><b>old</b><c/></a>";
        let mut dom = parse(src);
        let root = dom.root();
        let b = dom.children(root)[0];
        let text = dom.children(b)[0];
        dom.node_mut(text).kind = NodeKind::Text(TextValue::Owned("new<&>".into()));
        dom.node_mut(text).dirty = Dirty::SelfDirty;
        dom.node_mut(b).dirty = Dirty::DescendantDirty;
        dom.node_mut(root).dirty = Dirty::DescendantDirty;
        assert_eq!(serialize(&dom).unwrap(), b"<a x=\"1\"><b>new&lt;&amp;&gt;</b><c/></a>");
        // SelfDirty 元素：重建开标签，属性原序原引号；Owned 属性值转义
        if let NodeKind::Element(e) = &mut dom.node_mut(root).kind {
            e.attrs[0].value = AttrValue::Owned("q\"q".into());
        }
        dom.node_mut(root).dirty = Dirty::SelfDirty;
        assert_eq!(serialize(&dom).unwrap(), b"<a x=\"q&quot;q\"><b>new&lt;&amp;&gt;</b><c/></a>");
        // Deleted 子节点不输出；全部删除后 SelfDirty 元素自闭合
        let c = dom.children(root)[1];
        dom.node_mut(c).dirty = Dirty::Deleted;
        dom.node_mut(b).dirty = Dirty::Deleted;
        assert_eq!(serialize(&dom).unwrap(), b"<a x=\"q&quot;q\"/>");
    }
}
