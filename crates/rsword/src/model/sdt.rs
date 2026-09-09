//! 内容控件（`MOD-08`，任务 3.3）：`w:sdt` 的 `sdtPr` 读成 [`SdtInfo`]。
//!
//! 块级与 run 级 sdt 用同一个读取器。控件种类按 `sdtPr` 里第一个可识别的控件元素判定，**只看局部名**
//! ——复选框在 `w14`、重复节在 `w15`，Word 各版本的前缀不一样（TS 也是这么认的）。
//! 编辑策略在 `EDIT-03`：[`refusing_sdt`] 给出拒绝理由，`ContentLocked` / `SdtContentLocked` 只读，
//! 有 `data_binding` 的第一阶段也只读（显示文字只是绑定数据的缓存，Word 重开会从 customXml 刷回）。

use crate::xml::{Dom, LocalName, NodeId, QName};

/// 无字段枚举 + `as_str` + `parse`：把「变体 ↔ XML 字面」的名字表写成一张表，
/// 免得枚举、匹配、测试各抄一遍（M4 的 `model/macros.rs` 有同名的通用版本，这里只服务 sdt）。
///
/// ```ignore
/// sdt_enum! {
///     /// 文档注释
///     pub enum SdtLock { Unlocked => "unlocked", SdtLocked => "sdtLocked" }
/// }
/// ```
macro_rules! sdt_enum {
    ($(#[$m:meta])* pub enum $name:ident { $($(#[$vm:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$vm])* $variant,)+
        }

        impl $name {
            /// 全部变体，声明顺序。
            pub const ALL: &[$name] = &[$($name::$variant,)+];

            /// XML 字面。
            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text,)+
                }
            }

            /// 精确匹配 XML 字面。
            pub fn parse(s: &str) -> Option<$name> {
                match s {
                    $($text => Some($name::$variant),)+
                    _ => None,
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

sdt_enum! {
    /// 控件种类（`sdtPr` 里的控件元素）。识别不出来（只有 `w:id` / `w:tag` 一类）→ [`SdtControl::Unknown`]。
    pub enum SdtControl {
        /// `w:richText`：富文本（Word 的缺省内容控件）
        RichText => "richText",
        /// `w:text`：纯文本
        PlainText => "text",
        /// `w:picture`
        Picture => "picture",
        /// `w:comboBox`：可输入的下拉框
        ComboBox => "comboBox",
        /// `w:dropDownList`：只能选的下拉框
        DropDownList => "dropDownList",
        /// `w:date`
        Date => "date",
        /// `w14:checkbox`（Word 2013）
        Checkbox => "checkbox",
        /// `w:group`：成组的只读区
        Group => "group",
        /// `w:citation`：引文
        Citation => "citation",
        /// `w:bibliography`：参考文献
        Bibliography => "bibliography",
        /// `w:docPartObj`：构建基块（封面等）
        DocPartObj => "docPartObj",
        /// `w:docPartList`
        DocPartList => "docPartList",
        /// `w:equation`
        Equation => "equation",
        /// `w15:repeatingSection`
        RepeatingSection => "repeatingSection",
        /// `w15:repeatingSectionItem`
        RepeatingSectionItem => "repeatingSectionItem",
        /// 没有可识别的控件元素
        Unknown => "unknown",
    }
}

sdt_enum! {
    /// `w:lock/@w:val`。缺 `w:lock` 或字面不认识 → [`SdtLock::Unlocked`]。
    pub enum SdtLock {
        /// 都能改
        Unlocked => "unlocked",
        /// 控件本身不能删，内容可改
        SdtLocked => "sdtLocked",
        /// 内容只读，控件可删
        ContentLocked => "contentLocked",
        /// 都不行
        SdtContentLocked => "sdtContentLocked",
    }
}

impl SdtLock {
    /// 内容只读（`EDIT-03`：编辑落在这种 sdt 内 → `Err(EDIT_SDT_LOCKED)`）。
    pub const fn content_locked(self) -> bool {
        matches!(self, SdtLock::ContentLocked | SdtLock::SdtContentLocked)
    }

    /// 控件本身不可删除（`w:sdt` 元素受保护）。M3 没有删除整个 sdt 的操作，只建模。
    pub const fn sdt_locked(self) -> bool {
        matches!(self, SdtLock::SdtLocked | SdtLock::SdtContentLocked)
    }
}

/// `w:dataBinding`：控件内容绑定到 customXml part。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataBinding {
    pub prefix_mappings: Option<String>,
    pub xpath: Option<String>,
    pub store_item_id: Option<String>,
}

/// `w:docPartObj` / `w:docPartList` 的内容。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocPart {
    pub gallery: Option<String>,
    pub category: Option<String>,
    /// `w:docPartUnique`（三态 `OnOff`，缺省 false）。
    pub unique: bool,
}

/// 最近的 `w:sdt` 祖先（`MOD-08`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdtInfo {
    pub node: NodeId,
    /// `w:alias/@w:val`：给人看的标题。
    pub alias: Option<String>,
    /// `w:tag/@w:val`：给程序用的标签。
    pub tag: Option<String>,
    /// `w:id/@w:val`。
    pub id: Option<i32>,
    pub control: SdtControl,
    pub lock: SdtLock,
    pub data_binding: Option<DataBinding>,
    /// `w:docPartObj` / `w:docPartList` 的内容（控件种类见 `control`）。
    pub doc_part: Option<DocPart>,
    /// `w:placeholder/w:docPart/@w:val`：占位文字所在的构建基块名。
    pub placeholder: Option<String>,
    /// `w:showingPlcHdr`：当前显示的是占位文字而不是真实内容。
    pub showing_placeholder: bool,
}

