//! 表格属性表的手写辅助（`spec/05` PROP-08，任务 3.1）：生成的 `TblWidth` / `Merge` 只有属性字段，
//! 这里补上"读到的值是什么意思"的解释函数。解释只是读法，不改声明值（`MOD-07`：模型只存声明值）。

use crate::semantic::props::codec::Measure;
use crate::semantic::props::{Merge, MergeKind, TblWidth, TblWidthType, Val};

impl TblWidth {
    /// 绝对宽（twips）：`type` 缺省或 `dxa` 且 `w` 为数。`pct` / `auto` / `nil` 与 `Raw` 都是 `None`。
    pub fn twips(&self) -> Option<i32> {
        match (&self.kind, &self.w) {
            (None | Some(Val::Value(TblWidthType::Dxa)), Some(Val::Value(Measure::Number(n)))) => {
                Some(*n)
            }
            _ => None,
        }
    }

    /// 百分比宽（百分点，`50.0` = 50%）：`type=pct` 时数值是 1/50 百分点；字面 `NN%` 不看 `type` 直取。
    pub fn percent(&self) -> Option<f64> {
        match (&self.kind, &self.w) {
            (_, Some(Val::Value(Measure::Percent(hundredths)))) => {
                Some(f64::from(*hundredths) / 100.0)
            }
            (Some(Val::Value(TblWidthType::Pct)), Some(Val::Value(Measure::Number(n)))) => {
                Some(f64::from(*n) / 50.0)
            }
            _ => None,
        }
    }

    /// `w:type="auto"`（`w` 写 0，Word 的习惯）。
    pub fn auto() -> Self {
        Self { w: Some(Val::Value(Measure::Number(0))), kind: Some(Val::Value(TblWidthType::Auto)) }
    }

    /// `w:type="dxa"` 的绝对宽。
    pub fn dxa(twips: i32) -> Self {
        Self {
            w: Some(Val::Value(Measure::Number(twips))),
            kind: Some(Val::Value(TblWidthType::Dxa)),
        }
    }

    /// `w:type="pct"`，参数是百分点（`50.0` → `w:w="2500"`）。
    pub fn pct(percent: f64) -> Self {
        let fiftieths = (percent * 50.0).round() as i32;
        Self {
            w: Some(Val::Value(Measure::Number(fiftieths))),
            kind: Some(Val::Value(TblWidthType::Pct)),
        }
    }
}

impl Merge {
    /// `w:val="restart"`：合并区的第一格；其他（含无 `w:val`）都是 continue。
    pub fn is_restart(&self) -> bool {
        matches!(self.val, Some(Val::Value(MergeKind::Restart)))
    }

    pub fn restart() -> Self {
        Self { val: Some(Val::Value(MergeKind::Restart)) }
    }

    /// 裸 `<w:vMerge/>`。
    pub fn cont() -> Self {
        Self { val: None }
    }
}
