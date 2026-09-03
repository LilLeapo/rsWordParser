//! TEST-06 `fuzz_zip`：任意字节作为 docx。不 panic；要么 `Err`，要么成功打开，且无编辑保存字节相同。
#![no_main]

use libfuzzer_sys::fuzz_target;
use rsword::package::Package;

fuzz_target!(|data: &[u8]| {
    if let Ok(mut pkg) = Package::open(data) {
        let saved = pkg.save().expect("unedited save succeeds");
        assert_eq!(saved.as_slice(), data, "invariant 1: unedited save returns the input bytes");
        let ids: Vec<_> = pkg.parts().iter().map(|p| p.id).collect();
        for id in ids {
            let _ = pkg.dom(id);
            let _ = pkg.namespace_context(id);
        }
    }
});
