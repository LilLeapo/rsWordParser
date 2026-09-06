//! 媒体解析（`PKG-05`，`spec/15` 任务 4.1）的语料普查。
//!
//! 对每一份语料的**每个 XML part**，找出所有 `a:blip/@r:embed`、`a:blip/@r:link` 与
//! `v:imagedata/@r:id`，用**该 part 自己的 rels** 解析。DoD：未命中的只有真正缺关系的用例。

mod common;

use std::collections::BTreeMap;

use rsword::package::{MediaKind, MediaMiss, MediaRef, MediaStore, Package, PartId};
use rsword::xml::{LocalName, NsId, QName};

/// 文档本身就坏的引用：关系表里没有这个 id，TS 同样解析不出（对照过每份的 `expected.json`）。
/// 形如 (文档名, 未命中数)。数目变了要重新对照 TS，不是放宽。
const KNOWN_BROKEN: &[(&str, usize)] = &[
    // `a:blip r:embed` 指向不存在的 rId → TS `brokenImage: true`
    ("bugfix-regressions__015.docx", 1),
    ("bugfix-regressions__016.docx", 1),
    // 页眉里的坏引用；TS 的 `headerImages` 为 null
    ("resource-cleanup__004.docx", 1),
    // OLE 预览图的坏引用；TS 给 `label: "Embedded object"` 但没有 imageDataUrl
    ("smartart-ole__010.docx", 1),
    // VML `v:imagedata r:id="rId999"`；TS 当作普通段落
    ("wordart-vml__011.docx", 1),
    // hostile：`..` 越过包根 / 目标 part 不存在（`TEST-09`）
    ("rels-escape-root.docx", 1),
    ("rels-missing-target.docx", 1),
    // hostile：绘图里的关系全是悬空的（`a:blip` 与 `v:imagedata` 各一处；`wps:txbx` 不是媒体）
    ("drawing-missing-rels.docx", 2),
    // hostile：墨迹 run 的 `r:embed` 悬空（M6 6.8 的病态输入，`spec/17`）
    ("ink-garbage.docx", 1),
    // M6 语料 B9：OLE 预览 `v:imagedata r:id` 悬空且段落有文字；TS 保字节不保预览
    ("m6-ole__005.docx", 1),
];

#[derive(Default)]
struct Census {
    docs: usize,
    refs: usize,
    resolved: usize,
    external: usize,
    by_kind: BTreeMap<&'static str, usize>,
    misses: Vec<String>,
}

fn kind_name(k: MediaKind) -> &'static str {
    match k {
        MediaKind::Raster => "raster",
        MediaKind::Svg => "svg",
        MediaKind::Metafile => "metafile",
        MediaKind::Tiff => "tiff",
        MediaKind::Other => "other",
    }
}

/// part 里所有 (元素, 关系属性) 组合的引用。
fn rel_refs(pkg: &mut Package, part: PartId) -> Vec<String> {
    let Ok(Some(dom)) = pkg.dom(part) else { return Vec::new() };
    let mut out = Vec::new();
    // 必须走语义遍历：语料里有把坏 `r:embed` 写在未生效的 `mc:Choice` 里、真图在 Fallback 的文档
    // （`hf-images__010`）。用 `descendants` 会读到未生效的分支。
    for n in dom.semantic_descendants(dom.root()) {
        let Some(name) = dom.name(n) else { continue };
        let attrs: &[QName] = match (name.ns, name.local) {
            (NsId::A, LocalName::Blip) => {
                &[QName::new(NsId::R, LocalName::Embed), QName::new(NsId::R, LocalName::Link)]
            }
            // `o:relid` 是 VML 的另一种写法，语料里没有出现；出现时这里要一并加上。
            (NsId::V, LocalName::Imagedata) => &[QName::new(NsId::R, LocalName::Id)],
            _ => continue,
        };
        for &a in attrs {
            if let Some(v) = dom.attr_value(n, a) {
                let v = v.trim();
                if !v.is_empty() {
                    out.push(v.to_string());
                }
            }
        }
    }
    out
}

