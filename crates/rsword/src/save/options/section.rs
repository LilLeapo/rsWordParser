//! `SAVE-07` 的节相关保存选项 → `SectionPropsPatch`（`spec/16` 任务 5.6）。
//!
//! 四个选项都落在**最后一节**（body 级的那个 `w:sectPr`，TS 的"trailing hidden sectPr"）：
//!
//! | 选项 | 元素 |
//! | --- | --- |
//! | [`SectionSaveSettings`] | `w:pgSz` / `w:pgMar` / `w:pgBorders` / `w:cols` / `w:bidi` |
//! | `section_start_type` | `w:type`（`nextPage` = 删掉，它是缺省） |
//! | `pg_num_type` | `w:pgNumType`（两个字段都缺 = 删掉） |
//! | `title_pg` | `w:titlePg` |
//!
//! 新元素的位置由 `plan_apply_section_props_at` 按 `PROP-05` 的 `order` 决定，不跟随 TS 正则式的落点
//! （TS 把 `w:titlePg` 塞在 `w:docGrid` 之前，schema 里它在 `w:vAlign` / `w:noEndnote` 之后——
//! 同一个位置；但 TS 的 `w:bidi` 落在 `w:docGrid` 之前而 schema 要求它在 `w:textDirection` 之后，
//! 这类分歧一律按 schema 走）。

use crate::semantic::props::{
    Border, BorderStyle, Change, Column, Columns, ColumnsPatch, DocProtect, HexColorOrAuto,
    NumberFormat, PageBorderOffset, PageBorders, PageMar, PageNumber, PageSz, SectType,
    SectionProps, SectionPropsPatch, TableChange, Val,
};

/// TS `SectionSettings` 的**写侧**子集：`applySectionSettings` 只用到这些字段。
///
/// 读侧的全量形态在 `bind/compat_ts`（`sectionSettingsFromXml`），这里只收写回要的那些，
/// 免得把 `pageBorderProps` / `docGrid` 这些只读字段也搬进保存选项。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SectionSaveSettings {
    /// `w:pgSz/@w:w`。
    pub page_width: i32,
    /// `w:pgSz/@w:h`。
    pub page_height: i32,
    /// 为真写 `w:orient="landscape"`；纵向不写属性（同 TS）。
    pub landscape: bool,
    pub margin_top: i32,
    pub margin_right: i32,
    pub margin_bottom: i32,
    pub margin_left: i32,
    /// `w:pgMar/@w:header`；`None` 时保留文档原值。
    pub header_dist: Option<i32>,
    /// `w:pgMar/@w:footer`；`None` 时保留文档原值。
    pub footer_dist: Option<i32>,
    /// 为真放一圈 `single sz=4 space=24 auto`、`offsetFrom="page"` 的页面边框；为假删掉 `w:pgBorders`。
    pub page_border: bool,
    /// 栏数；1 表示不写 `w:num`。
    pub columns: i32,
    /// `w:cols/@w:space`；`None` 时按 TS 的两个缺省（改已有元素时 720，新建时 425）。
    pub col_space: Option<i32>,
    /// 不等宽各栏宽度；长度必须等于 `columns` 且 `columns > 1` 才生效。
    pub col_widths: Option<Vec<i32>>,
    /// `w:bidi`：`None` 不动，`Some` 确保存在 / 不存在。
    pub bidi: Option<bool>,
}

/// `w:pgNumType` 选项：两个字段都是 `None` 表示**删掉**这个元素（TS 同）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PgNumTypeOption {
    pub fmt: Option<NumberFormat>,
    pub start: Option<i32>,
}

/// `w:documentProtection` 选项（TS `DocProtection`）。`hash` 缺失时不写任何 crypt 属性。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectionOption {
    pub edit: DocProtect,
    pub enforced: bool,
    pub hash: Option<String>,
    pub salt: Option<String>,
    /// 缺省 100000。
    pub spin_count: Option<i32>,
    /// 缺省 14（SHA-512）。
    pub algorithm_sid: Option<i32>,
}

