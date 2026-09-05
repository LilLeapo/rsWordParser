//! 差分容忍（`COMPAT-09`）与差分报告（`TEST-03`）：键顺序无关、缺失与 `undefined` 等价、浮点 1e-6、
//! 按 `KNOWN_DIFFS.md` 的"文档 glob + 路径 glob"跳过已知差异并单独计数。

use std::collections::BTreeMap;

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

/// glob：`*` 匹配任意字符序列（含 `.` 与下标），其余逐字匹配。
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

/// `KNOWN_DIFFS.md` 的一条机器可读条目：文档名 glob + JSON 路径 glob（`*` 表示整份文档）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownDiff {
    pub doc: String,
    pub path: String,
}

impl KnownDiff {
    pub fn matches(&self, file: &str, path: &str) -> bool {
        path_matches(&self.doc, file) && path_matches(&self.path, path)
    }
}

/// `KNOWN_DIFFS.md` 原文（编进库里，工具与测试共用同一份清单）。
pub const KNOWN_DIFFS_MD: &str = include_str!("KNOWN_DIFFS.md");

/// 读 markdown 里 ```known-diffs 围栏块：每行 `<文档 glob> <路径 glob>`，`#` 起为注释。
pub fn parse_known_diffs(md: &str) -> Vec<KnownDiff> {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in md.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            in_block = !in_block && t.starts_with("```known-diffs");
            continue;
        }
        if !in_block {
            continue;
        }
        let body = t.split('#').next().unwrap_or("").trim();
        if body.is_empty() {
            continue;
        }
        let mut it = body.split_whitespace();
        let (Some(doc), Some(path)) = (it.next(), it.next()) else { continue };
        out.push(KnownDiff { doc: doc.to_string(), path: path.to_string() });
    }
    out
}

/// 库内置的已知差异清单。
pub fn known_diffs() -> Vec<KnownDiff> {
    parse_known_diffs(KNOWN_DIFFS_MD)
}

/// 把差异分成 `(未知, 已知数)`。`file` 是文档文件名（`xxx__001.docx`）。
pub fn split_known(diffs: Vec<Diff>, file: &str, known: &[KnownDiff]) -> (Vec<Diff>, usize) {
    let mut unknown = Vec::new();
    let mut n = 0;
    for d in diffs {
        if known.iter().any(|k| k.matches(file, &d.path)) {
            n += 1;
        } else {
            unknown.push(d);
        }
    }
    (unknown, n)
}

/// 兼容旧调用：只按路径模式过滤。
pub fn filter_known(diffs: Vec<Diff>, known: &[&str]) -> (Vec<Diff>, usize) {
    let k: Vec<KnownDiff> =
        known.iter().map(|p| KnownDiff { doc: "*".into(), path: (*p).to_string() }).collect();
    split_known(diffs, "", &k)
}

/// 路径去下标：`blocks[3].runs[0].text` → `blocks[].runs[].text`，用于聚合。
pub fn path_key(path: &str) -> String {
    path.chars().filter(|c| !c.is_ascii_digit()).collect()
}

/// M1 门的"文本段落"用例判定：只有 paragraph / heading / listItem 与允许的 passthrough，
/// 不含字段 / 图片 / 公式 / ruby / 脚注 / 批注 / 书签 / 文本框 / `w14:textFill` / 参考文献 / 页眉页脚。
pub fn is_text_case(e: &Value) -> bool {
    case_in_scope(e, Scope::Text)
}

/// M2 门的"字段与 Span"域：文本域再放开字段、范围标记、批注与注释引用。
///
/// 是文本域的**超集**——M2 之后这些特性都该是零未知差异，所以门只会更严，不会漏掉 M1 的用例。
/// 仍然排除后续里程碑的东西：图片 / 公式 / ruby / 文本框 / 表格 / 页眉页脚 / 参考文献。
pub fn is_span_field_case(e: &Value) -> bool {
    case_in_scope(e, Scope::Fields)
}

/// M3 门的"表格"域：字段域再放开表格块。
///
/// 是字段域的**超集**。仍然排除单元格里的绘图（`anchoredBoxes` 与 run 上的 `image`）与公式 / ruby
/// ——那些是 M4 / M6 的域（`spec/14`「不在 M3」）。
pub fn is_table_case(e: &Value) -> bool {
    case_in_scope(e, Scope::Tables)
}

/// 差分取样范围（`TEST-03`）；每一档是前一档的超集。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    /// M1 门：纯文本段落。
    Text,
    /// M2 门：再加字段 / 范围标记 / 批注 / 注释。
    Fields,
    /// M3 门：再加表格。
    Tables,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Text => "text",
            Scope::Fields => "fields",
            Scope::Tables => "tables",
        }
    }

    pub fn parse(s: &str) -> Option<Scope> {
        match s {
            "text" => Some(Scope::Text),
            "fields" => Some(Scope::Fields),
            "tables" => Some(Scope::Tables),
            _ => None,
        }
    }

    /// 该范围的取样判定。
    pub fn accepts(self, e: &Value) -> bool {
        case_in_scope(e, self)
    }
}

