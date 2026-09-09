# SPEC 01 · L0 包层（package）

对应 `docs/03` 第 3 节。职责：把 `.docx` 字节变成 part 图与关系图，判定 flavor，提供每个 part 的命名空间上下文与媒体访问。本层不理解 WordprocessingML 语义。

## PKG-01 zip 读取与 Unicode Path 字段中和

- 读取前**必须**扫描 EOCD → central directory，把每条记录 extra 区中 id `0x7075`（Info-ZIP Unicode Path）的字段 id 改写为 `0xFFFF`，再交给 zip 库。Word 按本地头文件名解析 part，忽略该字段；不中和会让冲突字段把 `word/document.xml` 指向别的条目（POI unicode-path 语料）。
- zip64（count 或 offset 为 `0xFFFF`/`0xFFFFFFFF`）**不**中和，原样处理。
- 中和只作用于读取用的内存副本；无编辑保存返回的是**原始**字节。

验收：unicode-path 语料的 `document.xml` 解析出正确正文；无编辑保存字节相同。

## PKG-02 限额

在解压任何 part 之前，按 central directory 声明的解压大小检查：

| 项 | 上限 | 错误 |
| --- | --- | --- |
| part 数（不含目录项） | 10,000 | `PKG_TOO_MANY_PARTS` |
| 单 part 解压大小 | 512 MiB | `PKG_PART_TOO_LARGE` |
| 总解压大小 | 1.5 GiB | `PKG_TOTAL_TOO_LARGE` |

超限**必须**在读取任何 part 内容前返回 `Err`。

验收：`docs/01` 第 4.1 节所述三个 hostile 用例。

## PKG-03 主 part 定位与非 docx 判定

1. 存在 `word/document.xml` → 主 part。
2. 否则读 `_rels/.rels`，取 `Type` 以 `/officeDocument` 结尾、`TargetMode` 非 External 的关系，目标去前导 `/`；存在即为主 part（LibreOffice 语料有 `word/trial.xml`）。
3. 都没有：若存在 `mimetype` 且以 `application/vnd.oasis.opendocument` 开头 → `Err(NotOoxml::OpenDocument(mime))`；否则 `Err(NotOoxml::MissingMainPart)`。

## PKG-04 内容类型

- 解析 `[Content_Types].xml`：`Default[@Extension → @ContentType]`（扩展名小写比较）与 `Override[@PartName → @ContentType]`。
- part 的内容类型：`Override("/" + uri)` 优先，其次 `Default(ext)`。
- 图片 MIME 判定顺序：扩展名表（png/jpg/jpeg/gif/bmp/webp/svg/emf/wmf/emz/wmz/tif/tiff）→ Override → Default；结果**必须**以 `image/` 开头才视为图片（`media/*.bin` 靠 Override）。
- `[Content_Types].xml` 缺失：诊断 `PKG_NO_CONTENT_TYPES`，继续（Word 会拒绝，但解析仍可进行）。

## PKG-05 关系

```
Relationship { id: String, kind: RelType, target: RelTarget, raw_type: String }
RelTarget    = Internal(PartUri) | External(String)
```

- 每个 part 的关系文件位于 `<dir>/_rels/<name>.rels`；包级为 `_rels/.rels`。
- `TargetMode="External"` → `External(target 原文)`，**禁止**做路径归一化。`http(s)://` 开头但未标 External 的目标也按 External 处理并记诊断 `PKG_EXTERNAL_WITHOUT_MODE`。
- 缺 `Id` 的关系跳过；重复 `Id` 后者覆盖并记诊断。

## PKG-06 路径归一化

`resolve(base: PartUri, target: &str) -> Result<PartUri>`：

1. `target` 以 `/` 开头 → 相对包根，去掉 `/`。
2. 否则 → `dir(base) + "/" + target`。
3. 按 `/` 分段：`.` 丢弃；`..` 弹出上一段；弹空栈 → `Err(PKG_PATH_ESCAPES_ROOT)`。
4. 百分号解码（`%20` 等）后比较；zip 条目名精确匹配，匹配失败再尝试大小写不敏感匹配并记诊断。

这是**唯一**的路径解析函数；`docs/01` 中 6 处各自拼接的逻辑全部归到这里。

## PKG-07 关系类型与双族识别

