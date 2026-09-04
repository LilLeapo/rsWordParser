//! 生成代码调用的读写辅助：诊断上下文、`w:val` 与属性读取、按 flavor 选拼写。

use std::borrow::Cow;

use crate::diag::{DiagCode, Diagnostic};
use crate::package::PartFlavor;
use crate::semantic::props::NewElement;
use crate::semantic::props::codec::Codec;
use crate::xml::dom::{Dom, NodeId};
use crate::xml::names::{LocalName, NsId, QName};

/// `w:val`。
pub const W_VAL: QName = QName::new(NsId::W, LocalName::Val);

/// 读取上下文：DOM、当前元素、诊断收集器。
pub struct Ctx<'a> {
    dom: &'a Dom,
    at: Option<NodeId>,
    diags: &'a mut Vec<Diagnostic>,
}

impl<'a> Ctx<'a> {
    pub fn new(dom: &'a Dom, diags: &'a mut Vec<Diagnostic>) -> Self {
        Self { dom, at: None, diags }
    }

    /// 借出的 DOM 引用不与 `&mut self` 绑定，方便在遍历 DOM 时继续记诊断。
    pub fn dom(&self) -> &'a Dom {
        self.dom
    }

    /// 进入某个元素：之后的诊断挂在它的区间上。
    pub fn enter(&mut self, node: NodeId) {
        self.at = Some(node);
    }

    pub fn current(&self) -> Option<NodeId> {
        self.at
    }

    /// `PROP_BAD_VALUE`：`what` 是属性名（如 `w:val`），`codec` 是期望的类型名。
    pub fn bad_value(&mut self, what: &str, codec: &str, text: &str) {
        let elem = self
            .at
            .and_then(|n| self.dom.name(n))
            .map_or_else(String::new, |q| q.display(self.dom.interner()).to_string());
        let range = self.at.and_then(|n| self.dom.node(n).lex.as_ref()).map(|l| l.range.clone());
        self.diags.push(Diagnostic::pre_existing(
            self.dom.part(),
            range,
            DiagCode::PropBadValue,
            format!("{elem}/@{what}=\"{text}\" 不是合法的 {codec}，按原文保留"),
        ));
    }

    /// 元素缺少必需的属性。
    pub fn missing_value(&mut self, what: &str, codec: &str) {
        let elem = self
            .at
            .and_then(|n| self.dom.name(n))
            .map_or_else(String::new, |q| q.display(self.dom.interner()).to_string());
        let range = self.at.and_then(|n| self.dom.node(n).lex.as_ref()).map(|l| l.range.clone());
        self.diags.push(Diagnostic::pre_existing(
            self.dom.part(),
            range,
            DiagCode::PropBadValue,
            format!("{elem} 缺少 @{what}（{codec}），按空原文保留"),
        ));
    }

    pub fn attr_display(&self, name: QName) -> String {
        name.display(self.dom.interner()).to_string()
    }
}

/// 读单 `w:val` 元素：缺 `w:val` 时由 codec 决定（`OnOff` → `true`，其余 `Raw("")` + 诊断）。
pub fn read_val<C: Codec>(node: NodeId, ctx: &mut Ctx<'_>) -> C::Value {
    let dom = ctx.dom();
    match dom.attr_value(node, W_VAL) {
        Some(text) => C::parse(&text, ctx),
        None => C::missing(ctx),
    }
}

/// 读元素的一个属性；`legacy` 是 Transitional 拼写，两者都不存在时 `None`。
pub fn read_attr<C: Codec>(
    node: NodeId,
    name: QName,
    legacy: Option<QName>,
    ctx: &mut Ctx<'_>,
) -> Option<C::Value> {
    let dom = ctx.dom();
    let attr = dom.attr(node, name).or_else(|| legacy.and_then(|l| dom.attr(node, l)))?;
    let what = ctx.attr_display(attr.name);
    let text = dom.attr_str(attr);
    Some(C::parse_attr(&text, &what, ctx))
}

/// 生成单 `w:val` 元素；`OnOff(true)` 不写属性。
pub fn emit_val<C: Codec>(name: QName, v: &C::Value, flavor: PartFlavor) -> NewElement {
    let mut e = NewElement::new(name);
    if let Some(text) = C::val_attr(v, flavor) {
        e.push_attr(W_VAL, text);
    }
    e
}

/// 按 flavor 选拼写：Transitional 且有 `legacy` 时用 `legacy`，否则用 `strict`。
pub const fn spell(flavor: PartFlavor, strict: QName, legacy: Option<QName>) -> QName {
    match (flavor, legacy) {
        (PartFlavor::Transitional, Some(l)) => l,
        _ => strict,
    }
}

/// `Cow<str>` 的辅助：把 `&'static str` 借出。
pub(crate) fn lit(s: &'static str) -> Cow<'static, str> {
    Cow::Borrowed(s)
}