/// 表格块本身是否在 M3 的域内：单元格里没有绘图（`anchoredBoxes` / run `image`）、公式与 ruby。
fn table_in_scope(table: &Value) -> bool {
    let Some(rows) = table.get("rows").and_then(Value::as_array) else { return true };
    for row in rows {
        for cell in row.as_array().into_iter().flatten() {
            if cell.get("anchoredBoxes").is_some() || cell.get("anchoredBoxAnchors").is_some() {
                return false;
            }
            for rp in cell.get("richParas").and_then(Value::as_array).into_iter().flatten() {
                for r in rp.get("runs").and_then(Value::as_array).into_iter().flatten() {
                    for k in ["image", "math", "ruby"] {
                        if r.get(k).is_some() {
                            return false;
                        }
                    }
                }
            }
            for nested in cell.get("nestedTables").and_then(Value::as_array).into_iter().flatten() {
                if !table_in_scope(nested) {
                    return false;
                }
            }
        }
    }
    true
}

fn case_in_scope(e: &Value, scope: Scope) -> bool {
    let fields_ok = scope >= Scope::Fields;
    let Some(blocks) = e.get("blocks").and_then(Value::as_array) else { return false };
    let mut text_blocks = 0;
    for b in blocks {
        let ty = b.get("type").and_then(Value::as_str).unwrap_or("");
        match ty {
            "paragraph" | "heading" | "listItem" => {
                text_blocks += 1;
                let runs = b.get("runs").and_then(Value::as_array).cloned().unwrap_or_default();
                for r in &runs {
                    // 后续里程碑的 run 特性
                    for k in ["image", "math", "ruby"] {
                        if r.get(k).is_some() {
                            return false;
                        }
                    }
                    if !fields_ok {
                        for k in [
                            "noteRef",
                            "xeTerm",
                            "refField",
                            "instrField",
                            "fldBeginXml",
                            "commentIds",
                        ] {
                            if r.get(k).is_some() {
                                return false;
                            }
                        }
                    }
                }
                for k in ["textboxes", "strayRuns"] {
                    if b.get(k).is_some() {
                        return false;
                    }
                }
                if !fields_ok {
                    for k in ["bookmarks", "hiddenBookmarks", "commentStarts", "commentEnds"] {
                        if b.get(k).is_some() {
                            return false;
                        }
                    }
                }
                let xml = b.get("originalXml").and_then(Value::as_str).unwrap_or("");
                if xml.contains("w14:textFill") {
                    return false;
                }
                if !fields_ok
                    && (xml.contains("<w:fldChar")
                        || xml.contains("<w:fldSimple")
                        || xml.contains("<w:instrText"))
                {
                    return false;
                }
            }
            "passthrough" => {
                let label = b.get("label").and_then(Value::as_str).unwrap_or("");
                let ok = label == "Section properties"
                    || label == "Section break paragraph"
                    || label == "Hidden paragraph"
                    || label == "Page break"
                    || b.get("invisibleMarker").and_then(Value::as_bool).unwrap_or(false);
                if !ok {
                    return false;
                }
            }
            "table" if scope >= Scope::Tables => {
                text_blocks += 1;
                match b.get("table") {
                    Some(t) if table_in_scope(t) => {}
                    // TS 解析失败（恶意深度）时没有 `table`，不该拿来当门
                    _ => return false,
                }
            }
            _ => return false,
        }
    }
    let empty_arr = |k: &str| e.get(k).and_then(Value::as_array).is_some_and(Vec::is_empty);
    // 批注与脚注 / 尾注在任务 2.6 落地，带它们的文档不再排除在文本域之外
    text_blocks > 0
        && empty_arr("sources")
        && e.get("headerText").is_none_or(Value::is_null)
        && e.get("footerText").is_none_or(Value::is_null)
        && e.get("hfParts").and_then(Value::as_object).is_some_and(|m| m.is_empty())
}

/// M4 绘图域的 JSON 路径（`TEST-10` 的 M4 门）：块上的图片 / 文本框 / 细横线 / 嵌入对象字段，
/// 以及 run 内的图片。表格内的图片（M3）、页眉页脚的图片（M5）、图表与公式（M6）不算绘图域——
/// 它们各自归后续里程碑，混进来会让 M4 的门永远关不上。
pub fn is_drawing_path(path: &str) -> bool {
    /// 块上的绘图域字段前缀。
    const FIELDS: [&str; 8] = [
        "image",       // imageDataUrl / imageWidthPx / imageWrap / imageZOrder…
        "textboxes",   // 文本框数组及其全部载荷
        "rule",        // 细横线 ruleWidthPx / ruleColorHex / ruleThicknessPx
        "decorative",  // 细横线与装饰性形状
        "oleProgId",   // 嵌入对象
        "brokenImage", //
        "strayRuns",   // 形状外的游离文字（4.6）
        "strayStyleId",
    ];
    let key = path_key(path);
    let Some(rest) = key.strip_prefix("blocks[].") else { return false };
    FIELDS.iter().any(|f| rest.starts_with(f))
        || rest.strip_prefix("runs[].").is_some_and(|r| r.starts_with("image"))
}

