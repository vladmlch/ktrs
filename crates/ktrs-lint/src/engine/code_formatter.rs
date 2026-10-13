//! Port of ktlint-rule-engine `internal/CodeFormatter.kt` (the format loop) and `AutocorrectHandler.kt`.

use std::borrow::Cow;

use ktrs_ast::Ast;

use crate::editorconfig::{END_OF_LINE_PROPERTY, EndOfLineValue};
use crate::engine::code::{Code, KtLintException, LintError};
use crate::engine::ktlint_rule_engine::{KtLintRuleEngine, UTF8_BOM};
use crate::engine::rule_execution_context::{RuleExecutionContext, create_rule_execution_context};
use crate::rule::AutocorrectDecision;

pub(crate) enum AutocorrectHandler<'a> {
    None,
    LintErrorAutocorrectHandler(&'a mut dyn FnMut(&LintError) -> AutocorrectDecision),
}

impl AutocorrectHandler<'_> {
    fn autocorrect_decision(&mut self, lint_error: &LintError) -> AutocorrectDecision {
        match self {
            AutocorrectHandler::None => AutocorrectDecision::NoAutocorrect,
            AutocorrectHandler::LintErrorAutocorrectHandler(callback) => callback(lint_error),
        }
    }

    fn is_none(&self) -> bool {
        matches!(self, AutocorrectHandler::None)
    }
}

/// `mutableSetOf<Pair<LintError, Boolean>>()`: insertion-ordered, duplicates dropped. Collected with duplicates
/// and deduplicated once by [`ErrorSet::into_distinct`], instead of cloning and hashing every error on add.
#[derive(Default)]
struct ErrorSet {
    order: Vec<(LintError, bool)>,
}

impl ErrorSet {
    fn add(&mut self, error: (LintError, bool)) {
        self.order.push(error);
    }

    /// The first occurrence of each error, in insertion order.
    fn into_distinct(self) -> Vec<(LintError, bool)> {
        let errors = self.order;
        let mut by_value: Vec<usize> = (0..errors.len()).collect();
        by_value.sort_unstable_by(|&a, &b| errors[a].cmp(&errors[b]).then(a.cmp(&b)));
        let mut keep = vec![true; errors.len()];
        for pair in by_value.windows(2) {
            if errors[pair[0]] == errors[pair[1]] {
                keep[pair[1]] = false;
            }
        }
        errors.into_iter().zip(keep).filter_map(|(error, keep)| keep.then_some(error)).collect()
    }
}

const CODE_FORMATTER_LOGGER: &str = "io.github.ktlint.core.rule.engine.internal.CodeFormatter";
const NOT_RESOLVED: &str = "Format was not able to resolve all violations which (theoretically) can be autocorrected in file";

/// `format(code, autocorrectHandler, callback, maxFormatRunsPerFile)`. The unmutated result borrows
/// `code.content`; callers returning a `String` pay the copy, `lint` just drops it.
pub(crate) fn format<'a>(
    engine: &KtLintRuleEngine,
    code: &'a Code,
    autocorrect_handler: AutocorrectHandler<'_>,
    callback: &mut dyn FnMut(&LintError, bool),
    max_format_runs_per_file: usize,
    after_pass: &mut dyn FnMut(&Ast),
) -> Result<Cow<'a, str>, KtLintException> {
    let (formatted_code, mut errors) = format_code(
        engine,
        code,
        autocorrect_handler,
        max_format_runs_per_file,
        after_pass,
    )?;
    errors.sort_by_key(|(e, _)| (e.line, e.col));
    for (e, corrected) in &errors {
        callback(e, *corrected);
    }
    let bom = if code.content.starts_with(UTF8_BOM) {
        UTF8_BOM.to_string()
    } else {
        String::new()
    };
    Ok(if formatted_code.starts_with(&bom) {
        formatted_code
    } else {
        Cow::Owned(bom + &formatted_code)
    })
}

