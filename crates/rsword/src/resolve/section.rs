//! 节的有效属性视图（`RES-10`，任务 5.2）。
//!
//! 模型只存声明值（`MOD-10`）；这里做两件 Word 的"链接到前一节"语义：
//!
//! 1. **引用继承**：某节没声明某个槽（kind × variant，共六个）时，沿文档序往前找第一个声明过
//!    该槽的节。第一节就没有 → 该槽为空。继承的结果带来源节下标，调用方（页眉页脚投影、
//!    `SetHeaderFooter`）据此判断"这一节是自己有 part 还是跟着前面的"——那正是新建 part 还是
//!    改写 part 的分界。
//! 2. **有效变体选择**：首页且本节 `w:titlePg` → `first`；偶数页且文档 `w:evenAndOddHeaders` →
//!    `even`；否则 `default`。选中的槽为空时**不回退** default：Word 里"首页不同"而没有首页页眉
//!    就是首页没有页眉。
//!
//! 三个变体各自继承（Word 与 TS 都是按 `w:type` 分别继承，不是整组继承）。

use crate::model::section::{HfKind, HfVariant, SectionGeom, SectionInfo};
use crate::resolve::Resolver;

/// 一个槽（kind × variant）的有效值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HfSlot {
    /// 本节没声明，前面也没有。
    Absent,
    /// 本节自己声明的关系 id。
    Declared(String),
    /// 继承自第 `from` 节（Word 的"链接到前一节"）。
    Inherited { from: usize, id: String },
}

impl HfSlot {
    /// 有效的关系 id（声明或继承来的）。
    pub fn rel_id(&self) -> Option<&str> {
        match self {
            HfSlot::Absent => None,
            HfSlot::Declared(id) | HfSlot::Inherited { id, .. } => Some(id),
        }
    }

    /// 是本节自己声明的（`SetHeaderFooter` 据此决定改写还是新建 part）。
    pub fn is_declared(&self) -> bool {
        matches!(self, HfSlot::Declared(_))
    }
}

/// 一个节的有效属性（`RES-10`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveSection {
    pub idx: usize,
    /// 六个槽：`hf[kind][variant]`，下标同 `HfKind::ALL` / `HfVariant::ALL`。
    pub(crate) hf: [[HfSlot; 3]; 2],
    /// 本节的 `w:titlePg`。
    pub title_pg: bool,
    /// 文档级的 `w:evenAndOddHeaders`（`settings.xml`）。
    pub even_and_odd: bool,
    pub geom: SectionGeom,
}

impl EffectiveSection {
    pub fn slot(&self, kind: HfKind, variant: HfVariant) -> &HfSlot {
        &self.hf[kind as usize][variant as usize]
    }

    /// 某种页面实际用哪个槽（`RES-10`）：先按 `titlePg` / `evenAndOddHeaders` 定变体，再取该槽。
    ///
    /// 选中的变体为空时**不回退** default（Word 语义）。
    pub fn for_page(&self, kind: HfKind, first_page: bool, even_page: bool) -> &HfSlot {
        self.slot(kind, self.variant_for_page(first_page, even_page))
    }

    /// 有效变体（不看槽是否为空）。
    pub fn variant_for_page(&self, first_page: bool, even_page: bool) -> HfVariant {
        if first_page && self.title_pg {
            HfVariant::First
        } else if even_page && self.even_and_odd {
            HfVariant::Even
        } else {
            HfVariant::Default
        }
    }
}

impl Resolver<'_> {
    /// 第 `idx` 节的有效属性（`RES-10`）。`idx` 越界返回 `None`。
    pub fn section(&self, sections: &[SectionInfo], idx: usize) -> Option<EffectiveSection> {
        let me = sections.get(idx)?;
        let even_and_odd = self.settings.is_some_and(|s| s.even_and_odd_headers == Some(true));
        let mut hf = [
            [HfSlot::Absent, HfSlot::Absent, HfSlot::Absent],
            [HfSlot::Absent, HfSlot::Absent, HfSlot::Absent],
        ];
        for kind in HfKind::ALL {
            for variant in HfVariant::ALL {
                hf[kind as usize][variant as usize] = inherit(sections, idx, kind, variant);
            }
        }
        Some(EffectiveSection { idx, hf, title_pg: me.title_pg(), even_and_odd, geom: me.geom() })
    }
}

/// 一个槽的继承：本节声明 → `Declared`；否则往前找第一个声明过的节 → `Inherited`；都没有 → `Absent`。
fn inherit(sections: &[SectionInfo], idx: usize, kind: HfKind, variant: HfVariant) -> HfSlot {
    if let Some(id) = sections[idx].hf_ref(kind, variant) {
        return HfSlot::Declared(id.to_string());
    }
    for prev in (0..idx).rev() {
        if let Some(id) = sections[prev].hf_ref(kind, variant) {
            return HfSlot::Inherited { from: prev, id: id.to_string() };
        }
    }
    HfSlot::Absent
}
