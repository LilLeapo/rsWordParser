//! `BIND-10`：模型投影后的预算裁剪，跨块索引保持全量；遍历不递归。
use super::{ApiError, session::bad};
use serde::Deserialize;
use serde_json::Value;

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Selection {
    pub display: bool,
    block_range: Option<BlockRange>,
    fields: Option<Vec<String>>,
    depth: Option<u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockRange {
    from: usize,
    to: usize,
}

impl Selection {
    pub fn select(self, value: &mut Value) -> Result<(), ApiError> {
        let total = value["main"].as_array().expect("main array").len();
        let mut truncated = false;
        if let Some(range) = self.block_range {
            if range.from > range.to {
                return Err(bad("blockRange.from 必须不大于 to"));
            }
            let from = range.from.min(total);
            let to = range.to.min(total);
            let blocks = value["main"].as_array_mut().expect("main array");
            blocks.truncate(to);
            blocks.drain(..from);
            truncated |= from != 0 || to != total;
        }
        if let Some(limit) = self.depth {
            let mut stack = vec![(value as &mut Value, 0)];
            while let Some((v, depth)) = stack.pop() {
                let block = v.get("node").is_some()
                    && matches!(
                        v.get("kind").and_then(Value::as_str),
                        Some("text" | "table" | "image" | "protected")
                    );
                if block && depth > limit {
                    *v = serde_json::json!({"kind":"protected", "node":v["node"], "protectedKind":{"kind":"tooDeep"}, "preview":"", "revisions":v["revisions"]});
                    truncated = true;
                    continue;
                }
                let next = depth + u32::from(block);
                match v {
                    Value::Array(items) => stack.extend(items.iter_mut().map(|v| (v, next))),
                    Value::Object(items) => stack.extend(items.values_mut().map(|v| (v, next))),
                    _ => {}
                }
            }
        }
        let object = value.as_object_mut().expect("document object");
        if let Some(fields) = self.fields {
            object.retain(|key, _| {
                let keep = fields.contains(key)
                    || matches!(key.as_str(), "spans" | "fields" | "revisions");
                truncated |= !keep;
                keep
            });
        }
        object.insert("totalBlocks".into(), Value::from(total));
        object.insert("truncated".into(), Value::from(truncated));
        Ok(())
    }
}
