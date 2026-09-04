# 属性表（`PROP-01`）

`build.rs`（生成器 `build/props.rs`）读这里的 TOML，生成 `semantic::props` 里的类型与函数（`PROP-07`）。
表只描述**结构**（哪个元素、哪个属性、用哪个 codec、什么顺序）；codec 的解析与写回规则在
`src/semantic/props/codec.rs` 手写（`PROP-02`）。

## 文件

| 文件 | 内容 |
| --- | --- |
| `types.toml` | `[enum.X]` 枚举类型、`[struct.X]` 带属性的元素类型（`w:color`、`w:rFonts` 一类） |
| 其余 `*.toml` | 若干 `[[table]]`：一个属性容器一张表（`w:rPr`、`w:pPr`、`w:pBdr`……） |

## `[enum.Name]`

```toml
[enum.Jc]
doc = "ST_Jc"
values = ["start", "center", "end", "both", "left", "right"]
```

生成 `enum Jc { Start, Center, … }`，变体名由字面首字母大写得到。字面不匹配时读为 `Val::Raw(原文)`
并记 `PROP_BAD_VALUE`，写回原文（`PROP-09`）。

## `[struct.Name]`

```toml
[struct.Color]
doc = "CT_Color"
attrs = [
  { name = "val", attr = "w:val", codec = "HexColorOrAuto" },
  { name = "theme_color", attr = "w:themeColor", codec = "ThemeColor" },
]
```

每个属性一列：`name` 是 Rust 字段名，`attr` 是属性 QName，`codec` 是标量 codec 或枚举名。
`legacy = "w:left"` 声明 Transitional 拼写（`attr` 写 Strict 拼写 `w:start`）：解析两者都接受，
生成按 `PartFlavor` 选拼写。

## `[[table]]`

```toml
[[table]]
name = "RunProps"          # Rust 类型名；函数名用 snake_case：read_run_props / diff_run_props / …
element = "w:rPr"
change = "w:rPrChange"     # 可选：修订快照容器
doc = "…"
order = ["w:rStyle", "w:rFonts", "w:b", …, "w:rPrChange"]   # 容器全部子元素的 schema 顺序，含未建模的

[[table.field]]
name = "bold"              # snake_case
element = "w:b"
codec = "OnOff"            # 标量 codec | 枚举 | struct | 另一张表的名字 | Raw
cs_twin = "bold_cs"        # 可选，PROP-03，只作元数据
in_change = true           # 默认 true：出现在 *PrChange 快照里
multi = false              # true → Vec<T>（w:tab、w:headerReference）
legacy = "w:left"          # 可选，同 struct 的 legacy；须与 element 在 order 里同一格：`"w:start|w:left"`
```

`order` 必须列出容器的**所有** schema 子元素（不只是建模的），这样合并写回时才能把新元素插到
未建模元素（如 `w:sectPr`）之前的正确位置。同一格里的同义对用 `|` 分隔。

codec 列的解析顺序：内建标量（见下表）→ `types.toml` 的枚举 → struct → 另一张 `[[table]]`（嵌套容器，
如 `pPr/rPr`、`pPr/pBdr`）→ `Raw`（整个元素按 DOM 保留，字段类型 `NodeId`）。

| 内建 codec | Rust 值类型 | 备注 |
| --- | --- | --- |
| `OnOff` | `bool` | 字段类型 `Option<bool>` 即 `PROP-04` 三态 |
| `HalfPoints` | `Val<u32>` | `ST_HpsMeasure` |
| `SignedHalfPoints` | `Val<i32>` | `ST_SignedHpsMeasure`（`w:position`） |
| `Twips` | `Val<i32>` | `ST_TwipsMeasure` / `ST_SignedTwipsMeasure` |
| `EighthPoints` | `Val<u32>` | `ST_EighthPointMeasure` |
| `HexColorOrAuto` | `Val<HexColorOrAuto>` | `ST_HexColor` |
| `Hex2` | `Val<u8>` | tint / shade |
| `Percent` | `Val<u32>` | `ST_TextScale` |
| `Str` | `String` | 原文 |
| `Int` / `UInt` | `Val<i32>` / `Val<u32>` | `ST_DecimalNumber` / 无符号 |

## 校验

生成器在构建时检查：字段名是合法 snake_case 且不重复；每个元素都在 `order` 里且不重复建模；
`legacy` 与 `element` 同格；前缀在 `schema/namespaces.tsv`、局部名在 `schema/local_names.txt`
（缺的会一次列全，补进 `local_names.txt` 即可）；codec 名可解析；`cs_twin` 指向本表字段。
