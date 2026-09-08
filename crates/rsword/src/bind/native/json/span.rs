//! 范围与字段的 JSON 投影（`BIND-02`；`SPAN-01`–`SPAN-05`、`FLD-01`–`FLD-07`）。
//!
//! `SpanIndex` / `FieldIndex` 本身不写表：`Document` 表经 `spans()` / `fields()` 访问器投影为
//! `Vec<RangeSpan>` / `Vec<FieldSpan>`；索引的倒排表与诊断不进这里（诊断走 `BIND-07` 的
//! [`Diagnostic`](crate::diag::Diagnostic)，见 `json/diag.rs`）。`RangeClass` / `SpanEnd`
//! 是内部配对用类型，不投影。
//!
//! 单位说明：`Anchor.index` 是**内容序列边界**（`0..=content_len(container)`，`SPAN-02`），
//! 不是 `spec/00` §0.4 的 UTF-16 坐标流偏移。

use crate::package::PartId;
use crate::span::{
    Affinity, Anchor, FieldForm, FieldId, FieldPolicy, FieldSpan, FlowId, InstrToken, Instruction,
    Keyword, RangeKind, RangeSpan, RevisionMeta, SpanId, SpanOrigin,
};
use crate::xml::NodeId;

use super::{as_str_json, json_str_enum, model_json};

// ---- 范围（`SPAN-02` / `SPAN-03`） ---------------------------------------------------------------

json_str_enum! {
    /// 锚点在边界处插入内容时的去向（`SPAN-02`）：`Left` 吸附左侧内容，`Right` 吸附右侧。
    Affinity test json_fields_cover_affinity {
        Left => "left";
        Right => "right";
    }
}

json_str_enum! {
    /// 范围是解析出来的还是本次会话新建的（`SPAN-09` 校验失败时 `origin` 的判定依据）。
    SpanOrigin test json_fields_cover_span_origin {
        Parsed => "parsed";
        Damaged => "damaged";
        New => "new";
    }
}

model_json! {
    /// 附着在 DOM 上的位置（`SPAN-02`）：容器 + 内容序列边界 + affinity。
    struct Anchor(cx) test json_fields_cover_anchor {
        container => "container", NodeId = container;
        /// 内容序列边界，**不是** UTF-16 偏移；标记自身不计入。
        index => "index", u32 = index;
        affinity => "affinity", Affinity = affinity;
        /// 物理标记元素；新建范围与字段边界没有（`FLD`）。
        opt marker => "marker", NodeId = marker;
    }
}