/// `w:writeProtection` 选项（TS `WriteProtection`）：`recommended` 与 `hash` 全无时等于删掉。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WriteProtectionOption {
    pub recommended: bool,
    pub hash: Option<String>,
    pub salt: Option<String>,
    pub spin_count: Option<i32>,
    pub algorithm_sid: Option<i32>,
}

/// `Option<T>` → `Change<Val<T>>`：给出就 `Set`，缺失就 `Keep`。
///
/// `SectionSaveSettings` 的度量字段与 `pg_num_type` 的两个字段共八处同形状（`spec/16` 的宏计划）。
///
/// ```ignore
/// let patch = SectionPropsPatch { page_numbers: ..., ..Default::default() };
/// let fmt = patch_some!(opt.fmt);      // Change<Val<NumberFormat>>
/// ```
macro_rules! patch_some {
    ($v:expr) => {
        match $v {
            Some(x) => Some($crate::semantic::props::Val::Value(x)),
            None => None,
        }
    };
}

/// 一圈页面边框（TS `applySectionSettings` 的字面值）。
fn border_box() -> PageBorders {
    let side = || {
        Some(Border {
            val: Some(Val::Value(BorderStyle::Single)),
            sz: Some(Val::Value(4)),
            space: Some(Val::Value(24)),
            color: Some(Val::Value(HexColorOrAuto::Auto)),
            ..Default::default()
        })
    };
    PageBorders {
        offset_from: Some(Val::Value(PageBorderOffset::Page)),
        top: side(),
        left: side(),
        bottom: side(),
        right: side(),
        ..Default::default()
    }
}

/// `w:cols`：栏数、栏间距与不等宽子元素。返回 `Keep` 表示这一项不用动。
fn columns_change(
    current: Option<&Columns>,
    s: &SectionSaveSettings,
) -> TableChange<Columns, ColumnsPatch> {
    let num = (s.columns > 1).then_some(Val::Value(s.columns));
    let widths = s.col_widths.as_deref().unwrap_or(&[]);
    // 不等宽：栏数对得上才重建；文档里已经是这些值就原字节不动（往返安全，同 TS 的 `unchanged`）
    if s.columns > 1 && widths.len() == s.columns as usize {
        let space = s.col_space.unwrap_or(720);
        let same = current.is_some_and(|c| {
            c.num.as_ref().and_then(Val::value).copied() == Some(s.columns)
                && c.col.len() == widths.len()
                && c.col
                    .iter()
                    .zip(widths)
                    .all(|(c, &w)| c.w.as_ref().and_then(Val::value).copied() == Some(w))
        });
        if same {
            return TableChange::Keep;
        }
        let last = widths.len() - 1;
        let col = widths
            .iter()
            .enumerate()
            .map(|(i, &w)| Column {
                w: Some(Val::Value(w)),
                space: (i < last).then_some(Val::Value(space)),
            })
            .collect();
        return TableChange::Set(Columns {
            equal_width: Some(false),
            space: Some(Val::Value(space)),
            num,
            col,
            ..Default::default()
        });
    }
    match current {
        // 已有元素：栏数变了才动，`w:col` 子元素随栏数变化失效（TS 同样丢掉它们）
        Some(c) => {
            let cur_num = c.num.as_ref().and_then(Val::value).copied().unwrap_or(1);
            let space = match s.col_space {
                Some(v) if v != c.space.as_ref().and_then(Val::value).copied().unwrap_or(720) => {
                    Some(Val::Value(v))
                }
                _ => c.space.clone(),
            };
            if cur_num == s.columns && space == c.space && c.col.is_empty() {
                return TableChange::Keep;
            }
            TableChange::Patch(ColumnsPatch {
                num: match num {
                    Some(v) => Change::Set(v),
                    None => Change::Unset,
                },
                space: match space {
                    Some(v) => Change::Set(v),
                    None => Change::Keep,
                },
                col: if c.col.is_empty() { Change::Keep } else { Change::Set(Vec::new()) },
                ..Default::default()
            })
        }
        // 没有元素：只有多栏才建（TS 新建时的 `w:space` 缺省是 425，不是 720）
        None if s.columns > 1 => TableChange::Set(Columns {
            space: Some(Val::Value(s.col_space.unwrap_or(425))),
            num,
            ..Default::default()
        }),
        None => TableChange::Keep,
    }
}

