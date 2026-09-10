//! `BIND-03` 无载荷枚举的稳定字串；穷尽匹配防止新增变体漏进线型。

use crate::model::BreakKind;
use crate::model::{HfKind, HfVariant};

macro_rules! wire_enum {
    ($ty:ty { $($variant:ident => $text:literal),+ $(,)? }) => {
        impl ::serde::Serialize for $ty {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(match self { $(Self::$variant => $text),+ })
            }
        }
        impl<'de> ::serde::Deserialize<'de> for $ty {
            fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = <String as ::serde::Deserialize>::deserialize(d)?;
                match s.as_str() { $($text => Ok(Self::$variant),)+ _ => Err(::serde::de::Error::custom("unknown enum value")) }
            }
        }
    };
}

wire_enum!(HfKind { Header => "header", Footer => "footer" });
wire_enum!(HfVariant { Default => "default", First => "first", Even => "even" });
wire_enum!(BreakKind { TextWrapping => "textWrapping", Page => "page", Column => "column" });