#[test]
fn pkg_05_media_resolves_across_the_corpus() {
    let mut c = Census::default();
    let mut docs: Vec<_> = common::docx_paths("synthetic");
    docs.extend(common::docx_paths("hostile"));
    docs.sort();

    for path in docs {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(&path).expect("read docx");
        let Ok(mut pkg) = Package::open(&bytes) else { continue };
        c.docs += 1;
        let mut store = MediaStore::new();
        let parts: Vec<PartId> = pkg.parts().iter().map(|p| p.id).collect();
        for part in parts {
            for rid in rel_refs(&mut pkg, part) {
                c.refs += 1;
                match store.resolve(&pkg, part, &rid) {
                    Ok(MediaRef::Media(id)) => {
                        c.resolved += 1;
                        *c.by_kind.entry(kind_name(store.get(id).kind)).or_default() += 1;
                    }
                    Ok(MediaRef::External(_)) => c.external += 1,
                    Err(e) => c.misses.push(format!(
                        "{file}: part {} rId {rid}: {}",
                        pkg.part(part).uri.as_str(),
                        e.as_str()
                    )),
                }
            }
        }
        // 解析到的媒体都要能读出字节并生成 dataURL。
        let ids: Vec<_> = store.iter().map(|(id, _)| id).collect();
        for id in ids {
            let url = store.data_url(&mut pkg, id).expect("data url");
            assert!(url.starts_with("data:"), "{file}: {url:.40}");
            assert!(url.contains(";base64,"), "{file}: {url:.40}");
        }
    }

    let mut unexpected = Vec::new();
    let mut per_doc: BTreeMap<&str, usize> = BTreeMap::new();
    for m in &c.misses {
        match KNOWN_BROKEN.iter().find(|(d, _)| m.starts_with(d)) {
            Some((d, _)) => *per_doc.entry(d).or_default() += 1,
            None => unexpected.push(m.clone()),
        }
    }
    for (doc, budget) in KNOWN_BROKEN {
        let got = per_doc.get(doc).copied().unwrap_or(0);
        assert_eq!(got, *budget, "{doc}: 已知坏引用数变了（预期 {budget}，实际 {got}）");
    }

    println!(
        "media: {} 份文档，{} 处引用；包内 {}（{:?}），外链 {}，未命中 {}（已知 {}）",
        c.docs,
        c.refs,
        c.resolved,
        c.by_kind,
        c.external,
        c.misses.len(),
        c.misses.len() - unexpected.len()
    );
    assert!(c.refs > 0, "语料里应当有媒体引用");
    assert!(unexpected.is_empty(), "未登记的媒体解析失败：\n{}", unexpected.join("\n"));
}

#[test]
fn pkg_05_media_is_interned_per_part() {
    // 同一张图被两处引用时只登记一次，且第二次解析不重复读字节。
    let path = common::docx_paths("synthetic")
        .into_iter()
        .find(|p| p.file_name().unwrap().to_string_lossy().starts_with("emf-image__001"))
        .expect("emf-image__001.docx");
    let bytes = std::fs::read(&path).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let main = pkg.main_part();
    let refs = rel_refs(&mut pkg, main);
    assert!(!refs.is_empty(), "该用例应当有 a:blip 引用");

    let mut store = MediaStore::new();
    let first = store.resolve(&pkg, main, &refs[0]).expect("resolve");
    let again = store.resolve(&pkg, main, &refs[0]).expect("resolve");
    assert_eq!(first, again);
    assert_eq!(store.len(), 1);

    let MediaRef::Media(id) = first else { panic!("应当是包内媒体") };
    // `docs/03` §3.5：EMF 不在 Rust 侧转换，只标 kind。
    assert_eq!(store.get(id).kind, MediaKind::Metafile);
    assert!(store.get(id).kind.needs_conversion());
    let url = store.data_url(&mut pkg, id).unwrap();
    assert!(url.starts_with("data:image/x-emf;base64,"), "{url:.40}");

    assert_eq!(store.resolve(&pkg, main, "rIdNope"), Err(MediaMiss::NoRel));
}
