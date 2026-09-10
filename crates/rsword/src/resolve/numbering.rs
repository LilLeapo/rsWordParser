//! RES-09：按文档序计算编号标记；不修改声明值，不猜未支持的数字格式。
use super::Resolver;
use crate::{
    model::ListRef,
    semantic::props::{Level, Val},
};
use std::collections::{BTreeMap, BTreeSet};

fn int(v: &Option<Val<i32>>) -> Option<i32> {
    v.as_ref()?.value().copied()
}
fn format_number(fmt: &str, n: i32) -> Option<String> {
    match fmt {
        "decimal" => Some(n.to_string()),
        "decimalZero" => Some(format!("{n:02}")),
        "none" => Some(String::new()),
        "upperLetter" | "lowerLetter" if n > 0 => {
            let mut n = n;
            let mut out = Vec::new();
            while n > 0 {
                n -= 1;
                out.push((b'A' + (n % 26) as u8) as char);
                n /= 26;
            }
            let s: String = out.into_iter().rev().collect();
            Some(if fmt == "lowerLetter" { s.to_lowercase() } else { s })
        }
        "upperRoman" | "lowerRoman" if (1..=3999).contains(&n) => {
            let mut n = n;
            let mut s = String::new();
            for (v, t) in [
                (1000, "M"),
                (900, "CM"),
                (500, "D"),
                (400, "CD"),
                (100, "C"),
                (90, "XC"),
                (50, "L"),
                (40, "XL"),
                (10, "X"),
                (9, "IX"),
                (5, "V"),
                (4, "IV"),
                (1, "I"),
            ] {
                while n >= v {
                    s.push_str(t);
                    n -= v;
                }
            }
            Some(if fmt == "lowerRoman" { s.to_lowercase() } else { s })
        }
        _ => None,
    }
}
fn fmt(level: &Level) -> Option<&str> {
    level.num_fmt.as_ref()?.val.as_ref()?.value().map(|v| v.as_str())
}

impl Resolver<'_> {
    /// RES-09：输入为所属内容流的文档序列表。None 表示无编号或无法可靠解释，调用方区分 numId=0。
    /// 不支持的自定义/地区格式返回 None，不能悄悄退成十进制。
    pub fn list_markers(&self, items: &[ListRef]) -> Vec<Option<String>> {
        let mut counters: BTreeMap<i32, BTreeMap<i32, i32>> = BTreeMap::new();
        let mut used = BTreeSet::new();
        items
            .iter()
            .map(|item| {
                if item.num_id == 0 || !(0..=8).contains(&item.ilvl) {
                    return None;
                }
                let numbering = self.numbering?;
                let num = numbering.num(item.num_id)?;
                let abs = num.abstract_id()?;
                let level = self.level(item.num_id, item.ilvl)?;
                let state = counters.entry(abs).or_default();
                // 显式 lvlRestart=0 永不重置；其他高层出现时按声明触发。
                for deeper in item.ilvl + 1..=8 {
                    if let Some(l) = self.level(item.num_id, deeper) {
                        let reset = int(&l.lvl_restart).unwrap_or(deeper);
                        if reset > 0 && item.ilvl < reset {
                            state.remove(&deeper);
                        }
                    }
                }
                let first = used.insert((item.num_id, item.ilvl));
                let start = if first {
                    num.override_for(item.ilvl).and_then(|o| int(&o.start_override))
                } else {
                    None
                };
                let next = match (start, state.get(&item.ilvl)) {
                    (Some(n), _) => n,
                    (None, Some(n)) => n.checked_add(1)?,
                    (None, None) => level.start_or_default(),
                };
                state.insert(item.ilvl, next);
                if level.lvl_pic_bullet_id.is_some() {
                    return None;
                }
                let format = fmt(level)?;
                if format == "none" {
                    return Some(String::new());
                }
                let pattern = level.lvl_text.as_ref()?.val.as_deref()?;
                if format == "bullet" {
                    let font = level
                        .rpr
                        .as_ref()
                        .and_then(|p| p.fonts.as_ref())
                        .and_then(|f| f.ascii.as_deref())
                        .unwrap_or("");
                    return pattern
                        .chars()
                        .map(|c| {
                            if ('\u{e000}'..='\u{f8ff}').contains(&c) {
                                super::symbol::decode_pua(font, c)
                            } else {
                                Some(c)
                            }
                        })
                        .collect();
                }
                if level.num_fmt.as_ref().is_some_and(|f| f.format.is_some()) {
                    return None;
                }
                let mut out = String::new();
                let mut chars = pattern.chars().peekable();
                while let Some(c) = chars.next() {
                    if c == '%' && chars.peek().is_some_and(|c| ('1'..='9').contains(c)) {
                        let index = chars.next().unwrap() as i32 - '1' as i32;
                        let referenced = self.level(item.num_id, index)?;
                        let value = state
                            .get(&index)
                            .copied()
                            .unwrap_or_else(|| referenced.start_or_default());
                        let f = if level.is_lgl == Some(true) && index < item.ilvl {
                            "decimal"
                        } else {
                            fmt(referenced)?
                        };
                        out.push_str(&format_number(f, value)?);
                    } else {
                        out.push(c);
                    }
                }
                Some(out)
            })
            .collect()
    }
}
