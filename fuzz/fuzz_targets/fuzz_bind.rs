//! TEST-06/10：任意 JSON 经过真实会话边界；错误与只读查询不得改变可观察状态。
//! 一行一个请求，允许同会话多步编辑；畸形 JSON 原样送入，不先过滤有效输入。
#![no_main]
use libfuzzer_sys::fuzz_target;
use rsword::bind::native::{ApiError, SessionTable};

const SEEDS: [&[u8]; 5] = [
    include_bytes!("../../corpus/synthetic/bidi__007.docx"),
    include_bytes!("../../corpus/synthetic/bugfix-regressions__006.docx"),
    include_bytes!("../../corpus/synthetic/bookmarks-crossref__006.docx"),
    include_bytes!("../../corpus/synthetic/revisions__001.docx"),
    include_bytes!("../../corpus/synthetic/table-revisions__001.docx"),
];

type Observed<T> = Result<T, (String, String)>;
fn observed<T>(value: Result<T, ApiError>) -> Observed<T> {
    value.map_err(|e| (e.code, e.message))
}
fn state(
    t: &mut SessionTable,
    id: &str,
) -> (Observed<String>, Observed<String>, Observed<Vec<u8>>) {
    (observed(t.document(id, None)), observed(t.diagnostics(id)), observed(t.save(id, None)))
}

// 查询成功也必须只读；每个入口都比较完整 JSON、诊断/逃生计数与保存字节。
macro_rules! query {
    ($table:ident, $id:ident, $before:ident, $json:ident; $($method:ident),+ $(,)?) => {
        $(let _ = $table.$method(&$id, $json, None);
          assert_eq!(state(&mut $table, &$id), $before, stringify!($method));)+
    };
}

fuzz_target!(|data: &[u8]| {
    let mut table = SessionTable::default();
    let id =
        table.open(SEEDS[data.first().copied().unwrap_or(0) as usize % SEEDS.len()], None).unwrap();
    let text = String::from_utf8_lossy(data);
    for json in text.split('\n').take(8) {
        let before = state(&mut table, &id);
        if table.apply(&id, json, None).is_err() {
            assert_eq!(state(&mut table, &id), before, "apply Err changed state");
        }
        let before = state(&mut table, &id);
        let _ = table.document(&id, Some(json));
        assert_eq!(state(&mut table, &id), before, "document changed state");
        query!(table, id, before, json; resolve_runs, resolve_paras, resolve_cells, resolve_sections, resolve_table);
    }
});
