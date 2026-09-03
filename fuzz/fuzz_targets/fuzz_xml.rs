//! TEST-06 `fuzz_xml`：任意字节作为 part。不 panic；解析成功时 `serialize == input`（Clean），
//! 转码 part 除外（XML-01：只能与转码后字节一致）。
#![no_main]

use libfuzzer_sys::fuzz_target;
use rsword::package::PartId;
use rsword::save::serialize;
use rsword::xml::Dom;

fuzz_target!(|data: &[u8]| {
    if let Ok(dom) = Dom::parse(PartId(0), data) {
        let out = serialize(&dom).expect("clean DOM serializes");
        if !dom.transcoded() {
            assert_eq!(out, data, "clean roundtrip must be byte-identical");
        }
        // 只读遍历也不得 panic
        let mut n = 0usize;
        for id in dom.descendants(dom.root()) {
            n += dom.semantic_children(id).count();
            let _ = dom.namespace_scope(id);
        }
        let _ = n;
        let _ = dom.check_dirty_invariants();
    }
});
