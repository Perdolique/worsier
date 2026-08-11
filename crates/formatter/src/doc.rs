use std::collections::HashMap;
use std::rc::Rc;

use dprint_core::formatting::{
    Condition, ConditionProperties, LineNumber, LineNumberAnchor, PrintItems, PrintOptions, Signal,
    actions, condition_helpers, format,
};

use crate::{IndentStyle, LineShape, ResolvedConfig};

#[derive(Clone, Copy, Debug)]
pub struct StatementSpacingCondition {
    pub previous_shape: LineShape,
    pub next_shape: LineShape,
    pub blank_lines: u8,
}

#[allow(
    dead_code,
    reason = "all doc primitives are part of the formatter's internal adapter surface"
)]
#[derive(Clone, Debug)]
pub enum Doc {
    Empty,
    Text(String),
    Concat(Vec<Self>),
    Space,
    HardLine,
    SoftLine,
    LineOrSpace,
    Indent(Box<Self>),
    Group(Box<Self>),
    Surround {
        inner: Box<Self>,
        flat_padding: bool,
    },
    ForceFlat(Box<Self>),
    Conditional {
        condition: bool,
        when_true: Box<Self>,
        when_false: Box<Self>,
    },
    LineSuffix(Box<Self>),
    Measured {
        id: usize,
        inner: Box<Self>,
    },
    StatementSeparator {
        previous: usize,
        next: usize,
        conditions: Vec<StatementSpacingCondition>,
    },
}

#[must_use]
pub const fn empty() -> Doc {
    Doc::Empty
}

#[must_use]
pub fn text(value: impl Into<String>) -> Doc {
    Doc::Text(value.into())
}

#[must_use]
pub fn token(value: impl Into<String>) -> Doc {
    text(value)
}

#[must_use]
pub fn concat(items: impl IntoIterator<Item = Doc>) -> Doc {
    Doc::Concat(items.into_iter().collect())
}

#[must_use]
pub const fn space() -> Doc {
    Doc::Space
}

#[must_use]
pub const fn hard_line() -> Doc {
    Doc::HardLine
}

#[must_use]
#[allow(
    dead_code,
    reason = "soft lines are used by later AST formatter families"
)]
pub const fn soft_line() -> Doc {
    Doc::SoftLine
}

#[must_use]
pub const fn line_or_space() -> Doc {
    Doc::LineOrSpace
}

#[must_use]
pub fn indent(doc: Doc) -> Doc {
    Doc::Indent(Box::new(doc))
}

#[must_use]
pub fn group(doc: Doc) -> Doc {
    Doc::Group(Box::new(doc))
}

#[must_use]
pub fn surround(doc: Doc, flat_padding: bool) -> Doc {
    Doc::Surround {
        inner: Box::new(doc),
        flat_padding,
    }
}

#[must_use]
#[allow(dead_code, reason = "force-flat is used by syntax-safety policies")]
pub fn force_flat(doc: Doc) -> Doc {
    Doc::ForceFlat(Box::new(doc))
}

#[must_use]
#[allow(dead_code, reason = "formatting policies use this adapter primitive")]
pub fn conditional(condition: bool, when_true: Doc, when_false: Doc) -> Doc {
    Doc::Conditional {
        condition,
        when_true: Box::new(when_true),
        when_false: Box::new(when_false),
    }
}

#[must_use]
#[allow(dead_code, reason = "comment formatting uses this adapter primitive")]
pub fn line_suffix(doc: Doc) -> Doc {
    Doc::LineSuffix(Box::new(doc))
}

#[must_use]
#[allow(
    dead_code,
    reason = "statement shape tracking uses this adapter primitive"
)]
pub fn measured(id: usize, doc: Doc) -> Doc {
    Doc::Measured {
        id,
        inner: Box::new(doc),
    }
}

#[must_use]
pub fn statement_separator(
    previous: usize,
    next: usize,
    conditions: Vec<StatementSpacingCondition>,
) -> Doc {
    Doc::StatementSeparator {
        previous,
        next,
        conditions,
    }
}

#[must_use]
pub fn forces_line_break(doc: &Doc) -> bool {
    match doc {
        Doc::HardLine | Doc::LineSuffix(_) => true,
        Doc::Concat(parts) => parts.iter().any(forces_line_break),
        Doc::Indent(inner)
        | Doc::Group(inner)
        | Doc::ForceFlat(inner)
        | Doc::Measured { inner, .. }
        | Doc::Surround { inner, .. } => forces_line_break(inner),
        Doc::Conditional {
            condition,
            when_true,
            when_false,
        } => forces_line_break(if *condition { when_true } else { when_false }),
        Doc::Empty
        | Doc::Text(_)
        | Doc::Space
        | Doc::SoftLine
        | Doc::LineOrSpace
        | Doc::StatementSeparator { .. } => false,
    }
}