fn format_code<'a>(
    engine: &KtLintRuleEngine,
    code: &'a Code,
    mut autocorrect_handler: AutocorrectHandler<'_>,
    max_format_runs_per_file: usize,
    after_pass: &mut dyn FnMut(&Ast),
) -> Result<(Cow<'a, str>, Vec<(LintError, bool)>), KtLintException> {
    let mut context = create_rule_execution_context(engine, code)?;
    let line_separator =
        determine_line_separator(code, context.setup.editor_config.get(&END_OF_LINE_PROPERTY));
    // With no autocorrect handler no error can be approved, so the loop breaks after the first pass
    // and `code_content` is never read: materializing the formatted text is skipped in lint mode.
    let mut code_content = if autocorrect_handler.is_none() {
        String::new()
    } else {
        formatted_code(&context, line_separator)
    };
    let mut errors = ErrorSet::default();
    let mut format_run_count = 0;
    let mut mutated = false;
    loop {
        let new_errors = format_pass(&mut context, &mut autocorrect_handler, after_pass)?;
        let any_autocorrected = new_errors
            .order
            .iter()
            .any(|(e, corrected)| e.can_be_auto_corrected && *corrected);
        new_errors.order.into_iter().for_each(|e| errors.add(e));
        if !any_autocorrected {
            break;
        }
        // Rule errors can cancel out, so the text decides whether the code changed.
        let updated_code_content = formatted_code(&context, line_separator);
        if updated_code_content == code_content {
            engine.warn(CODE_FORMATTER_LOGGER, || format!("{NOT_RESOLVED} {}", code.file_path_or_stdin()));
            break;
        }
        code_content = updated_code_content;
        mutated = true;
        format_run_count += 1;
        if format_run_count >= max_format_runs_per_file {
            break;
        }
    }
    if mutated && format_run_count == max_format_runs_per_file && !autocorrect_handler.is_none() {
        lint_after_format(&mut context, after_pass)?;
        engine.warn(CODE_FORMATTER_LOGGER, || {
            format!("{NOT_RESOLVED} {} in {max_format_runs_per_file} consecutive runs of format.", code.file_path_or_stdin())
        });
    }
    let formatted = if mutated {
        Cow::Owned(formatted_code(&context, line_separator))
    } else {
        Cow::Borrowed(code.content.as_str())
    };
    Ok((formatted, errors.into_distinct()))
}

fn formatted_code(context: &RuleExecutionContext, line_separator: &str) -> String {
    context
        .ast
        .text(context.ast.root())
        .replace('\n', line_separator)
}

fn format_pass(
    context: &mut RuleExecutionContext,
    autocorrect_handler: &mut AutocorrectHandler<'_>,
    after_pass: &mut dyn FnMut(&Ast),
) -> Result<ErrorSet, KtLintException> {
    let rules = context.setup.visitor_provider.rules();
    execute_rules_collecting(context, rules, autocorrect_handler, after_pass)
}

/// One more lint pass after the last allowed format pass: it only informs (upstream logs a warning).
fn lint_after_format(
    context: &mut RuleExecutionContext,
    after_pass: &mut dyn FnMut(&Ast),
) -> Result<bool, KtLintException> {
    let rules = context.setup.visitor_provider.rules();
    let mut has_errors_which_can_be_autocorrected = false;
    context.execute_rules(rules, true, &mut |_, _, _, can_be_auto_corrected| {
        has_errors_which_can_be_autocorrected |= can_be_auto_corrected;
        AutocorrectDecision::NoAutocorrect
    })?;
    after_pass(&context.ast);
    Ok(has_errors_which_can_be_autocorrected)
}

fn execute_rules_collecting(
    context: &mut RuleExecutionContext,
    rules: Vec<Box<dyn crate::rule::RuleV2>>,
    autocorrect_handler: &mut AutocorrectHandler<'_>,
    after_pass: &mut dyn FnMut(&Ast),
) -> Result<ErrorSet, KtLintException> {
    let mut errors = ErrorSet::default();
    let lint_mode = autocorrect_handler.is_none();
    let locator = context.position_in_text_locator.clone();
    context.execute_rules(
        rules,
        lint_mode,
        &mut |offset, rule_id, error_message, can_be_auto_corrected| {
            let (line, col) = locator.locate(offset);
            let lint_error = LintError {
                line,
                col,
                rule_id,
                detail: error_message.to_owned(),
                can_be_auto_corrected,
            };
            // Always asked, even when the error can't be autocorrected, so the consumer sees every error.
            let autocorrect_decision = autocorrect_handler.autocorrect_decision(&lint_error);
            // A rule that got the approval is assumed to have fixed the error.
            let autocorrect = autocorrect_decision == AutocorrectDecision::AllowAutocorrect
                && can_be_auto_corrected;
            errors.add((lint_error, autocorrect));
            autocorrect_decision
        },
    )?;
    after_pass(&context.ast);
    Ok(errors)
}

/// `determineLineSeparator`. Upstream's helper `doesNotContain` actually tests *contains*; ported as is.
fn determine_line_separator(
    code: &Code,
    eol_editor_config_property: EndOfLineValue,
) -> &'static str {
    if eol_editor_config_property == EndOfLineValue::Crlf
        || (eol_editor_config_property != EndOfLineValue::Lf && code.content.contains('\r'))
    {
        "\r\n"
    } else {
        "\n"
    }
}
