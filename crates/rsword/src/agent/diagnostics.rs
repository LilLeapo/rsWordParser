//! AGENT-01：版本化用户说明表；不改变原诊断字段或缺席状态。
use serde_json::{Value, json};
macro_rules! diagnostic_catalog {
    ($($code:literal => ($message:literal, $impact:literal);)*) => {
        pub const KNOWN: &[(&str,&str,&str)] = &[$(($code,$message,$impact)),*];
        fn description(code: &str) -> Option<(&'static str,&'static str)> {
            match code { $($code => Some(($message,$impact)),)* _=>None }
        }
        #[cfg(test)]
        #[test]
        fn agent_01_diagnostic_catalog_branches() {
            $(assert_eq!(description($code),Some(($message,$impact)));)*
            assert_eq!(description("ZZ_UNKNOWN"),None);
        }
    }
}
pub const VERSION: u32 = 1;
diagnostic_catalog! {
    "CHART_NO_SERIES" => ("没有带缓存值的系列，不能据此报告完整数据", "chartDataIncomplete");
    "XML_UNBOUND_PREFIX" => ("XML 前缀未绑定；原内容保留，对应扩展可能无法解释", "partialInterpretation");
    "AGENT_LIST_MARKER_UNRESOLVED" => ("编号声明缺失、损坏或格式尚不支持，不能可靠报告列表标记", "listMarkerUnavailable");
    "SPAN_NO_FLOW" => ("内容容器没有原生流身份，无法可靠定位或判断范围是否同流", "sourcePositionUnavailable");
}
pub fn diagnostic_view(raw: &Value) -> Value {
    let mut out = raw.as_object().cloned().unwrap_or_default();
    let code = raw.get("code").and_then(Value::as_str).unwrap_or("");
    let (message, impact, known) = match description(code) {
        Some((message, impact)) => (message, impact, true),
        None => (
            raw.get("message").and_then(Value::as_str).unwrap_or("未知诊断，未提供说明"),
            "unknown",
            false,
        ),
    };
    out.insert("userExplanation".into(), json!(message));
    out.insert("capabilityImpact".into(), json!(impact));
    out.insert("known".into(), json!(known));
    Value::Object(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn agent_01_unknown_diagnostic_preserves_presence() {
        let raw = json!({"code":"ZZ_UNKNOWN","message":"original","part":9,"range":[2,4]});
        let view = diagnostic_view(&raw);
        for (k, v) in raw.as_object().unwrap() {
            assert_eq!(&view[k], v);
        }
        assert!(view.get("origin").is_none());
        assert_eq!(view["known"], false);
        assert_eq!(view["userExplanation"], "original");
        assert!(diagnostic_view(&json!({"code":"ZZ_UNKNOWN"})).get("message").is_none());
    }
}