fn w(local: LocalName) -> QName {
    QName::w(local)
}

impl SdtInfo {
    /// 读 `w:sdt` 的 `w:sdtPr`；没有 `sdtPr` 时除 `node` 外全是缺省值。
    pub fn read(dom: &Dom, sdt: NodeId) -> SdtInfo {
        let mut info = SdtInfo {
            node: sdt,
            alias: None,
            tag: None,
            id: None,
            control: SdtControl::Unknown,
            lock: SdtLock::Unlocked,
            data_binding: None,
            doc_part: None,
            placeholder: None,
            showing_placeholder: false,
        };
        let Some(pr) = dom.semantic_children(sdt).find(|&n| dom.is(n, w(LocalName::SdtPr))) else {
            return info;
        };
        let val = |n: NodeId| dom.attr_value(n, w(LocalName::Val)).map(|v| v.into_owned());
        let attr =
            |n: NodeId, local: LocalName| dom.attr_value(n, w(local)).map(|v| v.into_owned());
        // `OnOff` 元素：存在即 true，除非 `w:val` 明确关掉（`PROP-02`）
        let on = |n: NodeId| !matches!(val(n).as_deref(), Some("0" | "false" | "off"));
        for child in dom.semantic_children(pr) {
            let Some(name) = dom.name(child) else { continue };
            // 控件元素只看局部名：checkbox 在 w14、repeatingSection* 在 w15
            if info.control == SdtControl::Unknown
                && let Some(kind) =
                    name.local.known_str().and_then(SdtControl::parse).filter(|c| c.is_control())
            {
                info.control = kind;
                if matches!(kind, SdtControl::DocPartObj | SdtControl::DocPartList) {
                    info.doc_part = Some(read_doc_part(dom, child));
                }
                continue;
            }
            match name.local {
                LocalName::Alias => info.alias = val(child),
                LocalName::UTag | LocalName::Tag => {
                    if info.tag.is_none() {
                        info.tag = val(child);
                    }
                }
                LocalName::Id => info.id = val(child).and_then(|v| v.trim().parse().ok()),
                LocalName::Lock => {
                    info.lock =
                        val(child).as_deref().and_then(SdtLock::parse).unwrap_or(SdtLock::Unlocked);
                }
                LocalName::DataBinding => {
                    info.data_binding = Some(DataBinding {
                        prefix_mappings: attr(child, LocalName::PrefixMappings),
                        xpath: attr(child, LocalName::Xpath),
                        store_item_id: attr(child, LocalName::StoreItemID),
                    });
                }
                LocalName::Placeholder => {
                    info.placeholder = dom
                        .semantic_children(child)
                        .find(|&n| dom.is(n, w(LocalName::DocPart)))
                        .and_then(val);
                }
                LocalName::ShowingPlcHdr => info.showing_placeholder = on(child),
                _ => {}
            }
        }
        info
    }

    /// 内容只读（`w:lock`）。
    pub fn content_locked(&self) -> bool {
        self.lock.content_locked()
    }

    /// 绑定到 customXml：第一阶段不可编辑（`EDIT-03`）。
    pub fn is_bound(&self) -> bool {
        self.data_binding.is_some()
    }

    /// 拒绝编辑的理由；两条都不成立时 `None`。
    pub fn refusal(&self) -> Option<SdtRefusal> {
        if self.content_locked() {
            Some(SdtRefusal::Locked)
        } else if self.is_bound() {
            Some(SdtRefusal::Bound)
        } else {
            None
        }
    }
}

impl SdtControl {
    /// 这个变体对应一个真正的控件元素（`Unknown` 不是）。
    const fn is_control(self) -> bool {
        !matches!(self, SdtControl::Unknown)
    }
}

fn read_doc_part(dom: &Dom, node: NodeId) -> DocPart {
    let mut dp = DocPart::default();
    for c in dom.semantic_children(node) {
        let Some(name) = dom.name(c) else { continue };
        let val = || dom.attr_value(c, w(LocalName::Val)).map(|v| v.into_owned());
        match name.local {
            LocalName::DocPartGallery => dp.gallery = val(),
            LocalName::DocPartCategory => dp.category = val(),
            LocalName::DocPartUnique => {
                dp.unique = !matches!(val().as_deref(), Some("0" | "false" | "off"));
            }
            _ => {}
        }
    }
    dp
}

/// 为什么拒绝编辑（`EDIT-03`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdtRefusal {
    /// `w:lock` 为 `contentLocked` / `sdtContentLocked` → `EDIT_SDT_LOCKED`。
    Locked,
    /// 有 `w:dataBinding` → `EDIT_SDT_BOUND`（第一阶段）。
    Bound,
}

/// `node`（含自身）的祖先里第一个拒绝内容编辑的 `w:sdt`；从最近的祖先往外找。
pub fn refusing_sdt(dom: &Dom, node: NodeId) -> Option<(SdtInfo, SdtRefusal)> {
    std::iter::once(node)
        .chain(dom.ancestors(node))
        .filter(|&n| dom.is(n, w(LocalName::Sdt)))
        .find_map(|n| {
            let info = SdtInfo::read(dom, n);
            info.refusal().map(|r| (info, r))
        })
}
