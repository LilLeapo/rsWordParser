//! `BIND-03` 字段 codec。DOM 句柄只在此边界解析，错误带字段路径。
use crate::semantic::props::*;
use crate::xml::{Dom, LocalName, NewElement, NewNode, NsId, QName};
use ::serde::{Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;
use std::marker::PhantomData;

#[derive(Debug, thiserror::Error)]
/// 上下文化转换失败；不可表达的属性必须显式拒绝，不丢弃未知 XML。
#[non_exhaustive]
#[cfg_attr(rsword_api_docs, deny(missing_docs))]
pub enum EditJsonError {
    /// 输入形状或 XML 片段不合法。
    #[error("BIND_BAD_ARGUMENT: {0}")]
    BadArgument(String),
    /// 反向审计转换不能无损表达该引擎值。
    #[error("BIND_EDIT_UNREPRESENTABLE: {0}")]
    Unrepresentable(String),
}
pub type Result<T> = std::result::Result<T, EditJsonError>;
pub struct DecodeCx<'a> {
    pub dom: &'a mut Dom,
    pub escapes: Vec<String>,
}
pub trait Codec {
    type Engine;
    type Wire: Serialize + DeserializeOwned + Clone;
    fn encode(v: &Self::Engine, dom: &Dom) -> Result<Self::Wire>;
    fn decode(v: &Self::Wire, cx: &mut DecodeCx<'_>) -> Result<Self::Engine>;
}
pub struct Shared<T>(PhantomData<T>);
impl<T: Serialize + DeserializeOwned + Clone> Codec for Shared<T> {
    type Engine = T;
    type Wire = T;
    fn encode(v: &T, _dom: &Dom) -> Result<T> {
        Ok(v.clone())
    }
    fn decode(v: &T, _cx: &mut DecodeCx<'_>) -> Result<T> {
        Ok(v.clone())
    }
}
pub struct Optional<C>(PhantomData<C>);
impl<C: Codec> Codec for Optional<C> {
    type Engine = Option<C::Engine>;
    type Wire = Option<C::Wire>;
    fn encode(v: &Self::Engine, dom: &Dom) -> Result<Self::Wire> {
        v.as_ref().map(|v| C::encode(v, dom)).transpose()
    }
    fn decode(v: &Self::Wire, cx: &mut DecodeCx<'_>) -> Result<Self::Engine> {
        v.as_ref().map(|v| C::decode(v, cx)).transpose()
    }
}
pub struct List<C>(PhantomData<C>);
impl<C: Codec> Codec for List<C> {
    type Engine = Vec<C::Engine>;
    type Wire = Vec<C::Wire>;
    fn encode(v: &Self::Engine, dom: &Dom) -> Result<Self::Wire> {
        v.iter().map(|v| C::encode(v, dom)).collect()
    }
    fn decode(v: &Self::Wire, cx: &mut DecodeCx<'_>) -> Result<Self::Engine> {
        v.iter().map(|v| C::decode(v, cx)).collect()
    }
}

pub struct XmlElement;
impl Codec for XmlElement {
    type Engine = NewElement;
    type Wire = String;
    fn encode(v: &NewElement, dom: &Dom) -> Result<String> {
        render(v, dom)
    }
    fn decode(v: &String, cx: &mut DecodeCx<'_>) -> Result<NewElement> {
        let node = parse_one(v, cx.dom)?;
        cx.escapes.push("XML element".to_owned());
        Ok(node)
    }
}
pub fn parse_one(xml: &str, dom: &mut Dom) -> Result<NewElement> {
    use crate::xml::NodeKind;
    let (tmp, tops) = crate::xml::parse_fragment_dom(dom, xml)
        .map_err(|e| EditJsonError::BadArgument(e.to_string()))?;
    if tops.len() != 1 || tmp.children(tmp.root()).len() != 1 {
        return Err(EditJsonError::BadArgument("expected exactly one XML element".into()));
    }
    let mut pending = vec![(tops[0], false)];
    let mut built = BTreeMap::new();
    while let Some((id, visited)) = pending.pop() {
        let e = tmp.element(id).expect("element work item");
        if !visited {
            pending.push((id, true));
            for &child in e.children.iter().rev() {
                match tmp.node(child).kind {
                    NodeKind::Element(_) => pending.push((child, false)),
                    NodeKind::Text(_) => {},
                    NodeKind::Opaque => return Err(EditJsonError::BadArgument(
                        "XML comments and processing instructions cannot be represented by NewElement".into(),
                    )),
                }
            }
            continue;
        }
        let mut out =
            NewElement::new(crate::xml::plan::map_qname(&tmp, e.name, dom.interner_mut()));
        for a in &e.attrs {
            out.push_attr(
                crate::xml::plan::map_qname(&tmp, a.name, dom.interner_mut()),
                tmp.attr_str(a).into_owned(),
            );
        }
        for &child in &e.children {
            out.children.push(match &tmp.node(child).kind {
                NodeKind::Element(_) => {
                    NewNode::Element(built.remove(&child).expect("child built"))
                }
                NodeKind::Text(_) => {
                    NewNode::Text(tmp.text(child).expect("text node").into_owned())
                }
                NodeKind::Opaque => unreachable!("rejected before construction"),
            });
        }
        built.insert(id, out);
    }
    Ok(built.remove(&tops[0]).expect("root built"))
}

fn uri(ns: NsId, dom: &Dom) -> Result<String> {
    if let NsId::Other(id) = ns {
        return Ok(dom.interner().resolve(id).to_owned());
    }
    ns.uri(dom.flavor())
        .map(str::to_owned)
        .ok_or_else(|| EditJsonError::Unrepresentable("namespace has no URI".into()))
}
fn name(q: QName, dom: &Dom, bindings: &BTreeMap<String, String>) -> Result<String> {
    let local = q.local.as_str(dom.interner());
    match q.ns {
        NsId::None => return Ok(local.to_owned()),
        NsId::Xml => return Ok(format!("xml:{local}")),
        NsId::Xmlns => {
            return Ok(if q.local == LocalName::Xmlns {
                "xmlns".into()
            } else {
                format!("xmlns:{local}")
            });
        }
        NsId::Unbound(id) => return Ok(format!("{}:{local}", dom.interner().resolve(id))),
        _ => {}
    }
    let target = uri(q.ns, dom)?;
    let prefix = bindings.iter().find(|(p, u)| !p.is_empty() && **u == target).map(|(p, _)| p);
    prefix
        .map(|p| format!("{p}:{local}"))
        .ok_or_else(|| EditJsonError::Unrepresentable(format!("no in-context prefix for {target}")))
}

/// 不走保存序列化器的 xml:space 自动补写；审计往返需要保留 NewElement 的字段原貌。
/// 显式工作栈遍历，不在嵌套内容上递归。
pub fn render(root: &NewElement, dom: &Dom) -> Result<String> {
    let mut bindings = BTreeMap::new();
    for ns in NsId::KNOWN {
        if let (Some(prefix), Some(uri)) = (ns.canonical_prefix(), ns.uri(dom.flavor())) {
            bindings.insert(prefix.to_owned(), uri.to_owned());
        }
    }
    for (prefix, ns) in dom.namespace_scope(dom.root()).effective() {
        if let Some(prefix) = prefix
            && let Ok(uri) = uri(ns, dom)
        {
            bindings.insert(dom.interner().resolve(prefix).to_owned(), uri);
        }
    }
    enum Event<'a> {
        Element(&'a NewElement, BTreeMap<String, String>),
        Text(&'a str),
        Close(String),
    }
    let mut pending = vec![Event::Element(root, bindings)];
    let mut out = Vec::new();
    while let Some(event) = pending.pop() {
        match event {
            Event::Text(t) => {
                valid_xml_chars(t)?;
                crate::xml::entities::escape_text(t, &mut out);
            }
            Event::Close(n) => out.extend_from_slice(format!("</{n}>").as_bytes()),
            Event::Element(e, mut scope) => {
                let NewElement { name: q, attrs, children } = e;
                for (q, value) in attrs {
                    if q.ns == NsId::Xmlns {
                        let prefix = if q.local == LocalName::Xmlns {
                            ""
                        } else {
                            q.local.as_str(dom.interner())
                        };
                        scope.insert(prefix.to_owned(), value.clone());
                    }
                }
                let n = name(*q, dom, &scope)?;
                out.extend_from_slice(format!("<{n}").as_bytes());
                for (q, value) in attrs {
                    valid_xml_chars(value)?;
                    out.extend_from_slice(format!(" {}=\"", name(*q, dom, &scope)?).as_bytes());
                    crate::xml::entities::escape_attr(value, b'"', &mut out);
                    out.push(b'"');
                }
                if children.is_empty() {
                    out.extend_from_slice(b"/>");
                    continue;
                }
                out.push(b'>');
                pending.push(Event::Close(n));
                for child in children.iter().rev() {
                    pending.push(match child {
                        NewNode::Element(e) => Event::Element(e, scope.clone()),
                        NewNode::Text(t) => Event::Text(t),
                    });
                }
            }
        }
    }
    String::from_utf8(out).map_err(|e| EditJsonError::BadArgument(e.to_string()))
}

fn valid_xml_chars(value: &str) -> Result<()> {
    if value.chars().any(|c| !matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')) {
        return Err(EditJsonError::Unrepresentable("illegal XML character".into()));
    }
    Ok(())
}

macro_rules! props_codec {
    ($codec:ident, $props:ty, $patch:ty, $apply:path, $emit:path, $read:path, $diff:path) => {
        pub struct $codec;
        impl Codec for $codec {
            type Engine = NewElement;
            type Wire = $patch;
            fn encode(v: &NewElement, dom: &Dom) -> Result<Self::Wire> {
                let xml = render(v, dom)?;
                let (tmp, tops) = crate::xml::parse_fragment_dom(dom, &xml)
                    .map_err(|e| EditJsonError::BadArgument(e.to_string()))?;
                let props = $read(&tmp, Some(tops[0]), &mut Vec::new());
                let patch = $diff(&<$props>::default(), &props);
                let mut fresh = <$props>::default();
                $apply(&mut fresh, &patch);
                if $emit(&fresh, dom.flavor()) != *v {
                    return Err(EditJsonError::Unrepresentable(stringify!($codec).into()));
                }
                Ok(patch)
            }
            fn decode(v: &Self::Wire, cx: &mut DecodeCx<'_>) -> Result<NewElement> {
                let mut props = <$props>::default();
                $apply(&mut props, v);
                Ok($emit(&props, cx.dom.flavor()))
            }
        }
    };
}
props_codec!(
    ParaPropsCodec,
    ParaProps,
    ParaPropsPatch,
    apply_para_props_patch,
    emit_para_props,
    read_para_props,
    diff_para_props
);
props_codec!(
    RunPropsCodec,
    RunProps,
    RunPropsPatch,
    apply_run_props_patch,
    emit_run_props,
    read_run_props,
    diff_run_props
);

pub struct Binary;
impl Codec for Binary {
    type Engine = Vec<u8>;
    type Wire = String;
    fn encode(v: &Vec<u8>, _dom: &Dom) -> Result<String> {
        let mut out = String::new();
        crate::package::media::base64_into(v, &mut out);
        Ok(out)
    }
    fn decode(v: &String, cx: &mut DecodeCx<'_>) -> Result<Vec<u8>> {
        let bytes = crate::package::media::base64_decode(v)
            .ok_or_else(|| EditJsonError::BadArgument("invalid base64".into()))?;
        if Self::encode(&bytes, cx.dom)? != *v {
            return Err(EditJsonError::BadArgument("noncanonical base64".into()));
        }
        cx.escapes.push("ReplacePartBytes".into());
        Ok(bytes)
    }
}

#[cfg_attr(rsword_api_docs, deny(missing_docs))]
impl EditJsonError {
    /// 在错误信息前附加载荷字段路径。
    pub fn at(self, path: &str) -> Self {
        match self {
            Self::BadArgument(s) => Self::BadArgument(format!("{path}: {s}")),
            Self::Unrepresentable(s) => Self::Unrepresentable(format!("{path}: {s}")),
        }
    }
}
pub struct PartXml;
impl Codec for PartXml {
    type Engine = String;
    type Wire = String;
    fn encode(v: &String, _dom: &Dom) -> Result<String> {
        Ok(v.clone())
    }
    fn decode(v: &String, cx: &mut DecodeCx<'_>) -> Result<String> {
        // part 级合法性由引擎在事务中检查，转换层只记录逃生口使用。
        cx.escapes.push("ReplacePartXml".into());
        Ok(v.clone())
    }
}
