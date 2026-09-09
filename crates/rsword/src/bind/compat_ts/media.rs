//! 媒体预取（`COMPAT-03` 的 `imageDataUrl` 等；`spec/15` 任务 4.4）。
//!
//! `ParsedDoc` 要把图片内联成 dataURL，读字节需要 `&mut Package`；而块投影拿的是 DOM 的
//! 不可变借用。所以先扫一遍 part、把它引用的每个媒体关系解析好放进 [`MediaMap`]，
//! 之后整条投影链路只读这张表。TS 的 `resolveBlipMedia` / `tableBlipMedia` 预取是同一个道理
//! （它是因为 JSZip 异步，我们是因为借用）。

use std::collections::HashMap;

use crate::package::{MediaKind, MediaRef, MediaStore, Package, PartId};
use crate::xml::{Dom, LocalName, MceRole, NodeId, NsId, QName};

/// 一个关系 id 解析出的显示用媒体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaOut {
    /// 包内媒体是 `data:<mime>;base64,…`，外链是原始 URL（浏览器自己去取）。
    pub url: String,
    /// 外链（`TargetMode="External"` 或 `http(s)://`）。
    pub external: bool,
    /// 包内媒体的类别；外链为 `None`。
    pub kind: Option<MediaKind>,
}

/// 一个 part 里「关系 id → 媒体」的预取表。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MediaMap {
    by_rid: HashMap<String, MediaOut>,
}

impl MediaMap {
    /// 扫 `part` 的 DOM，把所有 `a:blip/@r:embed`、`a:blip/@r:link`、`v:imagedata/@r:id`
    /// 一次解析完。解析不出来的关系不进表——调用方据此输出 `brokenImage`。
    pub fn build(pkg: &mut Package, part: PartId) -> MediaMap {
        let mut rids: Vec<String> = Vec::new();
        if let Ok(Some(dom)) = pkg.dom(part) {
            collect_rids(dom, &mut rids);
        }
        rids.sort();
        rids.dedup();

        let mut store = MediaStore::new();
        let mut by_rid = HashMap::new();
        for rid in rids {
            match store.resolve(pkg, part, &rid) {
                Ok(MediaRef::External(url)) => {
                    by_rid.insert(rid, MediaOut { url, external: true, kind: None });
                }
                Ok(MediaRef::Media(id)) => {
                    let kind = store.get(id).kind;
                    if let Ok(url) = store.data_url(pkg, id) {
                        by_rid.insert(rid, MediaOut { url, external: false, kind: Some(kind) });
                    }
                }
                Err(_) => {}
            }
        }
        MediaMap { by_rid }
    }

    pub fn get(&self, rid: &str) -> Option<&MediaOut> {
        self.by_rid.get(rid)
    }

    /// `embed` 优先、`link` 次之（TS `extractImage` 的 `r:embed ?? r:link`）。
    pub fn pick(&self, embed: Option<&str>, link: Option<&str>) -> Option<&MediaOut> {
        embed.and_then(|r| self.get(r)).or_else(|| link.and_then(|r| self.get(r)))
    }

    pub fn is_empty(&self) -> bool {
        self.by_rid.is_empty()
    }
}

fn collect_rids(dom: &Dom, out: &mut Vec<String>) {
    // 语义遍历：未生效的 `mc:Choice` 里的坏 rId 不算（见 `Dom::semantic_descendants`）。
    for n in dom.semantic_descendants(dom.root()) {
        push_rids(dom, n, out);
    }
    // 例外：chartex 配的回退图在**未生效**的 `mc:Fallback` 里，可它正是要画的那张图（`R12` → `Image`，
    // `DrawingFacts::fallback_picture`）。只放行 Choice 里是 `cx:chart` 的那些 Fallback，别的仍按语义遍历。
    for ac in dom.descendants(dom.root()) {
        let Some(e) = dom.element(ac) else { continue };
        if e.mce.role != MceRole::AlternateContent {
            continue;
        }
        let branches = dom.children(ac).iter().copied();
        let chartex_choice = branches.clone().any(|c| {
            dom.element(c).is_some_and(|b| b.mce.role == MceRole::Choice && b.mce.active)
                && dom.descendants(c).any(|n| {
                    dom.name(n).is_some_and(|q| q.local == LocalName::Chart)
                        && dom.is_ns(n, NsId::Cx, "cx")
                })
        });
        if !chartex_choice {
            continue;
        }
        for fb in
            branches.filter(|&c| dom.element(c).is_some_and(|b| b.mce.role == MceRole::Fallback))
        {
            for n in dom.descendants(fb) {
                push_rids(dom, n, out);
            }
        }
    }
}

/// 一个元素上的媒体关系 id：`a:blip/@r:embed | @r:link`、`v:imagedata/@r:id`。
fn push_rids(dom: &Dom, n: NodeId, out: &mut Vec<String>) {
    let Some(name) = dom.name(n) else { return };
    // 前缀未绑定时按字面量兜底（见 `Dom::is_ns`）：语料里有不声明 `xmlns:v` 的文档。
    let attrs: &[LocalName] = match name.local {
        LocalName::Blip if dom.is_ns(n, NsId::A, "a") => &[LocalName::Embed, LocalName::Link],
        LocalName::Imagedata if dom.is_ns(n, NsId::V, "v") => &[LocalName::Id],
        _ => return,
    };
    for &a in attrs {
        if let Some(v) = dom.attr_value(n, QName::new(NsId::R, a)) {
            let v = v.trim();
            if !v.is_empty() {
                out.push(v.to_string());
            }
        }
    }
}

/// 一次投影要的全部媒体预取：主 part 一张，辅助 part（页眉页脚…）各一张。
///
/// 图片关系是**按 part** 解析的（`PKG-05`：页眉里的 `r:embed` 查的是 `header1.xml.rels`），
/// 所以不能共用一张表。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MediaSet {
    pub main: MediaMap,
    pub aux: std::collections::BTreeMap<PartId, MediaMap>,
}

impl MediaSet {
    /// 主 part + 给定的辅助 part 各扫一遍。
    pub fn build(pkg: &mut Package, main: PartId, aux: &[PartId]) -> MediaSet {
        let mut set = MediaSet { main: MediaMap::build(pkg, main), ..Default::default() };
        for &p in aux {
            set.aux.insert(p, MediaMap::build(pkg, p));
        }
        set
    }

    /// 某个 part 的表；没预取过就给空表（图片一律输出 `brokenImage`）。
    pub fn part(&self, part: PartId) -> &MediaMap {
        self.aux.get(&part).unwrap_or(&EMPTY)
    }
}

/// `MediaSet::part` 的空表（没预取过的 part）。
static EMPTY: std::sync::LazyLock<MediaMap> = std::sync::LazyLock::new(MediaMap::default);