#[must_use]
pub fn render(doc: &Doc, config: &ResolvedConfig, new_line_text: &'static str) -> String {
    let options = PrintOptions {
        max_width: u32::from(config.line_width()),
        indent_width: config.indent_width(),
        use_tabs: matches!(config.indent_style(), IndentStyle::Tab),
        new_line_text,
    };

    format(
        || {
            let mut items = PrintItems::new();
            let measurements = collect_measurements(doc);
            let mut pending_reevaluations = HashMap::new();
            append(doc, &mut items, &measurements, &mut pending_reevaluations);
            debug_assert!(pending_reevaluations.is_empty());
            items
        },
        options,
    )
}

fn append(
    doc: &Doc,
    items: &mut PrintItems,
    measurements: &HashMap<usize, (LineNumber, LineNumber)>,
    pending_reevaluations: &mut HashMap<usize, Vec<dprint_core::formatting::ConditionReevaluation>>,
) {
    match doc {
        Doc::Empty => {}
        Doc::Text(value) => items.push_string(value.clone()),
        Doc::Concat(parts) => {
            for part in parts {
                append(part, items, measurements, pending_reevaluations);
            }
        }
        Doc::Space => items.push_space(),
        Doc::HardLine => items.push_signal(Signal::NewLine),
        Doc::SoftLine => items.push_signal(Signal::PossibleNewLine),
        Doc::LineOrSpace => items.push_signal(Signal::SpaceOrNewLine),
        Doc::Indent(inner) => {
            items.push_signal(Signal::StartIndent);
            append(inner, items, measurements, pending_reevaluations);
            items.push_signal(Signal::FinishIndent);
        }
        Doc::Group(inner) => {
            items.push_signal(Signal::StartNewLineGroup);
            append(inner, items, measurements, pending_reevaluations);
            items.push_signal(Signal::FinishNewLineGroup);
        }
        Doc::Surround {
            inner,
            flat_padding,
        } => append_surround(
            inner,
            *flat_padding,
            items,
            measurements,
            pending_reevaluations,
        ),
        Doc::ForceFlat(inner) => {
            items.push_signal(Signal::StartForceNoNewLines);
            append(inner, items, measurements, pending_reevaluations);
            items.push_signal(Signal::FinishForceNoNewLines);
        }
        Doc::Conditional {
            condition,
            when_true,
            when_false,
        } => {
            append(
                if *condition { when_true } else { when_false },
                items,
                measurements,
                pending_reevaluations,
            );
        }
        Doc::LineSuffix(inner) => {
            append(inner, items, measurements, pending_reevaluations);
            items.push_signal(Signal::ExpectNewLine);
        }
        Doc::Measured { id, inner } => {
            let &(start, end) = measurements
                .get(id)
                .expect("every measured document has line markers");
            items.push_info(start);
            append(inner, items, measurements, pending_reevaluations);
            items.push_info(end);
            if let Some(reevaluations) = pending_reevaluations.remove(id) {
                for reevaluation in reevaluations {
                    items.push_reevaluation(reevaluation);
                }
            }
        }
        Doc::StatementSeparator {
            previous,
            next,
            conditions,
        } => append_statement_separator(
            *previous,
            *next,
            conditions,
            items,
            measurements,
            pending_reevaluations,
        ),
    }
}

fn append_surround(
    inner: &Doc,
    flat_padding: bool,
    items: &mut PrintItems,
    measurements: &HashMap<usize, (LineNumber, LineNumber)>,
    pending_reevaluations: &mut HashMap<usize, Vec<dprint_core::formatting::ConditionReevaluation>>,
) {
    let start_line = LineNumber::new("surroundStart");
    let end_line = LineNumber::new("surroundEnd");
    let mut inner_items = PrintItems::new();
    append(inner, &mut inner_items, measurements, pending_reevaluations);
    let inner_path = inner_items.into_rc_path();

    let mut broken = PrintItems::new();
    broken.push_signal(Signal::NewLine);
    broken.push_signal(Signal::StartIndent);
    broken.push_optional_path(inner_path);
    broken.push_signal(Signal::FinishIndent);
    broken.push_signal(Signal::NewLine);

    let mut flat = PrintItems::new();
    if flat_padding {
        flat.push_space();
    }
    flat.push_optional_path(inner_path);
    if flat_padding {
        flat.push_space();
    }

    items.push_info(start_line);
    items.push_anchor(LineNumberAnchor::new(end_line));
    items.extend(actions::if_column_number_changes(move |context| {
        context.clear_info(end_line);
    }));
    let mut condition = Condition::new(
        "surroundIfMultiLine",
        ConditionProperties {
            condition: Rc::new(move |context| {
                condition_helpers::is_multiple_lines(context, start_line, end_line)
            }),
            true_path: Some(broken),
            false_path: Some(flat),
        },
    );
    let reevaluation = condition.create_reevaluation();
    items.push_condition(condition);
    items.push_info(end_line);
    items.push_reevaluation(reevaluation);
}