model_json! {
    /// 范围种类与它携带的文档事实（`SPAN-03`，`docs/03` §5.3）；`id` 是配对键（`w:id` 原值）。
    enum RangeKind(cx) test json_fields_cover_range_kind {
        /// `w:bookmarkStart` / `w:bookmarkEnd`。
        Bookmark { id, name, hidden, cols } => "bookmark" {
            id => "id", String = id;
            name => "name", String = name;
            /// `_` 前缀（`_GoBack` / `_Toc…`）：Word 不在书签列表里显示。
            flag hidden => "hidden" = hidden;
            /// `w:colFirst` / `w:colLast`：范围在表格中覆盖的列区间 `[first, last]`。
            opt cols => "cols", (u32, u32) = cols;
        };
        /// `w:commentRangeStart` / `w:commentRangeEnd`。
        Comment { id, reference } => "comment" {
            id => "id", String = id;
            /// 承载 `w:commentReference` 的 run（`SPAN-03` / `SPAN-04` 第 5 条）。
            opt reference => "reference", NodeId = reference;
        };
        /// `w:permStart` / `w:permEnd`。
        Permission { id, editor, group, cols } => "permission" {
            id => "id", String = id;
            opt editor => "editor", String = editor;
            opt group => "group", String = group;
            opt cols => "cols", (u32, u32) = cols;
        };
        /// `w:moveFromRangeStart` / `w:moveFromRangeEnd`。
        MoveFrom { id, name, meta } => "moveFrom" {
            id => "id", String = id;
            name => "name", String = name;
            meta => "meta", RevisionMeta = meta;
        };
        /// `w:moveToRangeStart` / `w:moveToRangeEnd`。
        MoveTo { id, name, meta } => "moveTo" {
            id => "id", String = id;
            name => "name", String = name;
            meta => "meta", RevisionMeta = meta;
        };
        /// `w:customXmlInsRangeStart` / `w:customXmlInsRangeEnd`。
        CustomXmlIns { id, meta } => "customXmlIns" {
            id => "id", String = id;
            meta => "meta", RevisionMeta = meta;
        };
        /// `w:customXmlDelRangeStart` / `w:customXmlDelRangeEnd`。
        CustomXmlDel { id, meta } => "customXmlDel" {
            id => "id", String = id;
            meta => "meta", RevisionMeta = meta;
        };
        /// `w:customXmlMoveFromRangeStart` / `w:customXmlMoveFromRangeEnd`。
        CustomXmlMoveFrom { id, meta } => "customXmlMoveFrom" {
            id => "id", String = id;
            meta => "meta", RevisionMeta = meta;
        };
        /// `w:customXmlMoveToRangeStart` / `w:customXmlMoveToRangeEnd`。
        CustomXmlMoveTo { id, meta } => "customXmlMoveTo" {
            id => "id", String = id;
            meta => "meta", RevisionMeta = meta;
        };
    }
}

model_json! {
    /// 一个范围（`SPAN-03`）。`start` / `end` 缺席 = 该端在 part 内缺失（损坏输入：孤儿终点 /
    /// 未闭合起点），由 `SPAN-09` 在保存前按 `PreExistingDamage` 修复。
    struct RangeSpan(cx) test json_fields_cover_range_span {
        id => "id", SpanId = id;
        part => "part", PartId = part;
        flow => "flow", FlowId = flow;
        kind => "kind", RangeKind = kind;
        origin => "origin", SpanOrigin = origin;
        /// 文件里没有物理标记元素（`SPAN-08` 物化不得为它插入标记）。
        flag implicit => "implicit" = implicit;
        /// 本次会话按 `SPAN-07` 整体删除；保留在索引里供撤销，物化与校验跳过。
        flag removed => "removed" = removed;
        opt start => "start", Anchor = start;
        opt end => "end", Anchor = end;
    }
}

// ---- 字段（`FLD-01`–`FLD-07`） --------------------------------------------------------------------

json_str_enum! {
    /// 字段策略（`FLD-06` / `FLD-07`）：决定显示形态、可编辑性与保存行为。
    FieldPolicy test json_fields_cover_field_policy {
        Marker => "marker";
        Atom => "atom";
        Link => "link";
        Form => "form";
        Picture => "picture";
        Object => "object";
        Block => "block";
        Unknown => "unknown";
    }
}

// `Keyword` 不是 `named_enum!`（`span/field/instr.rs` 的 `keywords!` 宏一处生成变体、`as_str`
// 与 `policy`，变体极多），但 `as_str` 语义相同（规范大写写法；`Unknown` 返回原文）——
// 字串即 JSON 值（`BIND-02`）。`Unknown` 是开放集合，schema 不枚举。
as_str_json!(Keyword);

model_json! {
    /// 指令的语义视图（`FLD-05`）：只做 tokenization。保存真相是 `form` 里指令 run 的原字节，
    /// **禁止**用本投影重新序列化指令（开关 `\r` `\h` `\* MERGEFORMAT` 会丢）。
    struct Instruction(cx) test json_fields_cover_instruction {
        keyword => "keyword", Keyword = keyword;
        tokens => "tokens", Vec<InstrToken> = tokens;
        /// `instr_nodes` 里所有 `w:instrText` / `w:delInstrText` 的文本按序拼接，不 trim。
        raw => "raw", String = raw;
    }
}

