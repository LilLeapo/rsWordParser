//! L0 包层（`spec/01-package.md`，`docs/03` §3）。
//!
//! 把 `.docx` 字节变成 part 图与关系图，判定 flavor，提供每个 part 的 [`NamespaceContext`]
//! 与媒体访问。本层不理解 WordprocessingML 语义。
//!
//! M0 任务：0.3 zip 读取（`PKG-01/02/11`）、0.4 内容类型与关系（`PKG-04..07`）、
//! 0.5 主 part 与 flavor（`PKG-03/08/09`）。
//!
//! [`NamespaceContext`]: docs/03 §3.4，任务 0.5

/// part 在会话内的稳定编号（zip 条目顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartId(pub u32);

/// `PKG-02` 限额：在解压任何 part 之前按 central directory 声明大小检查。
pub mod limits {
    /// part 数上限（不含目录项）。
    pub const MAX_PARTS: usize = 10_000;
    /// 单 part 解压大小上限：512 MiB。
    pub const MAX_PART_BYTES: u64 = 512 * 1024 * 1024;
    /// 总解压大小上限：1.5 GiB。
    pub const MAX_TOTAL_BYTES: u64 = 3 * 512 * 1024 * 1024;
}

/// OOXML 命名空间族（`spec/00` §0.3）。
///
/// 单个 part 的 flavor 由其根元素绑定的 URI 判定（`PKG-08`）；生成新节点时**按目标 part**
/// 的 flavor 工作，不按包。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartFlavor {
    /// `schemas.openxmlformats.org/.../2006/...`
    Transitional,
    /// `purl.oclc.org/ooxml/...`
    Strict,
}

/// 包级 flavor（`docs/03` §3.2、`PKG-08`）。`Mixed` 是防御性分类，产品只承诺前两者。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageFlavor {
    Transitional,
    Strict,
    Mixed,
}