`RelType` 枚举覆盖：officeDocument、styles、numbering、settings、webSettings、fontTable、theme、header、footer、footnotes、endnotes、comments、commentsExtended、commentsIds、commentsExtensible、people、image、hyperlink、chart、chartUserShapes、package（嵌入工作簿）、diagramData、diagramLayout、diagramQuickStyle、diagramColors、diagramDrawing（`http://schemas.microsoft.com/office/2007/relationships/diagramDrawing`）、customXml、customXmlProps、oleObject、glossaryDocument、coreProperties、extendedProperties、customProperties、thumbnail、Other(String)。

- Transitional 族 `http://schemas.openxmlformats.org/officeDocument/2006/relationships/<tail>` 与 Strict 族 `http://purl.oclc.org/ooxml/officeDocument/relationships/<tail>` **必须**映射到同一枚举值；Strict 的尾部差异：`extendedProperties`（Transitional 为 `extended-properties`）、`customProperties`（`custom-properties`）。
- `raw_type` 保留原文，生成新关系时按 part flavor 选族。

## PKG-08 flavor 判定

- part flavor：XML part 根元素的命名空间 URI 属于 Strict 族 → `Strict`，否则 `Transitional`。非 XML part 无 flavor。
- 包 flavor：主 part 与其 `.rels` 同族 → 该族；所有 XML part 同族则包为该族；否则 `Mixed` 并记诊断 `PKG_MIXED_FLAVOR`。
- `Mixed` 是防御分类：生成时按**目标 part** 的 flavor 工作，不按包。

## PKG-09 NamespaceContext（part 级）

```
NamespaceContext {
  root_decls: Vec<(Prefix, NsId)>,   // 根元素上的 xmlns 声明
  preferred: Map<NsId, Prefix>,      // 该 part 对每个 URI 惯用的前缀（取根声明；同 URI 多前缀取第一个）
  ignorable: Vec<Prefix>,            // 根上 mc:Ignorable
  flavor: PartFlavor,
}
```

- 这是快路径与生成时的偏好来源；**任何与位置相关的判断必须用** `Dom::namespace_scope(node)`（`XML-11`）。
- 规范前缀表（无既有绑定时分配）：`w r a wp wps wpg wpc wpi pic c cx dgm dsp lc m mc v o w10 w14 w15 w16 wp14 xml`。
- 已理解命名空间集合（MCE 选择用，`XML-09`）：`wps wpg wp14 w14 w15 cx c14` 对应 URI，可配置。`c14`（Word 2010 图表扩展）在 M6 6.1 加入：图表 part 的 `c:style` 一律包在 `mc:AlternateContent` 里，Word 2010+ 与 TS 读的都是 `Choice Requires="c14"` 那份。

## PKG-10 MediaStore

```
MediaId(u32)
MediaEntry { id, part: PartId, mime: String, kind: MediaKind /* Raster | Svg | Metafile | Tiff | Unknown */ }
```

- 只登记被关系引用为 image/oleObject/package 的二进制 part；未被引用的 part 仍保留在包中。
- 字节访问惰性；转换（Metafile → PNG、TIFF → PNG）为可插拔 `MediaDecoder` trait，默认实现只做 Raster/Svg 直通，Metafile/TIFF 返回 `Unsupported`（由 TS 侧继续转换，见 `docs/03` 3.5）。
- `External` 关系目标不是媒体，模型层记为外链。

## PKG-11 part 枚举与保留

- part 顺序按 zip 条目顺序保留，写回时不重排（`SAVE-06`）。
- 未知 part（如 `customXml/`、`word/glossary/`、厂商私有目录）原样保留。
- 解析失败的 XML part：记诊断，part 标记为 `Opaque`（无 DOM），保存原字节；若是主 part 则整体 `Err`（`XML-08`）。

## 验收清单

| ID | 用例 |
| --- | --- |
| PKG-01 | POI unicode-path 文档正文正确；无编辑保存字节相同 |
| PKG-02 | 三个 hostile 用例分别命中三种错误 |
| PKG-03 | `word/trial.xml` 主 part；`.odt` 报 OpenDocument；空 zip 报 MissingMainPart |
| PKG-04 | `media/image1.bin` + Override → 图片；无 Override → 非图片 |
| PKG-05/06 | `../media/x.png`、`/word/media/x.png`、`media/x.png` 解析到同一 part；External 超链接不归一化；`../../x` 报 escapes root |
| PKG-07 | Strict 关系类型文档的 header/footer/image 关系正确分类 |
| PKG-08 | Transitional、Strict、混合包分别判定 |
