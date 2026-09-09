//! 模型层的声明宏。
//!
//! 显示模型里有一批「无字段枚举 + 一个稳定短名字」的类型：种类、绕排、填充方式……名字用在
//! 诊断、语料普查的输出、以后的 i18n key 上。枚举写一遍、名字表再写一遍，迟早对不上——尤其是
//! 加变体的时候编译器不会提醒你去补名字表。宏把两者绑在同一处声明里。

/// 声明一个无字段枚举，并生成 `as_str`（名字表跟着变体走，漏了编译不过）。
///
/// ```ignore
/// named_enum! {
///     /// VML 元素种类。
///     pub enum VmlKind {
///         /// `v:shape`
///         Shape = "shape",
///         Group = "group",
///     }
/// }
/// ```
macro_rules! named_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($(#[$vmeta:meta])* $variant:ident = $text:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        $vis enum $name {
            $($(#[$vmeta])* $variant,)+
        }

        impl $name {
            /// 稳定的短名字。
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)+
                }
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

pub(crate) use named_enum;
