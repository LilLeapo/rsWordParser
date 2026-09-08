//! 主题声明值的 JSON 投影（`BIND-02`，`MOD-10`）：`a:theme/a:themeElements` 的字体方案与
//! 颜色方案。`ColorScheme` 的 12 槽是私有字段、不能进 `model_json!` 的解构，整对象
//! （含 `node` / `name`）由 `color_scheme_json` 展开，`Theme.colors` 行以 `raw` 引用，
//! 故 `ColorScheme` 本身没有投影表。

use serde_json::{Map, Value};

use crate::model::theme::{ColorScheme, FontScheme, FontSlots, Theme, ThemeSlot};
use crate::xml::NodeId;

use super::{as_str_json, model_json, set, set_some};

// `ThemeSlot` 有手写 `as_str`（`dk1` … `folHlink`）；schema 不列 12 槽，按任意字串。
as_str_json!(ThemeSlot);

model_json! {
    /// 一组字体（`a:majorFont` / `a:minorFont`，`MOD-10`）：三个脚本槽位与
    /// `a:font script→typeface` 表。`scripts` 的元组一律 `[script, typeface]`。
    struct FontSlots(cx) test json_fields_cover_font_slots {
        opt node => "node", NodeId = node;
        opt latin => "latin", String = latin;
        opt ea => "ea", String = ea;
        opt cs => "cs", String = cs;
        scripts => "scripts", Vec<(String, String)> = scripts;
    }
}

model_json! {
    /// `a:fontScheme`（`MOD-10`）。
    struct FontScheme(cx) test json_fields_cover_font_scheme {
        node => "node", NodeId = node;
        opt name => "name", String = name;
        major => "major", FontSlots = major;
        minor => "minor", FontSlots = minor;
    }
}

model_json! {
    /// `a:theme` 的声明值（`MOD-10`）。`colors` 投成 `ColorScheme` 的整对象
    /// `{node?, name?, colors}`（见 `color_scheme_json`）；raw 行的声明类型只服务 schema，
    /// 该形状没有现成投影类型可借，用 `Value`（schema `{}`）。
    struct Theme(cx) test json_fields_cover_theme {
        node => "node", NodeId = node;
        opt name => "name", String = name;
        opt fonts => "fonts", FontScheme = fonts;
        raw opt colors => "colors", Value = colors.as_ref().map(color_scheme_json);
    }
}

/// `ColorScheme` 的整对象投影（`MOD-10`）：`{node?, name?, colors: {dk1: "RRGGBB" | null, …}}`。
/// 12 槽恒写全（`ThemeSlot::ALL` 序），缺槽为 `null`；hex 大写 6 位无 `#`，与
/// `bind::native::json` 的 `[u8; 3]` 投影一致。
fn color_scheme_json(cs: &ColorScheme) -> Value {
    let mut o = Map::new();
    set_some!(&mut o,
        "node" => cs.node.map(|n| n.0),
        "name" => cs.name.clone(),
    );
    let mut colors = Map::new();
    for slot in ThemeSlot::ALL {
        let v = match cs.get(slot) {
            Some([r, g, b]) => Value::from(format!("{r:02X}{g:02X}{b:02X}")),
            None => Value::Null,
        };
        colors.insert(slot.as_str().to_string(), v);
    }
    set(&mut o, "colors", Value::Object(colors));
    Value::Object(o)
}
