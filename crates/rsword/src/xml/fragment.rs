//! XML 片段 → [`NewElement`]（`EDIT-03` 的 `NewBlock::Xml` / `NewInline::Xml`，`COMPAT-08` 的
//! `rawPPr` / `rawRPr` / `math.omml` / `ruby.xml` / `image.xml`）。
//!
//! 片段用目标 part 根元素的命名空间声明（补上全部已知规范前缀）包一层临时根后解析成临时 DOM，
//! 再按 [`NewElement::from_dom`] 转成与目标 DOM 无关的描述；片段里自带的 `xmlns:*` 声明原样保留为属性，
//! 未声明的前缀成为 `Unbound`（序列化按原样写回，`XML-05`）。

use crate::package::PartId;
use crate::xml::dom::{Dom, NodeKind};
use crate::xml::names::{LocalName, NsId};
use crate::xml::parse::XmlError;
use crate::xml::plan::NewElement;

/// 解析片段（可含多个顶层元素与文本），返回顶层元素列表；顶层文本忽略。
pub fn parse_fragment(target: &mut Dom, xml: &str) -> Result<Vec<NewElement>, XmlError> {
    let (tmp, tops) = parse_fragment_dom(target, xml)?;
    Ok(tops
        .into_iter()
        .filter_map(|c| NewElement::from_dom(&tmp, c, target.interner_mut()))
        .collect())
}

/// 解析片段为临时 DOM（可以先用 `read_*` 读取属性表再转换），返回临时 DOM 与顶层元素节点。
pub fn parse_fragment_dom(
    target: &Dom,
    xml: &str,
) -> Result<(Dom, Vec<crate::xml::NodeId>), XmlError> {
    let wrapped = wrap(target, xml);
    let tmp = Dom::parse(PartId(u32::MAX), wrapped.as_bytes())?;
    let root = tmp.root();
    let tops: Vec<_> = tmp
        .children(root)
        .iter()
        .copied()
        .filter(|&c| matches!(tmp.node(c).kind, NodeKind::Element(_)))
        .collect();
    Ok((tmp, tops))
}

/// 用临时根包住片段：先按目标 flavor 声明全部已知规范前缀，再让目标根自己的声明覆盖同名前缀。
fn wrap(target: &Dom, xml: &str) -> String {
    let flavor = target.flavor();
    let mut decls: Vec<(String, String)> = Vec::new();
    for ns in NsId::KNOWN {
        if matches!(ns, NsId::Xml | NsId::Xmlns) {
            continue;
        }
        if let (Some(p), Some(uri)) = (ns.canonical_prefix(), ns.uri(flavor))
            && !p.is_empty()
        {
            decls.push((p.to_string(), uri.to_string()));
        }
    }
    if let Some(e) = target.element(target.root()) {
        for a in &e.attrs {
            if a.name.ns != NsId::Xmlns || a.name.local == LocalName::Xmlns {
                continue; // 默认命名空间不搬：片段里的无前缀名不该继承主 part 的默认空间
            }
            let prefix = a.name.local.as_str(target.interner()).to_string();
            let uri = target.attr_str(a).into_owned();
            match decls.iter_mut().find(|(p, _)| *p == prefix) {
                Some(d) => d.1 = uri,
                None => decls.push((prefix, uri)),
            }
        }
    }
    let mut s = String::with_capacity(xml.len() + 64 * decls.len());
    s.push_str("<fragment-root");
    for (p, uri) in &decls {
        s.push_str(" xmlns:");
        s.push_str(p);
        s.push_str("=\"");
        s.push_str(uri);
        s.push('"');
    }
    s.push('>');
    s.push_str(xml);
    s.push_str("</fragment-root>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::names::QName;
    use crate::xml::plan::NewNode;

    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    #[test]
    fn edit_03_fragment_to_new_element() {
        let mut dom = Dom::parse(
            PartId(0),
            format!("<w:document xmlns:w=\"{W}\"><w:body/></w:document>").as_bytes(),
        )
        .unwrap();
        let frags = parse_fragment(
            &mut dom,
            "<w:pPr><w:jc w:val=\"center\"/><w:keepNext/></w:pPr><w:r><w:t xml:space=\"preserve\">a &amp; b</w:t></w:r>",
        )
        .unwrap();
        assert_eq!(frags.len(), 2);
        assert_eq!(frags[0].name, QName::w(LocalName::PPr));
        assert_eq!(frags[0].child_elements().count(), 2);
        let jc = frags[0].child_elements().next().unwrap();
        assert_eq!(jc.attrs, vec![(QName::w(LocalName::Val), "center".to_string())]);
        let t = frags[1].child_elements().next().unwrap();
        assert_eq!(t.name, QName::w(LocalName::T));
        assert_eq!(t.attrs[0].0, QName::new(NsId::Xml, LocalName::Space));
        assert_eq!(t.children, vec![NewNode::Text("a & b".to_string())]);
        // m: 前缀不在目标根上声明，靠已知前缀表
        let m = parse_fragment(&mut dom, "<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>").unwrap();
        assert_eq!(m[0].name.ns, NsId::M);
        // 未知前缀 → Unbound，未知局部名 → Other（重新 intern 到目标）
        let u = parse_fragment(&mut dom, "<zz:thing w:foo=\"1\"/>").unwrap();
        assert!(matches!(u[0].name.ns, NsId::Unbound(_)));
        assert!(matches!(u[0].name.local, LocalName::Other(_)));
        let id = u[0].materialize(&mut dom);
        assert_eq!(dom.name(id).unwrap().local.as_str(dom.interner()), "thing");
    }
}
