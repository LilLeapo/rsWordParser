//! 独立参考模型（handoff §5.2–5.4）：不用生产 locate / edit / XML codec / resolve 计算预期。
//!
//! 模型只表示测试所需的短段落：逐 Unicode scalar 的字符、直接 `bold` / `italic`、段落 `jc`。
//! 坐标一律按 UTF-16 code unit；插入位置和删除区间先全部穷举，再跑有状态 1–4 步序列和随机
//! save/reopen 序列。实际投影仍从公开 `EditSession` 读取，模型预期在测试侧独立计算。

mod common;

use common::Rng;
use rsword::diag::DiagCode;
use rsword::edit::{EditContext, EditOp, EditSession, InlinePos, NewInline, NewRun, Utf16Offset};
use rsword::error::Error;
use rsword::model::{Inline, OBJECT_REPLACEMENT, TextBlock};
use rsword::semantic::props::{Change, Jc, ParaPropsPatch, RunPropsPatch, Val};
use rsword::xml::NodeId;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Style {
    bold: Option<bool>,
    italic: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Cell {
    ch: char,
    style: Style,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ParaModel {
    cells: Vec<Cell>,
    jc: Option<Val<Jc>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DocModel {
    paras: Vec<ParaModel>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OffsetError {
    Surrogate,
    OutOfRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ModelOp {
    Insert { para: usize, at: u32, text: String, style: Style },
    Delete { para: usize, from: u32, to: u32 },
    SetRunStyle { para: usize, from: u32, to: u32, bold: Change<bool>, italic: Change<bool> },
    SetParaJc { para: usize, jc: Change<Val<Jc>> },
    Replace { para: usize, text: String },
}

impl Cell {
    fn new(ch: char, style: Style) -> Self {
        Self { ch, style }
    }
}

impl ParaModel {
    fn from_runs(runs: &[(&str, Style)]) -> Self {
        let mut cells = Vec::new();
        for (text, style) in runs {
            cells.extend(text.chars().map(|ch| Cell::new(ch, *style)));
        }
        Self { cells, jc: None }
    }

    fn text(&self) -> String {
        self.cells.iter().map(|c| c.ch).collect()
    }

    fn utf16_len(&self) -> u32 {
        self.cells.iter().map(|c| c.ch.len_utf16() as u32).sum()
    }

    fn index_at(&self, offset: u32) -> Result<usize, OffsetError> {
        let mut units = 0u32;
        for (i, cell) in self.cells.iter().enumerate() {
            if units == offset {
                return Ok(i);
            }
            let next = units + cell.ch.len_utf16() as u32;
            if offset > units && offset < next {
                return Err(OffsetError::Surrogate);
            }
            units = next;
        }
        if offset == units { Ok(self.cells.len()) } else { Err(OffsetError::OutOfRange) }
    }

    fn range(&self, from: u32, to: u32) -> Result<std::ops::Range<usize>, OffsetError> {
        if from > to {
            return Err(OffsetError::OutOfRange);
        }
        Ok(self.index_at(from)?..self.index_at(to)?)
    }

    fn insert(&mut self, at: u32, text: &str, style: Style) -> Result<(), OffsetError> {
        let index = self.index_at(at)?;
        let cells: Vec<_> = text.chars().map(|ch| Cell::new(ch, style)).collect();
        self.cells.splice(index..index, cells);
        Ok(())
    }

    fn delete(&mut self, from: u32, to: u32) -> Result<(), OffsetError> {
        let range = self.range(from, to)?;
        self.cells.drain(range);
        Ok(())
    }

    fn set_run_style(
        &mut self,
        from: u32,
        to: u32,
        bold: &Change<bool>,
        italic: &Change<bool>,
    ) -> Result<(), OffsetError> {
        let range = self.range(from, to)?;
        for cell in &mut self.cells[range] {
            match bold {
                Change::Keep => {}
                Change::Unset => cell.style.bold = None,
                Change::Set(v) => cell.style.bold = Some(*v),
            }
            match italic {
                Change::Keep => {}
                Change::Unset => cell.style.italic = None,
                Change::Set(v) => cell.style.italic = Some(*v),
            }
        }
        Ok(())
    }
}

impl DocModel {
    fn two_paragraphs() -> Self {
        Self {
            paras: vec![
                ParaModel::from_runs(&[
                    ("a", Style::default()),
                    ("😀", Style { bold: Some(true), italic: None }),
                ]),
                ParaModel::from_runs(&[
                    ("中", Style { bold: None, italic: Some(true) }),
                    ("bc", Style::default()),
                ]),
            ],
        }
    }

    fn apply(&mut self, op: &ModelOp) -> Result<Option<(usize, u32, i32)>, OffsetError> {
        match op {
            ModelOp::Insert { para, at, text, style } => {
                let delta = text.encode_utf16().count() as i32;
                self.paras[*para].insert(*at, text, *style)?;
                Ok(Some((*para, *at, delta)))
            }
            ModelOp::Delete { para, from, to } => {
                let range = self.paras[*para].range(*from, *to)?;
                let units: u32 = self.paras[*para].cells[range.clone()]
                    .iter()
                    .map(|c| c.ch.len_utf16() as u32)
                    .sum();
                self.paras[*para].delete(*from, *to)?;
                Ok((units > 0).then_some((*para, *from, -(units as i32))))
            }
            ModelOp::SetRunStyle { para, from, to, bold, italic } => {
                self.paras[*para].set_run_style(*from, *to, bold, italic)?;
                Ok(None)
            }
            ModelOp::SetParaJc { para, jc } => {
                self.paras[*para].jc = match jc {
                    Change::Keep => self.paras[*para].jc.clone(),
                    Change::Unset => None,
                    Change::Set(v) => Some(v.clone()),
                };
                Ok(None)
            }
            ModelOp::Replace { para, text } => {
                let jc = self.paras[*para].jc.clone();
                self.paras[*para] = ParaModel::from_runs(&[(text, Style::default())]);
                self.paras[*para].jc = jc;
                Ok(None)
            }
        }
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn run_xml(text: &str, style: Style) -> String {
    let mut rpr = String::new();
    if style.bold.is_some() || style.italic.is_some() {
        rpr.push_str("<w:rPr>");
        match style.bold {
            Some(true) => rpr.push_str("<w:b/>"),
            Some(false) => rpr.push_str(r#"<w:b w:val="0"/>"#),
            None => {}
        }
        match style.italic {
            Some(true) => rpr.push_str("<w:i/>"),
            Some(false) => rpr.push_str(r#"<w:i w:val="0"/>"#),
            None => {}
        }
        rpr.push_str("</w:rPr>");
    }
    format!(r#"<w:r>{rpr}<w:t xml:space="preserve">{}</w:t></w:r>"#, escape(text))
}

fn docx_from_model(model: &DocModel) -> Vec<u8> {
    let mut body = String::new();
    for para in &model.paras {
        body.push_str("<w:p>");
        let mut start = 0usize;
        while start < para.cells.len() {
            let style = para.cells[start].style;
            let mut end = start + 1;
            while end < para.cells.len() && para.cells[end].style == style {
                end += 1;
            }
            let text: String = para.cells[start..end].iter().map(|c| c.ch).collect();
            body.push_str(&run_xml(&text, style));
            start = end;
        }
        body.push_str("</w:p>");
    }
    common::docx_with_body(&body)
}

fn actual_cells(block: &TextBlock) -> Vec<Cell> {
    let mut out = Vec::new();
    for inline in &block.inlines {
        match inline {
            Inline::Run(run) => {
                let style = Style { bold: run.props.bold, italic: run.props.italic };
                for seg in &run.segments {
                    use rsword::model::{BreakKind, SegmentKind};
                    match &seg.kind {
                        SegmentKind::Text | SegmentKind::DelText => {
                            out.extend(
                                run.segment_text(seg).chars().map(|ch| Cell::new(ch, style)),
                            );
                        }
                        SegmentKind::Tab | SegmentKind::PTab { .. } => {
                            out.push(Cell::new('\t', style));
                        }
                        SegmentKind::Br { kind, .. } => out.push(Cell::new(
                            match kind {
                                BreakKind::TextWrapping => '\n',
                                BreakKind::Page | BreakKind::Column => OBJECT_REPLACEMENT,
                            },
                            style,
                        )),
                        SegmentKind::Cr => out.push(Cell::new('\n', style)),
                        SegmentKind::NoBreakHyphen => out.push(Cell::new('\u{2011}', style)),
                        SegmentKind::SoftHyphen => out.push(Cell::new('\u{00AD}', style)),
                        SegmentKind::Ink
                        | SegmentKind::FootnoteRefMark
                        | SegmentKind::EndnoteRefMark => {}
                        _ if seg.utf16_len == 0 => {}
                        _ => out.push(Cell::new(OBJECT_REPLACEMENT, style)),
                    }
                }
            }
            Inline::Field { .. } | Inline::Atom(_) => {
                out.push(Cell::new(OBJECT_REPLACEMENT, Style::default()));
            }
        }
    }
    out
}

fn nth_para(s: &EditSession, i: usize) -> NodeId {
    s.document().text_blocks().nth(i).expect("paragraph").node
}

fn assert_model(s: &EditSession, model: &DocModel, context: &str) {
    let actual: Vec<_> = s.document().text_blocks().collect();
    assert_eq!(actual.len(), model.paras.len(), "{context}: paragraph count");
    for (i, (block, expected)) in actual.into_iter().zip(&model.paras).enumerate() {
        assert_eq!(block.text(), expected.text(), "{context}: paragraph {i} text");
        assert_eq!(
            block.utf16_len(),
            expected.utf16_len(),
            "{context}: paragraph {i} UTF-16 length"
        );
        assert_eq!(actual_cells(block), expected.cells, "{context}: paragraph {i} cells/styles");
        assert_eq!(block.props.jc, expected.jc, "{context}: paragraph {i} jc");
    }
}

fn edit_error_code(error: &Error) -> Option<DiagCode> {
    match error {
        Error::Edit { code, .. } => Some(*code),
        _ => None,
    }
}

fn pos(para: NodeId, offset: u32) -> InlinePos {
    InlinePos { part: None, para, offset: Utf16Offset(offset) }
}

fn explicit_style(bold: Option<bool>, italic: Option<bool>) -> RunPropsPatch {
    RunPropsPatch {
        bold: bold.map_or(Change::Unset, Change::Set),
        italic: italic.map_or(Change::Unset, Change::Set),
        ..Default::default()
    }
}

fn apply_op(s: &mut EditSession, model: &mut DocModel, op: &ModelOp) {
    let expected_delta = model.apply(op).expect("model op must be valid");
    let actual = match op {
        ModelOp::Insert { para, at, text, style } => s.apply(
            EditOp::InsertText {
                at: pos(nth_para(s, *para), *at),
                text: text.clone(),
                props: Some(explicit_style(style.bold, style.italic)),
            },
            &EditContext::default(),
        ),
        ModelOp::Delete { para, from, to } => s.apply(
            EditOp::DeleteRange {
                from: pos(nth_para(s, *para), *from),
                to: pos(nth_para(s, *para), *to),
            },
            &EditContext::default(),
        ),
        ModelOp::SetRunStyle { para, from, to, bold, italic } => s.apply(
            EditOp::SetRunProps {
                from: pos(nth_para(s, *para), *from),
                to: pos(nth_para(s, *para), *to),
                patch: RunPropsPatch {
                    bold: bold.clone(),
                    italic: italic.clone(),
                    ..Default::default()
                },
            },
            &EditContext::default(),
        ),
        ModelOp::SetParaJc { para, jc } => s.apply(
            EditOp::SetParaProps {
                part: None,
                para: nth_para(s, *para),
                patch: ParaPropsPatch { jc: jc.clone(), ..Default::default() },
            },
            &EditContext::default(),
        ),
        ModelOp::Replace { para, text } => s.apply(
            EditOp::ReplaceInlines {
                part: None,
                para: nth_para(s, *para),
                inlines: vec![NewInline::Run(NewRun::text(text.clone()))],
            },
            &EditContext::default(),
        ),
    }
    .unwrap_or_else(|e| panic!("production rejected valid op {op:?}: {e}"));
    if let Some((model_para, at, delta)) = expected_delta {
        let node = nth_para(s, model_para);
        assert!(
            actual.offset_delta.iter().any(|&(n, off, d)| n == node && off.0 == at && d == delta),
            "offset_delta 缺 {node:?}@{at}:{delta}: {:?}",
            actual.offset_delta
        );
    }
}

#[test]
fn edit_02_all_insert_positions_and_surrogate_midpoints() {
    let bases = ["", "a", "中", "😀", "e\u{301}", " &<>", "a😀中"];
    let inserted = ["x", "😀", "中", " \t\n", "&<>"];
    let style = Style { bold: Some(true), italic: Some(false) };
    let mut valid = 0usize;
    let mut invalid = 0usize;
    for base in bases {
        let len = base.encode_utf16().count() as u32;
        for at in 0..=len {
            for text in inserted {
                let mut model = DocModel {
                    paras: vec![
                        ParaModel::from_runs(&[(base, Style::default())]),
                        ParaModel::from_runs(&[("KEEP", Style::default())]),
                    ],
                };
                let mut s = EditSession::open(&docx_from_model(&model)).unwrap();
                match model.paras[0].index_at(at) {
                    Ok(_) => {
                        apply_op(
                            &mut s,
                            &mut model,
                            &ModelOp::Insert { para: 0, at, text: text.to_string(), style },
                        );
                        assert_model(&s, &model, &format!("insert {base:?}@{at} {text:?}"));
                        valid += 1;
                    }
                    Err(OffsetError::Surrogate) => {
                        let before = s.save().unwrap();
                        let err = s
                            .apply(
                                EditOp::InsertText {
                                    at: pos(nth_para(&s, 0), at),
                                    text: text.to_string(),
                                    props: Some(explicit_style(style.bold, style.italic)),
                                },
                                &EditContext::default(),
                            )
                            .expect_err("surrogate midpoint must be rejected");
                        assert_eq!(edit_error_code(&err), Some(DiagCode::EditSplitSurrogate));
                        assert_eq!(s.save().unwrap(), before, "rejected insert changed bytes");
                        invalid += 1;
                    }
                    Err(OffsetError::OutOfRange) => unreachable!(),
                }
            }
        }
    }
    assert!(valid >= 90, "valid insert cases = {valid}");
    assert!(invalid > 0, "surrogate midpoint cases = {invalid}");
}

#[test]
fn edit_02_all_delete_intervals_and_surrogate_midpoints() {
    let bases = ["", "a", "中", "😀", "e\u{301}", " &<>", "a😀中"];
    let mut valid = 0usize;
    let mut invalid = 0usize;
    for base in bases {
        let len = base.encode_utf16().count() as u32;
        for from in 0..=len {
            for to in from..=len {
                let mut model = DocModel {
                    paras: vec![
                        ParaModel::from_runs(&[(base, Style::default())]),
                        ParaModel::from_runs(&[("KEEP", Style::default())]),
                    ],
                };
                let mut s = EditSession::open(&docx_from_model(&model)).unwrap();
                let valid_range = model.paras[0].range(from, to).is_ok();
                if valid_range {
                    apply_op(&mut s, &mut model, &ModelOp::Delete { para: 0, from, to });
                    assert_model(&s, &model, &format!("delete {base:?}[{from},{to})"));
                    valid += 1;
                } else {
                    let before = s.save().unwrap();
                    let err = s
                        .apply(
                            EditOp::DeleteRange {
                                from: pos(nth_para(&s, 0), from),
                                to: pos(nth_para(&s, 0), to),
                            },
                            &EditContext::default(),
                        )
                        .expect_err("surrogate midpoint must be rejected");
                    assert_eq!(edit_error_code(&err), Some(DiagCode::EditSplitSurrogate));
                    assert_eq!(s.save().unwrap(), before, "rejected delete changed bytes");
                    invalid += 1;
                }
            }
        }
    }
    assert!(valid > 20, "valid delete cases = {valid}");
    assert!(invalid > 0, "surrogate midpoint cases = {invalid}");
}

#[test]
fn edit_03_run_property_set_unset_matches_reference_model() {
    let mut model = DocModel::two_paragraphs();
    let mut s = EditSession::open(&docx_from_model(&model)).unwrap();
    let ops = [
        ModelOp::SetRunStyle {
            para: 0,
            from: 0,
            to: 1,
            bold: Change::Set(false),
            italic: Change::Set(true),
        },
        ModelOp::SetRunStyle { para: 0, from: 1, to: 3, bold: Change::Unset, italic: Change::Keep },
        ModelOp::SetRunStyle {
            para: 1,
            from: 0,
            to: 3,
            bold: Change::Set(true),
            italic: Change::Unset,
        },
    ];
    for op in ops {
        apply_op(&mut s, &mut model, &op);
        assert_model(&s, &model, &format!("{op:?}"));
    }
}

#[test]
fn edit_03_controls_specials_and_atom_positions_match_reference_model() {
    let body = concat!(
        r#"<w:p><w:r><w:t xml:space="preserve">A</w:t></w:r>"#,
        r#"<w:br w:type="page"/>"#,
        r#"<w:r><w:t xml:space="preserve">B</w:t></w:r></w:p>"#,
        r#"<w:p><w:r><w:t>KEEP</w:t></w:r></w:p>"#,
    );
    let mut s = EditSession::open(&common::docx_with_body(body)).unwrap();
    let mut model = DocModel {
        paras: vec![
            ParaModel {
                cells: vec![
                    Cell::new('A', Style::default()),
                    Cell::new(OBJECT_REPLACEMENT, Style::default()),
                    Cell::new('B', Style::default()),
                ],
                jc: None,
            },
            ParaModel::from_runs(&[("KEEP", Style::default())]),
        ],
    };
    let style = Style { bold: Some(true), italic: Some(true) };
    for (at, text) in [(0, " &<>\t\n😀"), (2, "中\tx"), (4, "😀")] {
        apply_op(&mut s, &mut model, &ModelOp::Insert { para: 0, at, text: text.into(), style });
        assert_model(&s, &model, &format!("control insert @{at}"));
    }
}

#[test]
fn edit_03_hyperlink_and_multiple_text_segments_are_visible_but_not_merged() {
    let body = concat!(
        r#"<w:p><w:hyperlink w:anchor="target">"#,
        r#"<w:r><w:t xml:space="preserve">link</w:t></w:r>"#,
        r#"</w:hyperlink><w:bookmarkStart w:id="1" name="target"/>"#,
        r#"<w:r><w:t xml:space="preserve">ab</w:t><w:tab/><w:t xml:space="preserve">cd</w:t></w:r>"#,
        r#"<w:bookmarkEnd w:id="1"/></w:p>"#,
    );
    let mut s = EditSession::open(&common::docx_with_body(body)).unwrap();
    let mut model = DocModel {
        paras: vec![ParaModel {
            cells: "linkab\tcd".chars().map(|ch| Cell::new(ch, Style::default())).collect(),
            jc: None,
        }],
    };
    assert_model(&s, &model, "initial hyperlink/multi-segment");
    apply_op(
        &mut s,
        &mut model,
        &ModelOp::Insert {
            para: 0,
            at: 6,
            text: "😀&".into(),
            style: Style { bold: Some(true), italic: None },
        },
    );
    assert_model(&s, &model, "hyperlink insert");
    let saved = s.save().unwrap();
    let reopened = EditSession::open(&saved).unwrap();
    assert_model(&reopened, &model, "hyperlink save/reopen");
}

#[test]
fn edit_03_para_props_and_replace_inlines_match_reference_model() {
    let mut model = DocModel::two_paragraphs();
    let mut s = EditSession::open(&docx_from_model(&model)).unwrap();
    let ops = [
        ModelOp::SetParaJc { para: 0, jc: Change::Set(Val::Value(Jc::Center)) },
        ModelOp::SetParaJc { para: 1, jc: Change::Set(Val::Value(Jc::Right)) },
        ModelOp::SetParaJc { para: 0, jc: Change::Unset },
        ModelOp::Replace { para: 1, text: "替换😀".into() },
    ];
    for op in ops {
        apply_op(&mut s, &mut model, &op);
        assert_model(&s, &model, &format!("{op:?}"));
    }
}

fn exhaustive_candidates(model: &DocModel, step: usize) -> Vec<ModelOp> {
    let len0 = model.paras[0].utf16_len();
    let first_units = model.paras[0].cells.first().map_or(0, |c| c.ch.len_utf16() as u32);
    let bold = if step % 2 == 0 { Change::Set(true) } else { Change::Unset };
    vec![
        ModelOp::Insert {
            para: 0,
            at: 0,
            text: "x".into(),
            style: Style { bold: Some(true), italic: None },
        },
        ModelOp::Insert {
            para: 0,
            at: len0,
            text: "😀".into(),
            style: Style { bold: None, italic: Some(true) },
        },
        ModelOp::Delete { para: 0, from: 0, to: first_units },
        ModelOp::SetRunStyle {
            para: 0,
            from: 0,
            to: len0,
            bold: bold.clone(),
            italic: Change::Keep,
        },
        ModelOp::SetParaJc {
            para: 0,
            jc: if step % 2 == 0 { Change::Set(Val::Value(Jc::Center)) } else { Change::Unset },
        },
        ModelOp::Replace { para: 1, text: format!("r{step}") },
    ]
}

#[test]
fn test_07_exhaustive_one_to_four_step_sequences() {
    fn replay(prefix: &[ModelOp]) -> (EditSession, DocModel) {
        let mut model = DocModel::two_paragraphs();
        let mut s = EditSession::open(&docx_from_model(&model)).unwrap();
        for op in prefix {
            apply_op(&mut s, &mut model, op);
        }
        (s, model)
    }

    fn walk(
        prefix: &mut Vec<ModelOp>,
        state: &DocModel,
        max_steps: usize,
        sequences: &mut usize,
        operations: &mut usize,
    ) {
        if !prefix.is_empty() {
            let (s, model) = replay(prefix);
            assert_eq!(&model, state, "replay model drift");
            assert_model(&s, state, &format!("sequence {prefix:?}"));
            *sequences += 1;
            *operations += prefix.len();
        }
        if prefix.len() == max_steps {
            return;
        }
        for op in exhaustive_candidates(state, prefix.len()) {
            let mut next = state.clone();
            next.apply(&op).unwrap_or_else(|e| panic!("candidate {op:?} invalid: {e:?}"));
            prefix.push(op);
            walk(prefix, &next, max_steps, sequences, operations);
            prefix.pop();
        }
    }

    let max_steps: usize = std::env::var("RSWORD_MODEL_EXHAUSTIVE_STEPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    let max_steps = max_steps.clamp(1, 4);
    let mut sequences = 0usize;
    let mut operations = 0usize;
    walk(&mut Vec::new(), &DocModel::two_paragraphs(), max_steps, &mut sequences, &mut operations);
    let expected_sequences: usize = (1..=max_steps).map(|n| 6usize.pow(n as u32)).sum();
    assert_eq!(sequences, expected_sequences);
    eprintln!(
        "reference-model exhaustive: {sequences} sequences / {operations} operations / max {max_steps} steps"
    );
}

fn boundaries(para: &ParaModel) -> Vec<u32> {
    let mut out = vec![0];
    let mut units = 0u32;
    for cell in &para.cells {
        units += cell.ch.len_utf16() as u32;
        out.push(units);
    }
    out
}

fn random_boundary(para: &ParaModel, rng: &mut Rng) -> u32 {
    let b = boundaries(para);
    b[rng.below(b.len())]
}

fn random_insert_text(rng: &mut Rng) -> &'static str {
    ["x", "中", "😀", " \t\n", "&<>", "e\u{301}"][rng.below(6)]
}

fn random_style(rng: &mut Rng) -> Style {
    let tri = |rng: &mut Rng| match rng.below(3) {
        0 => None,
        1 => Some(true),
        _ => Some(false),
    };
    Style { bold: tri(rng), italic: tri(rng) }
}

fn random_patch(rng: &mut Rng) -> (Change<bool>, Change<bool>) {
    let c = |rng: &mut Rng| match rng.below(3) {
        0 => Change::Keep,
        1 => Change::Unset,
        _ => Change::Set(rng.chance(2)),
    };
    (c(rng), c(rng))
}

#[test]
fn test_07_reference_model_random_sequences_with_save_reopen() {
    let sequences: usize =
        std::env::var("RSWORD_MODEL_SEQUENCES").ok().and_then(|v| v.parse().ok()).unwrap_or(100);
    let steps: usize =
        std::env::var("RSWORD_MODEL_STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(100);
    let only = std::env::var("RSWORD_MODEL_SEED").ok().and_then(|v| v.parse::<u64>().ok());
    let mut total_ops = 0usize;
    let mut saves = 0usize;

    for seed in 0..sequences as u64 {
        if only.is_some_and(|v| v != seed) {
            continue;
        }
        let mut rng = Rng(seed.wrapping_add(0x9e37_79b9_7f4a_7c15));
        let mut model = DocModel::two_paragraphs();
        let mut s = EditSession::open(&docx_from_model(&model)).unwrap();
        for step in 0..steps {
            let para = rng.below(model.paras.len());
            let op = match rng.below(100) {
                0..=34 => ModelOp::Insert {
                    para,
                    at: random_boundary(&model.paras[para], &mut rng),
                    text: random_insert_text(&mut rng).into(),
                    style: random_style(&mut rng),
                },
                35..=64 => {
                    let a = random_boundary(&model.paras[para], &mut rng);
                    let b = random_boundary(&model.paras[para], &mut rng);
                    ModelOp::Delete { para, from: a.min(b), to: a.max(b) }
                }
                65..=82 => {
                    let a = random_boundary(&model.paras[para], &mut rng);
                    let b = random_boundary(&model.paras[para], &mut rng);
                    let (bold, italic) = random_patch(&mut rng);
                    ModelOp::SetRunStyle { para, from: a.min(b), to: a.max(b), bold, italic }
                }
                83..=94 => {
                    let jc = match rng.below(3) {
                        0 => Change::Keep,
                        1 => Change::Unset,
                        _ => {
                            Change::Set(Val::Value([Jc::Center, Jc::Right, Jc::Left][rng.below(3)]))
                        }
                    };
                    ModelOp::SetParaJc { para, jc }
                }
                _ => ModelOp::Replace {
                    para,
                    text: ["plain", "中文😀", " \t\n ", "&<>"][rng.below(4)].into(),
                },
            };
            apply_op(&mut s, &mut model, &op);
            assert_model(&s, &model, &format!("seed={seed} step={step} op={op:?}"));
            total_ops += 1;

            // 攻击非法 UTF-16 边界：期望拒绝且整包字节不变，然后继续合法操作。
            if rng.chance(7) {
                let text = model.paras[para].text();
                let len = text.encode_utf16().count() as u32;
                let mut units = 0u32;
                let mut bad = None;
                for ch in text.chars() {
                    if ch.len_utf16() == 2 {
                        bad = Some(units + 1);
                        break;
                    }
                    units += ch.len_utf16() as u32;
                }
                if let Some(off) = bad.filter(|&off| off <= len) {
                    let before = s.save().unwrap();
                    let err = s.apply(
                        EditOp::InsertText {
                            at: pos(nth_para(&s, para), off),
                            text: "X".into(),
                            props: None,
                        },
                        &EditContext::default(),
                    );
                    assert!(
                        matches!(
                            &err,
                            Err(Error::Edit { code, .. }) if *code == DiagCode::EditSplitSurrogate
                        ),
                        "seed={seed} step={step} surrogate attack: {err:?}"
                    );
                    assert_eq!(s.save().unwrap(), before, "failed attack left residue");
                    assert_model(&s, &model, &format!("seed={seed} after rejected step={step}"));
                }
            }

            if step % 17 == 16 || rng.chance(20) {
                let saved = s.save().unwrap();
                s = EditSession::open(&saved).unwrap();
                assert_model(&s, &model, &format!("seed={seed} reopen after step={step}"));
                saves += 1;
            }
        }
    }
    eprintln!(
        "reference-model random: {sequences} seeds x {steps} steps = {total_ops} operations, {saves} save/reopen cycles"
    );
}