/// 一类未知差异的聚合：出现次数与首个样例。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PathStat {
    pub count: usize,
    pub docs: usize,
    pub sample_doc: String,
    pub sample_path: String,
    pub sample_expected: Option<Value>,
    pub sample_actual: Option<Value>,
}

/// 多份文档的差分汇总（`TEST-03` 的输出）。
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub docs: usize,
    pub docs_with_unknown: usize,
    pub known: usize,
    pub unknown: usize,
    /// 按去下标路径聚合。
    pub by_path: BTreeMap<String, PathStat>,
}

impl Report {
    /// 记录一份文档的差分结果。
    pub fn add(&mut self, file: &str, unknown: Vec<Diff>, known: usize) {
        self.docs += 1;
        self.known += known;
        if unknown.is_empty() {
            return;
        }
        self.docs_with_unknown += 1;
        let mut seen_keys = Vec::new();
        for d in unknown {
            self.unknown += 1;
            let key = path_key(&d.path);
            let st = self.by_path.entry(key.clone()).or_default();
            st.count += 1;
            if !seen_keys.contains(&key) {
                st.docs += 1;
                seen_keys.push(key);
            }
            if st.sample_doc.is_empty() {
                st.sample_doc = file.to_string();
                st.sample_path = d.path;
                st.sample_expected = d.expected;
                st.sample_actual = d.actual;
            }
        }
    }
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
        let (unknown, known) = filter_known(out.clone(), &["b.c[*].d"]);
        assert_eq!((unknown.len(), known), (1, 1));
        let k = vec![KnownDiff { doc: "doc__0*".into(), path: "b.*".into() }];
        assert_eq!(split_known(out.clone(), "doc__001.docx", &k).1, 2);
        assert_eq!(split_known(out, "other.docx", &k).1, 0);
        assert_eq!(path_key("blocks[3].runs[0].text"), "blocks[].runs[].text");
    }

    #[test]
    fn test_03_known_diffs_block_parses() {
        let md = "# x\n\n```known-diffs\n# comment\nnumbering-defs__012* numbering.*   # why\n* styles.*.tableDisplay*\n```\ntext\n```\nnot known\n```\n";
        let k = parse_known_diffs(md);
        assert_eq!(k.len(), 2);
        assert_eq!(
            k[0],
            KnownDiff { doc: "numbering-defs__012*".into(), path: "numbering.*".into() }
        );
        assert!(k[1].matches("any.docx", "styles.Foo.tableDisplay.fill"));
        assert!(!known_diffs().is_empty(), "KNOWN_DIFFS.md 须含机器可读块");
    }

    #[test]
    fn test_10_drawing_domain_paths() {
        for p in [
            "blocks[3].imageWidthPx",
            "blocks[0].textboxes[1].paras[0].runs[0].text",
            "blocks[2].ruleColorHex",
            "blocks[2].decorative",
            "blocks[9].oleProgId",
            "blocks[1].runs[0].image.wrap",
            "blocks[1].strayRuns[0].text",
        ] {
            assert!(is_drawing_path(p), "{p} 应属绘图域");
        }
        // 表格里的图片归 M3、页眉页脚的图片归 M5、图表与公式归 M6
        for p in [
            "blocks[3].table.richParas[0].runs[0].image.dataUrl",
            "headerImages[0].dataUrl",
            "blocks[3].chartDisplay.kind",
            "blocks[3].formulaDisplay",
            "blocks[3].runs[0].text",
            "internal.documentXml",
        ] {
            assert!(!is_drawing_path(p), "{p} 不该算绘图域");
        }
    }

    #[test]
    fn test_03_report_aggregates_by_path() {
        let mut r = Report::default();
        r.add(
            "a.docx",
            vec![Diff {
                path: "blocks[1].runs[0].text".into(),
                expected: Some(json!("x")),
                actual: None,
            }],
            1,
        );
        r.add(
            "b.docx",
            vec![Diff {
                path: "blocks[7].runs[2].text".into(),
                expected: None,
                actual: Some(json!("y")),
            }],
            0,
        );
        r.add("c.docx", vec![], 3);
        assert_eq!((r.docs, r.docs_with_unknown, r.known, r.unknown), (3, 2, 4, 2));
        let st = &r.by_path["blocks[].runs[].text"];
        assert_eq!((st.count, st.docs), (2, 2));
        assert_eq!(st.sample_doc, "a.docx");
    }
}
