//! BIND-01：有状态原生协议薄壳；兼容面仍保持原来的五个无状态函数。
use super::js_error;
use wasm_bindgen::prelude::*;

/// 一张原生协议会话表；每份文档由 open 返回一个不透明字符串句柄。
#[wasm_bindgen(js_name = SessionTable)]
#[derive(Default)]
pub struct NativeSessions {
    inner: rsword::bind::native::SessionTable,
}

#[wasm_bindgen(js_class = SessionTable)]
impl NativeSessions {
    /// 创建空会话表。
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }
    /// 幂等释放会话。
    pub fn close(&mut self, id: &str) {
        self.inner.close(id);
    }
    /// 引擎、构建提交与 native 协议版本。
    pub fn version(&self) -> String {
        rsword::bind::native::SessionTable::version().to_string()
    }
}

rsword::bind_export! { class [wasm_bindgen(js_class = SessionTable)] error(js_error, JsValue); NativeSessions(inner);
    /// 打开文档；options 是 OpenOptions JSON。
    open(bytes: &[u8], options: Option<String>) -> String => open(bytes, options.as_deref());
    /// 单向模型 JSON，支持预算选项。
    document(id: &str, options: Option<String>) -> String => document(id, options.as_deref());
    /// 原子应用 EditOp JSON。
    apply(id: &str, op: &str, context: Option<String>) -> String => apply(id, op, context.as_deref());
    /// 函数式保存；结果由调用方写盘。
    save(id: &str, options: Option<String>) -> Vec<u8> => save(id, options.as_deref());
    /// 按句柄取媒体原字节。
    media(id: &str, media: u32) -> Vec<u8> => media(id, media);
    /// 按字节及 MIME 去重加入媒体。
    #[wasm_bindgen(js_name = addMedia)]
    add_media(id: &str, bytes: &[u8], mime: &str) -> u32 => add_media(id, bytes, mime);
    /// 批量 run 查询，ids 为 JSON 数组，缺省主 part。
    #[wasm_bindgen(js_name = resolveRuns)]
    resolve_runs(id: &str, ids: &str, part: Option<u32>) -> String => resolve_runs(id, ids, part);
    /// 批量段落查询。
    #[wasm_bindgen(js_name = resolveParas)]
    resolve_paras(id: &str, ids: &str, part: Option<u32>) -> String => resolve_paras(id, ids, part);
    /// 批量单元格查询。
    #[wasm_bindgen(js_name = resolveCells)]
    resolve_cells(id: &str, ids: &str, part: Option<u32>) -> String => resolve_cells(id, ids, part);
    /// 批量节查询，隐式节用 sectionIndex。
    #[wasm_bindgen(js_name = resolveSections)]
    resolve_sections(id: &str, ids: &str, part: Option<u32>) -> String => resolve_sections(id, ids, part);
    /// 批量表格视图查询。
    #[wasm_bindgen(js_name = resolveTable)]
    resolve_table(id: &str, ids: &str, part: Option<u32>) -> String => resolve_table(id, ids, part);
    /// 只读调试出口：当前 part 字节。
    #[wasm_bindgen(js_name = partBytes)]
    part_bytes(id: &str, part: u32) -> Vec<u8> => part_bytes(id, part);
    /// 只读调试出口：节点子树 XML。
    #[wasm_bindgen(js_name = nodeXml)]
    node_xml(id: &str, node: u32, part: Option<u32>) -> String => node_xml(id, node, part);
    /// 累计诊断与成功提交的 XML 逃生口计数。
    diagnostics(id: &str) -> String => diagnostics(id);
}

#[test]
fn bind_01_wasm_shell_native_lifecycle() {
    let bytes = rsword::save::blank_docx(None).unwrap();
    let mut table = NativeSessions::new();
    let id = table.open(&bytes, None).unwrap();
    assert!(table.document(&id, None).unwrap().contains("totalBlocks"));
    assert_eq!(table.save(&id, None).unwrap(), bytes);
    assert_eq!(table.resolve_runs(&id, "[]", None).unwrap(), "[]");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&table.version()).unwrap()["protocol"],
        "native/0"
    );
    table.close(&id);
    table.close(&id);
}
