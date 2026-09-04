//! 差分容忍（`COMPAT-09`）：键顺序无关、缺失与 `undefined` 等价、浮点 1e-6、按路径模式跳过已知差异。

use serde_json::Value;

/// 一处差异。`expected` / `actual` 为 `None` 表示该侧缺失。
#[derive(Debug, Clone, PartialEq)]
pub struct Diff {
    /// `blocks[3].runs[0].text`、`styles.Heading1.display.bold`
    pub path: String,
    pub expected: Option<Value>,
    pub actual: Option<Value>,
}

/// 递归比较，差异追加到 `out`。
pub fn diff_json(expected: &Value, actual: &Value, out: &mut Vec<Diff>) {
    walk(String::new(), Some(expected), Some(actual), out);
}

fn walk(path: String, e: Option<&Value>, a: Option<&Value>, out: &mut Vec<Diff>) {
    match (e, a) {
        (Some(Value::Object(eo)), Some(Value::Object(ao))) => {
            let mut keys: Vec<&String> = eo.keys().chain(ao.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let p = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                walk(p, eo.get(k), ao.get(k), out);
            }
        }
        (Some(Value::Array(ea)), Some(Value::Array(aa))) => {
            for i in 0..ea.len().max(aa.len()) {
                walk(format!("{path}[{i}]"), ea.get(i), aa.get(i), out);
            }
        }
        (Some(Value::Number(x)), Some(Value::Number(y))) => {
            let (x, y) = (x.as_f64().unwrap_or(f64::NAN), y.as_f64().unwrap_or(f64::NAN));
            if (x - y).abs() > 1e-6 && !(x.is_nan() && y.is_nan()) {
                out.push(Diff { path, expected: e.cloned(), actual: a.cloned() });
            }
        }
        (Some(x), Some(y)) if x == y => {}
        (None, None) => {}
        _ => out.push(Diff { path, expected: e.cloned(), actual: a.cloned() }),
    }
}

/// 路径模式：`*` 匹配任意字符序列（含 `.` 与下标），其余逐字匹配。
pub fn path_matches(pattern: &str, path: &str) -> bool {
    fn go(p: &[u8], s: &[u8]) -> bool {
        match p.split_first() {
            None => s.is_empty(),
            Some((b'*', rest)) => (0..=s.len()).any(|i| go(rest, &s[i..])),
            Some((c, rest)) => s.first() == Some(c) && go(rest, &s[1..]),
        }
    }
    go(pattern.as_bytes(), path.as_bytes())
}

/// 过滤掉命中已知差异模式的条目，返回 `(未知差异, 已知差异数)`。
pub fn filter_known(diffs: Vec<Diff>, known: &[&str]) -> (Vec<Diff>, usize) {
    let mut unknown = Vec::new();
    let mut n = 0;
    for d in diffs {
        if known.iter().any(|k| path_matches(k, &d.path)) {
            n += 1;
        } else {
            unknown.push(d);
        }
    }
    (unknown, n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compat_09_tolerant_diff() {
        let e = json!({"a": 1.0, "b": {"c": [1, 2, {"d": "x"}]}, "z": null});
        let a = json!({"b": {"c": [1, 2.0000001, {"d": "y"}], "extra": true}, "a": 1, "z": null});
        let mut out = Vec::new();
        diff_json(&e, &a, &mut out);
        let paths: Vec<&str> = out.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(paths, ["b.c[2].d", "b.extra"]);
        assert!(path_matches("b.c[*].d", "b.c[2].d"));
        assert!(path_matches("blocks[*].runs[*].rawRPr", "blocks[12].runs[0].rawRPr"));
        assert!(!path_matches("b.c[*].e", "b.c[2].d"));
        let (unknown, known) = filter_known(out, &["b.c[*].d"]);
        assert_eq!((unknown.len(), known), (1, 1));
    }
}