/// [`SectionSaveSettings`] → 补丁。`current` 是这一节现在的属性（`w:pgMar` 的 `w:gutter` 等
/// 未给出的属性要从它继承——补丁里 `Change::Set` 是整元素替换）。
pub fn settings_patch(current: &SectionProps, s: &SectionSaveSettings) -> SectionPropsPatch {
    let old_mar = current.page_margins.clone().unwrap_or_default();
    let page_margins = PageMar {
        top: Some(Val::Value(s.margin_top)),
        right: Some(Val::Value(s.margin_right)),
        bottom: Some(Val::Value(s.margin_bottom)),
        left: Some(Val::Value(s.margin_left)),
        // 未给出的沿用原值；整个元素是新建的就按 TS 的缺省
        header: patch_some!(s.header_dist).or_else(|| {
            old_mar.header.clone().or(current.page_margins.is_none().then_some(Val::Value(708)))
        }),
        footer: patch_some!(s.footer_dist).or_else(|| {
            old_mar.footer.clone().or(current.page_margins.is_none().then_some(Val::Value(708)))
        }),
        gutter: old_mar.gutter.clone().or(current.page_margins.is_none().then_some(Val::Value(0))),
    };
    let page_size = PageSz {
        w: Some(Val::Value(s.page_width)),
        h: Some(Val::Value(s.page_height)),
        orient: s.landscape.then_some(Val::Value(crate::semantic::props::PageOrient::Landscape)),
        // 打印机纸型代码是硬件配置，改页面尺寸不该丢它（TS 整个替换 `w:pgSz`，会丢；
        // 语料里没有带 `w:code` 的用例，出现了就是我们更保守）
        code: current.page_size.as_ref().and_then(|p| p.code.clone()),
    };
    SectionPropsPatch {
        page_size: Change::Set(page_size),
        page_margins: Change::Set(page_margins),
        page_borders: if s.page_border {
            TableChange::Set(border_box())
        } else {
            TableChange::Unset
        },
        columns: columns_change(current.columns.as_ref(), s),
        bidi: match s.bidi {
            None => Change::Keep,
            Some(true) => Change::Set(true),
            Some(false) => Change::Unset,
        },
        ..Default::default()
    }
}

/// `sectionStartType`：`nextPage` 是缺省，写它等于删掉 `w:type`。
pub fn start_type_patch(kind: SectType) -> SectionPropsPatch {
    SectionPropsPatch {
        kind: match kind {
            SectType::NextPage => Change::Unset,
            other => Change::Set(Val::Value(other)),
        },
        ..Default::default()
    }
}

/// `pgNumType`：两个字段都缺就删掉整个元素。
pub fn pg_num_type_patch(o: &PgNumTypeOption) -> SectionPropsPatch {
    let page_numbers = if o.fmt.is_none() && o.start.is_none() {
        Change::Unset
    } else {
        Change::Set(PageNumber {
            fmt: patch_some!(o.fmt),
            start: patch_some!(o.start),
            chap_style: None,
            chap_sep: None,
        })
    };
    SectionPropsPatch { page_numbers, ..Default::default() }
}

/// `titlePg`：首页不同。
pub fn title_pg_patch(on: bool) -> SectionPropsPatch {
    SectionPropsPatch {
        title_pg: if on { Change::Set(true) } else { Change::Unset },
        ..Default::default()
    }
}
