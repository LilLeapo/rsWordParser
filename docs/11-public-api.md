# 11 · Rust 公共 API 观察期与文档审计（BIND-11）

负责人于 2026-09-09 决定：8.5 采用 `doc(hidden)` 观察版，缩小实际 semver 面推迟到
观察期之后。隐藏项仍可被下游调用；本文件不是“缺省构建只有小面”的证明，门 3 的这半条未达成。
`compat_ts` 的实际 feature 门控另在 8.7 完成，协议仍为 `native/0`。

稳定承诺为 `bind::native` 全部导出与下面列出的根部核心类型。原生模块的公共函数、trait 方法
由 crate 的 `warn(missing_docs)` 常规检查。隐藏祖先下的稳定定义和固有 impl 使用
`cfg_attr(rsword_api_docs, deny(missing_docs))`；审计构建取消祖先隐藏，对观察项允许缺文档，
稳定定义的 `deny` 覆盖它。CI 执行 `RUSTFLAGS="--cfg rsword_api_docs -D warnings" cargo check -p rsword --lib`。

以下清单由 `tests/api_docs.rs` 的 `include_str!` 编入测试。token 扫描遍历整个 `src/`，
稳定定义、同名固有 impl（包括宏模板）、审计注解位置、成文位置集合必须一致。
新增固有 impl 而未加注解，或擅自给清单外项加注解，都必须失败。
trait impl 不能增加固有方法，方法文档继承 trait，故不属于这里的固有 impl 注解清单。
宏里的 `$table` 特指 `bind_export!` 的 sessions 分支，当前两处调用目标均为 `SessionTable`；
同一模板只记一次。序号按同一源文件中该类型的显式声明、随后宏模板扫描次序计，不使用行号。

## 稳定类型

```bind-11-types
src/xml/dom.rs Node
src/xml/mod.rs Dirty
src/span/index.rs Anchor
src/span/index.rs RangeSpan
src/span/field/index.rs FieldSpan
src/edit/mod.rs EditOp
src/edit/mod.rs EditContext
src/edit/plan.rs MutationResult
src/edit/session.rs EditSession
src/error.rs Error
src/diag.rs DiagCode
src/bind/native/error.rs ApiError
src/bind/native/session.rs SessionId
src/bind/native/session.rs SessionTable
src/bind/native/schema.rs SchemaDefs
src/bind/native/json.rs ProjCx
src/bind/native/json.rs DocumentOpts
src/bind/native/json.rs DocumentJson
src/bind/native/json.rs ToJson
src/bind/native/json.rs MediaEntry
src/bind/native/edit/codec.rs EditJsonError
src/bind/native/edit/ops.rs EditOpJson
src/bind/native/edit/payload.rs NewRunJson
src/bind/native/edit/payload.rs NewInlineJson
src/bind/native/edit/payload.rs NewFieldJson
src/bind/native/edit/payload.rs NewAtomJson
src/bind/native/edit/payload.rs NewBlockJson
```

## 值对象豁免

以下字段集合明确固定：Anchor 是位置四元组；ProjCx 是包引用与显示开关；DocumentOpts
只控制完整投影的显示开关（会话预算另行定义）；DocumentJson 是一个 JSON 值的透明包装。
其余结构体和枚举均要求 `non_exhaustive`。SessionId 是不透明字符串别名，ToJson 是 trait，
两者不适用该属性。公开载荷里引用的旧模块类型仍处于观察期，不因引用自动扩大稳定承诺。

```bind-11-values
src/span/index.rs Anchor
src/bind/native/json.rs ProjCx
src/bind/native/json.rs DocumentOpts
src/bind/native/json.rs DocumentJson
```

## 审计注解位置

祖先可见性也锁定，防止改回无条件 `doc(hidden)` 后压掉子项的审计：

```bind-11-ancestors
diag
edit
error
model
package
resolve
save
semantic
span
xml
```

```bind-11-audit
src/bind/native/edit/codec.rs impl:EditJsonError:1
src/bind/native/edit/codec.rs type:EditJsonError
src/bind/native/edit/ops.rs impl:EditOpJson:1
src/bind/native/edit/ops.rs type:EditOpJson
src/bind/native/edit/payload.rs type:NewAtomJson
src/bind/native/edit/payload.rs type:NewBlockJson
src/bind/native/edit/payload.rs type:NewFieldJson
src/bind/native/edit/payload.rs type:NewInlineJson
src/bind/native/edit/payload.rs type:NewRunJson
src/bind/native/error.rs impl:ApiError:1
src/bind/native/error.rs type:ApiError
src/bind/native/exports.rs impl:$table:1
src/bind/native/json.rs type:DocumentJson
src/bind/native/json.rs type:DocumentOpts
src/bind/native/json.rs type:MediaEntry
src/bind/native/json.rs type:ProjCx
src/bind/native/json.rs type:ToJson
src/bind/native/schema.rs impl:SchemaDefs:1
src/bind/native/schema.rs type:SchemaDefs
src/bind/native/session.rs impl:SessionTable:1
src/bind/native/session.rs impl:SessionTable:2
src/bind/native/session.rs type:SessionId
src/bind/native/session.rs type:SessionTable
src/diag.rs impl:DiagCode:1
src/diag.rs type:DiagCode
src/edit/ink_ops.rs impl:EditSession:1
src/edit/media_ops.rs impl:EditSession:1
src/edit/mod.rs impl:EditContext:1
src/edit/mod.rs type:EditContext
src/edit/mod.rs type:EditOp
src/edit/plan.rs impl:MutationResult:1
src/edit/plan.rs type:MutationResult
src/edit/session.rs impl:EditSession:1
src/edit/session.rs type:EditSession
src/error.rs impl:Error:1
src/error.rs type:Error
src/save/prune.rs impl:EditSession:1
src/span/field/index.rs impl:FieldSpan:1
src/span/field/index.rs type:FieldSpan
src/span/index.rs impl:Anchor:1
src/span/index.rs impl:RangeSpan:1
src/span/index.rs type:Anchor
src/span/index.rs type:RangeSpan
src/xml/dom.rs type:Node
src/xml/mod.rs impl:Dirty:1
src/xml/mod.rs type:Dirty
```

## 迁移方式

根部稳定重导出提供统一入口，旧模块路径本版仍可用。EditContext 使用 `Default` 与
`with_*` 方法构造；MutationResult 由 apply 返回，也可从 Default 填充公开字段。
新增字段允许演进，不依赖穷尽结构体字面量。所有破坏性变更须在 crate minor 版本记录迁移路径；
DiagCode 的既有变体名和机器码只保留、只追加，由 `tests/public_api.rs` 的发布表锁定。
