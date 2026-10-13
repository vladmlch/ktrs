//! `RuleExecutionContext.createRuleExecutionContext` (the companion object): parse, reject error elements,
//! load the `.editorconfig` and pick the enabled rules.

use std::borrow::Cow;
use std::rc::Rc;

use ktrs_ast::{Ast, NodeId};
use ktrs_parser::{FileKind, parse_file};
use ktrs_syntax::SyntaxKind::ERROR_ELEMENT;

use super::RuleExecutionContext;
use crate::editorconfig::{EditorConfig, KtlintVersion};
use crate::engine::code::{Code, KtLintException, KtLintParseException};
use crate::engine::ktlint_rule_engine::{KtLintRuleEngine, UTF8_BOM};
use crate::engine::position_in_text_locator::PositionInTextLocator;
use crate::engine::rule_filter::RULE_EXECUTION_RULE_FILTER_LOGGER;
use crate::engine::suppression_locator::SuppressionLocator;

const RULE_EXECUTION_CONTEXT_LOGGER_1_8: &str =
    "com.pinterest.ktlint.rule.engine.internal.RuleExecutionContext";

/// The `.editorconfig` is loaded before parsing (its version decides the BOM handling), but its error
/// is raised after the parse check, in upstream's order.
pub(crate) fn create_rule_execution_context(
    engine: &KtLintRuleEngine,
    code: &Code,
) -> Result<RuleExecutionContext, KtLintException> {
    let editor_config = engine
        .editor_config_loader()
        .load(code.file_path.as_deref());
    let ktlint_version = editor_config
        .as_ref()
        .map_or(KtlintVersion::default(), KtlintVersion::of);
    let normalized_text = normalize_text(&code.content, ktlint_version);
    let psi_file_name = code.psi_file_name();
    let parse = parse_file(&normalized_text, FileKind::from_file_name(&psi_file_name));
    // Owns the normalized text (moving it when normalization had to copy) so `locate` can build
    // its segment table lazily; `parse` no longer borrows it.
    let position_in_text_locator = PositionInTextLocator::new(normalized_text.into_owned());
    // Before any parse error: see `ktrs_syntax::MissedTokens`.
    if let Some(missed) = parse.first_missed_tokens() {
        return Err(KtLintException::MissedTokens(missed.clone()));
    }
    let mut ast = Ast::from_parse(&parse);
    ast.set_psi_file_name(&psi_file_name);
    if let Some(error_element) = find_error_element(&ast, ast.root()) {
        let (line, col) = position_in_text_locator
            .locate(ast.utf16_offset(error_element, ast.start_offset(error_element)));
        return Err(KtLintParseException {
            line,
            col,
            message: ast.error_description(error_element).to_owned(),
        }
        .into());
    }
    let editor_config = editor_config?;
    if ktlint_version.is_1_8() {
        warn_if_property_is_obsolete(engine, &editor_config, "disabled_rules", "0.49");
        warn_if_property_is_obsolete(engine, &editor_config, "ktlint_disabled_rules", "0.49");
    }
    let setup = engine.rule_setup(editor_config);
    for warning in &setup.rule_filter_warnings {
        engine.warn(RULE_EXECUTION_RULE_FILTER_LOGGER, || warning.clone());
    }
    if let Some(message) = &setup.rule_filter_error {
        return Err(KtLintException::IllegalState(message.clone()));
    }
    Ok(RuleExecutionContext {
        // 1.8 names the file, 2.0 gives its path.
        file_path_or_stdin: if ktlint_version.is_1_8() {
            code.file_name_or_stdin()
        } else {
            code.file_path_or_stdin()
        },
        ast,
        suppression_locator: SuppressionLocator::new(&setup.editor_config),
        setup,
        position_in_text_locator: Rc::new(position_in_text_locator),
    })
}

/// 1.8 drops the first BOM wherever it is; 2.0 only one at the start. A file without `\r` needs
/// no copy: the borrowed text is returned as is (or minus a leading BOM).
fn normalize_text<'a>(text: &'a str, ktlint_version: KtlintVersion) -> Cow<'a, str> {
    let text = if text.contains('\r') {
        Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(text)
    };
    if ktlint_version.is_1_8() {
        // `replacen(UTF8_BOM, "", 1)`: the first BOM wherever it is.
        let mut text = text;
        if let Some(index) = text.find(UTF8_BOM) {
            text.to_mut()
                .replace_range(index..index + UTF8_BOM.len_utf8(), "");
        }
        return text;
    }
    match text {
        Cow::Borrowed(text) => Cow::Borrowed(text.strip_prefix(UTF8_BOM).unwrap_or(text)),
        Cow::Owned(mut text) => {
            if text.starts_with(UTF8_BOM) {
                text.drain(..UTF8_BOM.len_utf8());
            }
            Cow::Owned(text)
        }
    }
}

/// 1.8 `EditorConfig.warnIfPropertyIsObsolete` (removed in 2.0).
fn warn_if_property_is_obsolete(
    engine: &KtLintRuleEngine,
    editor_config: &EditorConfig,
    property_name: &str,
    ktlint_version: &str,
) {
    if editor_config.contains(property_name) {
        engine.warn(RULE_EXECUTION_CONTEXT_LOGGER_1_8, || {
            format!(
                "Editorconfig property '{property_name}' is obsolete and is not used by KtLint starting from version \
                 {ktlint_version}. Remove the property from all '.editorconfig' files."
            )
        });
    }
}

/// The first `PsiErrorElement` depth-first; error elements are composites, so the PSI walk over
/// `children` and a node walk agree.
fn find_error_element(ast: &Ast, node: NodeId) -> Option<NodeId> {
    if ast.element_type(node) == ERROR_ELEMENT && !ast.is_leaf_element(node) {
        return Some(node);
    }
    let mut child = ast.first_child_node(node);
    while let Some(c) = child {
        if let Some(error_element) = find_error_element(ast, c) {
            return Some(error_element);
        }
        child = ast.tree_next(c);
    }
    None
}