fn collect_measurements(doc: &Doc) -> HashMap<usize, (LineNumber, LineNumber)> {
    fn visit(doc: &Doc, measurements: &mut HashMap<usize, (LineNumber, LineNumber)>) {
        match doc {
            Doc::Measured { id, inner } => {
                measurements.insert(
                    *id,
                    (
                        LineNumber::new("statementStart"),
                        LineNumber::new("statementEnd"),
                    ),
                );
                visit(inner, measurements);
            }
            Doc::Concat(parts) => {
                for part in parts {
                    visit(part, measurements);
                }
            }
            Doc::Indent(inner)
            | Doc::Group(inner)
            | Doc::ForceFlat(inner)
            | Doc::LineSuffix(inner)
            | Doc::Surround { inner, .. } => visit(inner, measurements),
            Doc::Conditional {
                when_true,
                when_false,
                ..
            } => {
                visit(when_true, measurements);
                visit(when_false, measurements);
            }
            Doc::Empty
            | Doc::Text(_)
            | Doc::Space
            | Doc::HardLine
            | Doc::SoftLine
            | Doc::LineOrSpace
            | Doc::StatementSeparator { .. } => {}
        }
    }

    let mut measurements = HashMap::new();
    visit(doc, &mut measurements);
    measurements
}

fn append_statement_separator(
    previous: usize,
    next: usize,
    conditions: &[StatementSpacingCondition],
    items: &mut PrintItems,
    measurements: &HashMap<usize, (LineNumber, LineNumber)>,
    pending_reevaluations: &mut HashMap<usize, Vec<dprint_core::formatting::ConditionReevaluation>>,
) {
    let &(previous_start, previous_end) = measurements
        .get(&previous)
        .expect("the previous statement has line markers");
    let &(next_start, next_end) = measurements
        .get(&next)
        .expect("the next statement has line markers");
    let mut path = hard_lines(1);
    for condition in conditions.iter().rev() {
        let true_path = hard_lines(usize::from(condition.blank_lines) + 1);
        let false_path = path;
        let previous_shape = condition.previous_shape;
        let next_shape = condition.next_shape;
        let mut dprint_condition = Condition::new(
            "statementSpacing",
            ConditionProperties {
                condition: Rc::new(move |context| {
                    let previous_multiline = condition_helpers::is_multiple_lines(
                        context,
                        previous_start,
                        previous_end,
                    )?;
                    let next_multiline =
                        condition_helpers::is_multiple_lines(context, next_start, next_end)?;
                    Some(
                        shape_matches(previous_shape, previous_multiline)
                            && shape_matches(next_shape, next_multiline),
                    )
                }),
                true_path: Some(true_path),
                false_path: Some(false_path),
            },
        );
        pending_reevaluations
            .entry(next)
            .or_default()
            .push(dprint_condition.create_reevaluation());
        let mut condition_path = PrintItems::new();
        condition_path.push_condition(dprint_condition);
        path = condition_path;
    }
    items.push_anchor(LineNumberAnchor::new(next_end));
    items.extend(path);
}

fn hard_lines(count: usize) -> PrintItems {
    let mut items = PrintItems::new();
    for _ in 0..count {
        items.push_signal(Signal::NewLine);
    }
    items
}

const fn shape_matches(shape: LineShape, multiline: bool) -> bool {
    match shape {
        LineShape::Any => true,
        LineShape::SingleLine => !multiline,
        LineShape::MultiLine => multiline,
    }
}

#[cfg(test)]
mod tests {
    use super::{concat, group, line_or_space, render, surround, text};
    use crate::{FormatConfig, resolve_config};

    #[test]
    fn group_stays_flat_when_it_fits() {
        let config = resolve_config(FormatConfig::default()).unwrap();
        let doc = group(concat([
            text("["),
            surround(concat([text("one,"), line_or_space(), text("two")]), false),
            text("]"),
        ]));

        assert_eq!(render(&doc, &config, "\n"), "[one, two]");
    }

    #[test]
    fn group_breaks_and_indents_over_width() {
        let raw = FormatConfig {
            line_width: 8,
            ..FormatConfig::default()
        };
        let config = resolve_config(raw).unwrap();
        let doc = group(concat([
            text("["),
            surround(concat([text("one,"), line_or_space(), text("two")]), false),
            text("]"),
        ]));

        assert_eq!(render(&doc, &config, "\n"), "[\n  one,\n  two\n]");
    }
}
