# SPEC 09 · 保存（save/）

对应 `docs/03` 第 9 节。职责：校验规范状态、物化 Span、无损序列化脏 part、写回包。

## SAVE-01 流程

```
save(session, opts):
  1. 若无任何脏节点、无新增/删除 part、opts 无变更请求 → 返回 session.original_bytes（不变式 1）
  2. validate_all()            → Vec<Diagnostic>；EngineInvariantViolation 按 SAVE-02 处理
  3. materialize_spans()       → SPAN-08（可能产生新的脏节点）
  4. apply_save_options()      → core.xml 时间、removePersonalInformation 等（作为普通 DOM 变更）
  5. for part in dirty_parts: bytes = serialize(part)   → XML-13
  6. write_package()           → SAVE-06
```

步骤 3–4 也经 `MutationPlan`，保证同一套脏标记规则。

## SAVE-02 校验与来源

| 检查 | 失败处理（PreExistingDamage） | 失败处理（EngineInvariantViolation） |
| --- | --- | --- |
| Span 起终点成对、同 `FlowId`、起在终前（`SPAN-09`） | 孤儿删除 / 反序交换，记诊断 | 调试与 CI：`Err(SAVE_INVARIANT)`；发布：同左并记诊断 |
| 字段 begin/separate/end 顺序与嵌套（`FLD-13`） | 未闭合字段保持原字节 | 同上 |
| `r:*` 引用在所在 part 的 `.rels` 中存在 | 记诊断，保留 | 同上 |
| `[Content_Types].xml` 覆盖所有 part；`.rels` 目标存在 | 补 Default/Override，记诊断 | 同上 |
| 所有使用的前缀在其作用域内已绑定（`XML-11`） | 记诊断（输入本就如此） | 同上 |
| 属性容器子元素顺序符合 `PROP-05`（仅检查 `New/SelfDirty` 节点） | 不适用 | 同上 |
| 修订 `w:id` 全局唯一 | 记诊断 | 同上 |
| `New` / 脏 `w:tbl` 的每行网格宽度（`gridBefore + Σ gridSpan + gridAfter`）= `tblGrid` 列数（`SAVE_TABLE_GRID`，M3） | 记诊断，保留（输入本就如此） | 同上 |
| 段落至少含 `w:pPr` 之外的合法结构（空段允许） | — | — |

来源判定：解析阶段记录的缺陷集合为 `PreExisting`；保存时新出现且不在该集合中的为 `EngineInvariantViolation`。

## SAVE-03 序列化

按 `XML-13`。补充规则：

- **`w:t` / `w:delText` / `w:instrText`**：`New` 或 `SelfDirty` 时**一律**写 `xml:space="preserve"`（`Clean` 原字节不动）。
- **属性容器**：`PROP-06` 计划已把变化落到子元素，容器自身为 `DescendantDirty`，其开闭标签原字节。
- **命名空间**：`New` 子树的前缀按目标位置 `Scope` 与 `NamespaceContext`；未绑定前缀在子树根声明（`XML-14`）。需要出现在 `mc:Ignorable` 的前缀（`w14 w15 w16* wp14`）在根 `mc:Ignorable` 缺失时追加，根 `SelfDirty`。
- **flavor**：`New` 节点的 QName URI 族与属性编解码按 `PartFlavor`（`PROP-02`）；Strict part 中**禁止**生成 VML（水印等需 VML 的功能在 Strict 下返回 `Err(SAVE_STRICT_NO_VML)` 或用 DrawingML 替代）。
- **实体与非法字符**：`XML-06`。
- **自闭合**：无子节点的 `New/SelfDirty` 元素写 `/>`。
- 输出必须是良构 XML；调试构建下对每个脏 part 做一次快速良构性扫描（不解析成树）。

## SAVE-04 属性容器合并

见 `PROP-06`。唯一补充：`w:pPr` 中 `w:rPr`（段落标记）与 `w:sectPr`、`w:pPrChange` 的相对顺序固定为 `…, rPr, sectPr, pPrChange`；`rPr` 内 `w:ins/w:del/w:moveFrom/w:moveTo`（段落标记修订）在其他 rPr 子元素之前。

## SAVE-05 新 part 与关系

- 新 part 命名见 `EDIT-06`；内容类型：`header/footer` → `application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml` / `footer+xml`；`comments` → `…comments+xml`；`commentsExtended` → `application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtended+xml`（`w15`）；`footnotes/endnotes` → `…footnotes+xml`/`…endnotes+xml`；`chart` → `application/vnd.openxmlformats-officedocument.drawingml.chart+xml`；媒体按扩展名 `Default`。
- 关系类型按目标 part 的 flavor 选族（`PKG-07`）。
- 新 part 的根元素命名空间声明：复制主 part 根的声明集合（保证 `w14` 等前缀一致），加 `mc:Ignorable` 同值。
- `[Content_Types].xml` 与 `.rels` 的修改走同一 DOM 机制（它们也是 XML part），因此也满足"未变部分原字节"。

## SAVE-06 包写回

- 遍历原 zip 条目**按原顺序**：未变 part → 原压缩数据直接拷贝（`zip` crate `raw_copy_file`，不解压不重压），条目名、方法、CRC 不变；变脏 part → 用 Deflate 写新数据；删除的 part 跳过。
- 新 part 追加在末尾（`[Content_Types].xml` 保持原位置）。
- 包级元数据（本地头 extra 字段、时间戳、通用标志、central directory 布局）**不保证**逐字节一致（不变式 2 的措辞）。
- `0x7075` 字段：原条目通过 raw copy 保留其本地头原样；central directory 由 writer 重建，不再包含被中和的字段（Word 忽略它，无影响）。

## SAVE-07 保存选项

`SaveOptions { saved_at: Option<Timestamp> /* core.xml dcterms:modified */, remove_personal_info: Option<bool>, section: …, header/footer: …, page_color: … }` 与 TS `SaveOptions` 对齐；每项都翻译为 `EditOp`/DOM 变更，没有旁路。

`removePersonalInformation`（设置或文档标志为 true 时）：修订与批注的 `w:author` 改为 `Author`、`w:date` 删除；`core.xml` 的 creator/lastModifiedBy 清空；与 TS 行为对齐。

## SAVE-08 不变式验证（实现内自检，调试构建）

- 不变式 1：无脏节点时 `save` 不进入序列化路径（断言）。
- 不变式 2：对每个脏 part，收集所有 `Clean` 节点的 `lex.range` 字节，断言每段都是输出的子串（可抽样）。
- 不变式 3：`Protected(Unparseable|TooDeep)` 节点在保存后仍为 `Clean`。

## 验收清单

| ID | 用例 |
| --- | --- |
| SAVE-01 | 打开→保存 → 字节相同（全部语料） |
| SAVE-02 | 人为孤儿 `bookmarkEnd` 文档保存成功且诊断为 PreExisting；测试中注入 Span 破坏 → CI 报错 |
| SAVE-03 | 改一个字 → 该 `w:t` 带 preserve，段落其他 run 原字节；Strict 文档改字后仍为 Strict 且 `ST_OnOff` 为 `true/false` |
| SAVE-05 | 首次添加批注 → `comments.xml`、`.rels`、`[Content_Types].xml` 正确且其他条目原压缩数据不变 |
| SAVE-06 | 改动一个 part 后，其他条目的 CRC 与压缩字节与原文件相同 |
| SAVE-07 | `remove_personal_info` 后无 `w:author` 非 `Author` 的修订 |