model_json! {
    /// 指令里的一项（`FLD-05`）。
    enum InstrToken(cx) test json_fields_cover_instr_token {
        /// 裸词（`bare`）。
        Word(String) as "text" => "word";
        /// 引号串，已去掉外层引号并解掉 `\"` / `\\`。
        Quoted(String) as "text" => "quoted";
        /// `\h` `\o "1-3"` …；`arg` 缺席 = 开关无实参（下一项是开关或指令到头）。
        Switch { name, arg } => "switch" {
            /// 开关字符（`char`，投单字符字串）。
            raw name => "name", String = name.to_string();
            opt arg => "arg", Box<InstrToken> = arg;
        };
        /// 通用格式开关 `\*` `\#` `\@` `\!`（`FLD-05`）。
        GeneralFormat { kind, arg } => "generalFormat" {
            ~ kind => "formatKind", String = &kind.to_string(), "与变体内标签键 kind 撞名（同 SegmentKind::Br 的 breakKind）；char 无 ToJson impl，投单字符字串";
            arg => "arg", String = arg;
        };
        /// 嵌套字段占位（指令文本里的 `U+FFFC`，`FLD-03`）。
        Nested(FieldId) as "field" => "nested";
    }
}

model_json! {
    /// 字段的两种形式（`FLD-01`）；保存真相是这里的节点（`FLD-05`）。
    enum FieldForm(cx) test json_fields_cover_field_form {
        /// 复杂字段：三个含 `w:fldChar` 的 `w:r`（`separate` 可缺省）。
        Complex { begin, separate, end, instr_nodes, result_nodes } => "complex" {
            begin => "begin", NodeId = begin;
            opt separate => "separate", NodeId = separate;
            end => "end", NodeId = end;
            /// begin 与 separate（或 end）之间的 run。
            instr_nodes => "instrNodes", Vec<NodeId> = instr_nodes;
            /// separate 与 end 之间的 run（跨段时横跨多个段落）。
            result_nodes => "resultNodes", Vec<NodeId> = result_nodes;
        };
        /// `w:fldSimple[@w:instr]`，子节点即结果。
        Simple { node, result_nodes } => "simple" {
            node => "node", NodeId = node;
            result_nodes => "resultNodes", Vec<NodeId> = result_nodes;
        };
    }
}

model_json! {
    /// 一个字段（`FLD-02`，`docs/03` §5.4）。字段正确嵌套（与范围不同）。
    struct FieldSpan(cx) test json_fields_cover_field_span {
        id => "id", FieldId = id;
        part => "part", PartId = part;
        flow => "flow", FlowId = flow;
        form => "form", FieldForm = form;
        /// 语义视图；保存真相是 `form` 里的节点（`FLD-05`）。
        instr => "instr", Instruction = instr;
        /// 指令区或结果区内的子字段。
        nested => "nested", Vec<FieldId> = nested;
        opt parent => "parent", FieldId = parent;
        policy => "policy", FieldPolicy = policy;
        /// `w:fldChar/@w:fldLock`：`UpdateBlockField` 拒绝（`FLD-07`）。
        flag lock => "lock" = lock;
        /// `w:fldChar/@w:dirty`：Word 打开时会重算。
        flag dirty_flag => "dirtyFlag" = dirty_flag;
        /// begin run 里的 `w:ffData`（表单域定义，`FLD-10`）。
        opt ff_data => "ffData", NodeId = ff_data;
        /// 指令里有 `w:delInstrText`：指令被修订删除（`MOD-09`）。
        flag instr_deleted => "instrDeleted" = instr_deleted;
        /// begin 与 end 不在同一个 `w:p` 里（`FLD-06` 覆盖规则：一律 `Block`）。
        flag cross_paragraph => "crossParagraph" = cross_paragraph;
    }
}
