//! The ported standard rules (`ktlint-ruleset-standard`), registered like `StandardRuleSetProvider`.

mod annotation;
mod annotation_spacing_rule;
mod argument_list_wrapping_rule;
mod backing_property_naming_rule;
mod binary_expression_wrapping_rule;
mod blank_line_before_declaration_rule;
mod blank_line_before_file_annotation;
mod blank_line_before_imports;
mod blank_line_before_package;
mod blank_line_between_when_conditions;
mod block_comment_initial_star_alignment_rule;
mod call_expression_wrapping_rule;
pub mod chain_method_continuation;
mod chain_wrapping_rule;
mod class_naming_rule;
pub mod class_signature;
mod comment_spacing_rule;
mod comment_wrapping_rule;
mod condition_wrapping_rule;
mod context_parameter_list_wrapping_rule;
mod context_receiver_wrapping_rule;
mod discouraged_comment_location_rule;
mod enum_entry_name_case_rule;
mod enum_wrapping_rule;
mod expression_operand_wrapping_rule;
mod filename_rule;
mod final_newline_rule;
mod fun_keyword_spacing_rule;
mod function_expression_body_rule;
mod function_literal;
mod function_naming_rule;
mod function_return_type_spacing_rule;
pub mod function_signature;
mod function_start_of_body_spacing_rule;
mod function_type_modifier_spacing_rule;
mod function_type_reference_spacing_rule;
mod if_else_bracing_rule;
mod if_else_wrapping_rule;
mod import_ordering_rule;
mod indentation;
pub mod internal;
mod kdoc_rule;
mod kdoc_wrapping_rule;
mod lambda_return_rule;
pub mod max_line_length_rule;
mod mixed_condition_operators_rule;
mod modifier_list_spacing_rule;
mod modifier_order_rule;
mod multi_line_if_else_rule;
mod multiline_expression_wrapping_rule;
mod multiline_loop_rule;
mod no_blank_line_at_start_of_file_rule;
mod no_blank_line_before_rbrace_rule;
mod no_blank_line_in_list_rule;
mod no_blank_lines_in_chained_method_calls_rule;
mod no_consecutive_blank_lines_rule;
mod no_consecutive_comments_rule;
mod no_empty_class_body_rule;
mod no_empty_file_rule;
mod no_empty_first_line_in_class_body_rule;
mod no_empty_first_line_in_method_block_rule;
mod no_line_break_after_else_rule;
mod no_line_break_before_assignment_rule;
mod no_multiple_spaces_rule;
mod no_semicolons_rule;
mod no_single_line_block_comment_rule;
mod no_trailing_spaces_rule;
mod no_unit_return_rule;
mod no_unused_imports_rule;
mod no_wildcard_imports_rule;
mod nullable_type_spacing_rule;
mod package_name_rule;
mod parameter_list_spacing_rule;
mod parameter_list_wrapping_rule;
mod parameter_wrapping_rule;
mod property_naming_rule;
mod property_wrapping_rule;
mod spacing_around_angle_brackets_rule;
mod spacing_around_colon_rule;
mod spacing_around_comma_rule;
mod spacing_around_curly_rule;
mod spacing_around_dot_rule;
mod spacing_around_double_colon_rule;
mod spacing_around_keyword_rule;
mod spacing_around_operators_rule;
mod spacing_around_parens_rule;
mod spacing_around_range_operator_rule;
mod spacing_around_square_brackets_rule;
mod spacing_around_unary_operator_rule;
mod spacing_between_declarations_with_annotations_rule;
mod spacing_between_declarations_with_comments_rule;
mod spacing_between_function_name_and_opening_parenthesis_rule;
mod statement_wrapping_rule;
mod string_template_indent;
mod string_template_rule;
mod then_spacing_rule;
mod trailing_comma_on_call_site_rule;
mod trailing_comma_on_declaration_site;
mod try_catch_finally_spacing_rule;
mod type_argument_comment_rule;
mod type_argument_list_spacing_rule;
mod type_parameter_comment_rule;
mod type_parameter_list_spacing_rule;
mod unnecessary_parentheses_before_trailing_lambda_rule;
mod value_argument_comment_rule;
mod value_parameter_comment_rule;
mod when_entry_bracing;
mod wrapping_rule;

pub use annotation::{ANNOTATIONS_WITH_PARAMETERS_NOT_TO_BE_WRAPPED_PROPERTY, AnnotationRule};
pub use annotation_spacing_rule::AnnotationSpacingRule;
pub use argument_list_wrapping_rule::{ArgumentListWrappingRule, IGNORE_WHEN_PARAMETER_COUNT_GREATER_OR_EQUAL_THAN_PROPERTY};
pub use backing_property_naming_rule::BackingPropertyNamingRule;
pub use binary_expression_wrapping_rule::BinaryExpressionWrappingRule;
pub use blank_line_before_declaration_rule::BlankLineBeforeDeclarationRule;
pub use blank_line_before_file_annotation::BlankLineBeforeFileAnnotation;
pub use blank_line_before_imports::BlankLineBeforeImports;
pub use blank_line_before_package::BlankLineBeforePackage;
pub use blank_line_between_when_conditions::{BlankLineBetweenWhenConditions, LINE_BREAK_AFTER_WHEN_CONDITION_PROPERTY};
pub use block_comment_initial_star_alignment_rule::BlockCommentInitialStarAlignmentRule;
pub use call_expression_wrapping_rule::CallExpressionWrappingRule;
pub use chain_method_continuation::ChainMethodContinuationRule;
pub use chain_wrapping_rule::ChainWrappingRule;
pub use class_naming_rule::ClassNamingRule;
pub use class_signature::ClassSignatureRule;
pub use comment_spacing_rule::CommentSpacingRule;
pub use comment_wrapping_rule::CommentWrappingRule;
pub use condition_wrapping_rule::ConditionWrappingRule;
pub use context_parameter_list_wrapping_rule::ContextParameterListWrappingRule;
pub use context_receiver_wrapping_rule::ContextReceiverWrappingRule;
pub use discouraged_comment_location_rule::DiscouragedCommentLocationRule;
pub use enum_entry_name_case_rule::EnumEntryNameCaseRule;
pub use enum_wrapping_rule::EnumWrappingRule;
pub use expression_operand_wrapping_rule::ExpressionOperandWrappingRule;
pub use filename_rule::FilenameRule;
pub use final_newline_rule::FinalNewlineRule;
pub use fun_keyword_spacing_rule::FunKeywordSpacingRule;
pub use function_expression_body_rule::FunctionExpressionBodyRule;
pub use function_literal::FunctionLiteralRule;
pub use function_naming_rule::FunctionNamingRule;
pub use function_return_type_spacing_rule::FunctionReturnTypeSpacingRule;
pub use function_signature::FunctionSignatureRule;
pub use function_start_of_body_spacing_rule::FunctionStartOfBodySpacingRule;
pub use function_type_modifier_spacing_rule::FunctionTypeModifierSpacingRule;
pub use function_type_reference_spacing_rule::FunctionTypeReferenceSpacingRule;
pub use if_else_bracing_rule::IfElseBracingRule;
pub use if_else_wrapping_rule::IfElseWrappingRule;
pub use import_ordering_rule::ImportOrderingRule;
pub use indentation::{INDENT_WHEN_ARROW_ON_NEW_LINE, IndentationRule};
pub use kdoc_rule::KdocRule;
pub use kdoc_wrapping_rule::KdocWrappingRule;
pub use lambda_return_rule::LambdaReturnRule;
pub use max_line_length_rule::MaxLineLengthRule;
pub use mixed_condition_operators_rule::MixedConditionOperatorsRule;
pub use modifier_list_spacing_rule::ModifierListSpacingRule;
pub use modifier_order_rule::ModifierOrderRule;
pub use multi_line_if_else_rule::MultiLineIfElseRule;
pub use multiline_expression_wrapping_rule::MultilineExpressionWrappingRule;
pub use multiline_loop_rule::MultilineLoopRule;
pub use no_blank_line_at_start_of_file_rule::NoBlankLineAtStartOfFileRule;
pub use no_blank_line_before_rbrace_rule::NoBlankLineBeforeRbraceRule;
pub use no_blank_line_in_list_rule::NoBlankLineInListRule;
pub use no_blank_lines_in_chained_method_calls_rule::NoBlankLinesInChainedMethodCallsRule;
pub use no_consecutive_blank_lines_rule::NoConsecutiveBlankLinesRule;
pub use no_consecutive_comments_rule::NoConsecutiveCommentsRule;
pub use no_empty_class_body_rule::NoEmptyClassBodyRule;
pub use no_empty_file_rule::NoEmptyFileRule;
pub use no_empty_first_line_in_class_body_rule::NoEmptyFirstLineInClassBodyRule;
pub use no_empty_first_line_in_method_block_rule::NoEmptyFirstLineInMethodBlockRule;
pub use no_line_break_after_else_rule::NoLineBreakAfterElseRule;
pub use no_line_break_before_assignment_rule::NoLineBreakBeforeAssignmentRule;
pub use no_multiple_spaces_rule::NoMultipleSpacesRule;
pub use no_semicolons_rule::NoSemicolonsRule;
pub use no_single_line_block_comment_rule::NoSingleLineBlockCommentRule;
pub use no_trailing_spaces_rule::NoTrailingSpacesRule;
pub use no_unit_return_rule::NoUnitReturnRule;
pub use no_unused_imports_rule::NoUnusedImportsRule;
pub use no_wildcard_imports_rule::NoWildcardImportsRule;
pub use nullable_type_spacing_rule::NullableTypeSpacingRule;
pub use package_name_rule::PackageNameRule;
pub use parameter_list_spacing_rule::ParameterListSpacingRule;
pub use parameter_list_wrapping_rule::ParameterListWrappingRule;
pub use parameter_wrapping_rule::ParameterWrappingRule;
pub use property_naming_rule::PropertyNamingRule;
pub use property_wrapping_rule::PropertyWrappingRule;
pub use spacing_around_angle_brackets_rule::SpacingAroundAngleBracketsRule;
pub use spacing_around_colon_rule::SpacingAroundColonRule;
pub use spacing_around_comma_rule::SpacingAroundCommaRule;
pub use spacing_around_curly_rule::SpacingAroundCurlyRule;
pub use spacing_around_dot_rule::SpacingAroundDotRule;
pub use spacing_around_double_colon_rule::SpacingAroundDoubleColonRule;
pub use spacing_around_keyword_rule::SpacingAroundKeywordRule;
pub use spacing_around_operators_rule::SpacingAroundOperatorsRule;
pub use spacing_around_parens_rule::SpacingAroundParensRule;
pub use spacing_around_range_operator_rule::SpacingAroundRangeOperatorRule;
pub use spacing_around_square_brackets_rule::SpacingAroundSquareBracketsRule;
pub use spacing_around_unary_operator_rule::SpacingAroundUnaryOperatorRule;
pub use spacing_between_declarations_with_annotations_rule::SpacingBetweenDeclarationsWithAnnotationsRule;
pub use spacing_between_declarations_with_comments_rule::SpacingBetweenDeclarationsWithCommentsRule;
pub use spacing_between_function_name_and_opening_parenthesis_rule::SpacingBetweenFunctionNameAndOpeningParenthesisRule;
pub use statement_wrapping_rule::StatementWrappingRule;
pub use string_template_indent::StringTemplateIndentRule;
pub use string_template_rule::StringTemplateRule;
pub use then_spacing_rule::ThenSpacingRule;
pub use trailing_comma_on_call_site_rule::{TRAILING_COMMA_ON_CALL_SITE_PROPERTY, TrailingCommaOnCallSiteRule};
pub use trailing_comma_on_declaration_site::{TRAILING_COMMA_ON_DECLARATION_SITE_PROPERTY, TrailingCommaOnDeclarationSiteRule};
pub use try_catch_finally_spacing_rule::TryCatchFinallySpacingRule;
pub use type_argument_comment_rule::TypeArgumentCommentRule;
pub use type_argument_list_spacing_rule::TypeArgumentListSpacingRule;
pub use type_parameter_comment_rule::TypeParameterCommentRule;
pub use type_parameter_list_spacing_rule::TypeParameterListSpacingRule;
pub use unnecessary_parentheses_before_trailing_lambda_rule::UnnecessaryParenthesesBeforeTrailingLambdaRule;
pub use value_argument_comment_rule::ValueArgumentCommentRule;
pub use value_parameter_comment_rule::ValueParameterCommentRule;
pub use when_entry_bracing::WhenEntryBracing;
pub use wrapping_rule::WrappingRule;

use std::sync::LazyLock;

use crate::editorconfig::KtlintVersion::{V1_8, V2_0};
use crate::rule::{About, RuleV2};
use crate::rule_provider::RuleV2Provider;

/// `STANDARD_RULE_ABOUT` (`StandardRule.kt`).
pub const STANDARD_RULE_ABOUT: About = About {
    maintainer: "KtLint",
    repository_url: "https://github.com/ktlint/ktlint",
    issue_tracker_url: "https://github.com/ktlint/ktlint/issues",
};

/// The ported slice of `StandardRuleSetProvider().getRuleProviders()`, in its order, plus the rules that only
/// ktlint 1.8's rule set has (`only_in(V1_8)`; the 2.0 counterparts are `only_in(V2_0)`). Built once per process:
/// each `RuleV2Provider::new` instantiates its rule for the metadata, so callers clone the cached list.
pub fn standard_rule_providers() -> Vec<RuleV2Provider> {
    STANDARD_RULE_PROVIDERS.clone()
}

static STANDARD_RULE_PROVIDERS: LazyLock<Vec<RuleV2Provider>> = LazyLock::new(standard_rule_providers_list);

fn standard_rule_providers_list() -> Vec<RuleV2Provider> {
    vec![
        RuleV2Provider::new(|| Box::new(AnnotationRule::new())),
        RuleV2Provider::new(|| Box::new(AnnotationSpacingRule)),
        RuleV2Provider::new(|| Box::new(ArgumentListWrappingRule::new())),
        RuleV2Provider::new(|| Box::new(BackingPropertyNamingRule::new()) as Box<dyn RuleV2>),
        RuleV2Provider::new(|| Box::new(BinaryExpressionWrappingRule::new())),
        RuleV2Provider::new(|| Box::new(BlankLineBeforeDeclarationRule::default())).only_in(V2_0),
        RuleV2Provider::new(|| Box::new(BlankLineBeforeDeclarationRule::ktlint_1_8())).only_in(V1_8),
        RuleV2Provider::new(|| Box::new(BlankLineBeforeFileAnnotation::default())).only_in(V2_0),
        RuleV2Provider::new(|| Box::new(BlankLineBeforeImports::default())).only_in(V2_0),
        RuleV2Provider::new(|| Box::new(BlankLineBeforePackage::default())).only_in(V2_0),
        RuleV2Provider::new(|| Box::new(BlankLineBetweenWhenConditions::new())),
        RuleV2Provider::new(|| Box::new(BlockCommentInitialStarAlignmentRule)),
        RuleV2Provider::new(|| Box::new(CallExpressionWrappingRule::new())).only_in(V2_0),
        RuleV2Provider::new(|| Box::new(ChainMethodContinuationRule::new())),
        RuleV2Provider::new(|| Box::new(ChainWrappingRule::new())),
        RuleV2Provider::new(|| Box::new(ClassNamingRule::default())),
        RuleV2Provider::new(|| Box::new(ClassSignatureRule::new())),
        RuleV2Provider::new(|| Box::new(CommentSpacingRule)),
        RuleV2Provider::new(|| Box::new(CommentWrappingRule)),
        RuleV2Provider::new(|| Box::new(ConditionWrappingRule)).only_in(V1_8),
        RuleV2Provider::new(|| Box::new(ContextParameterListWrappingRule::new())).only_in(V2_0),
        RuleV2Provider::new(|| Box::new(ContextParameterListWrappingRule::context_receiver_list_wrapping())).only_in(V1_8),
        RuleV2Provider::new(|| Box::new(ContextReceiverWrappingRule::new())),
        RuleV2Provider::new(|| Box::new(DiscouragedCommentLocationRule)).only_in(V1_8),
        RuleV2Provider::new(|| Box::new(EnumEntryNameCaseRule::default())),
        RuleV2Provider::new(|| Box::new(EnumWrappingRule::new())),
        RuleV2Provider::new(|| Box::new(ExpressionOperandWrappingRule::new())),
        RuleV2Provider::new(|| Box::new(FilenameRule::default())),
        RuleV2Provider::new(|| Box::new(FinalNewlineRule::new())),
        RuleV2Provider::new(|| Box::new(FunKeywordSpacingRule)),
        RuleV2Provider::new(|| Box::new(FunctionExpressionBodyRule::new())),
        RuleV2Provider::new(|| Box::new(FunctionLiteralRule::new())),
        RuleV2Provider::new(|| Box::new(FunctionNamingRule::new())),
        RuleV2Provider::new(|| Box::new(FunctionReturnTypeSpacingRule::new())),
        RuleV2Provider::new(|| Box::new(FunctionSignatureRule::new())),
        RuleV2Provider::new(|| Box::new(FunctionStartOfBodySpacingRule)),
        RuleV2Provider::new(|| Box::new(FunctionTypeModifierSpacingRule)),
        RuleV2Provider::new(|| Box::new(FunctionTypeReferenceSpacingRule)),
        RuleV2Provider::new(|| Box::new(IfElseBracingRule::new())),
        RuleV2Provider::new(|| Box::new(IfElseWrappingRule::new())),
        RuleV2Provider::new(|| Box::new(ImportOrderingRule::new())),
        RuleV2Provider::new(|| Box::new(IndentationRule::new())),
        RuleV2Provider::new(|| Box::new(KdocRule)),
        RuleV2Provider::new(|| Box::new(KdocWrappingRule)),
        RuleV2Provider::new(|| Box::new(LambdaReturnRule)).only_in(V2_0),
        RuleV2Provider::new(|| Box::new(MaxLineLengthRule::new())),
        RuleV2Provider::new(|| Box::new(MixedConditionOperatorsRule)),
        RuleV2Provider::new(|| Box::new(ModifierListSpacingRule::new())),
        RuleV2Provider::new(|| Box::new(ModifierOrderRule)),
        RuleV2Provider::new(|| Box::new(MultiLineIfElseRule::new())),
        RuleV2Provider::new(|| Box::new(MultilineExpressionWrappingRule::new())),
        RuleV2Provider::new(|| Box::new(MultilineLoopRule::new())),
        RuleV2Provider::new(|| Box::new(NoBlankLineAtStartOfFileRule::default())).only_in(V2_0),
        RuleV2Provider::new(|| Box::new(NoBlankLineBeforeRbraceRule)),
        RuleV2Provider::new(|| Box::new(NoBlankLineInListRule)),
        RuleV2Provider::new(|| Box::new(NoBlankLinesInChainedMethodCallsRule)),
        RuleV2Provider::new(|| Box::new(NoConsecutiveBlankLinesRule)),
        RuleV2Provider::new(|| Box::new(NoConsecutiveCommentsRule)),
        RuleV2Provider::new(|| Box::new(NoEmptyClassBodyRule)),
        RuleV2Provider::new(|| Box::new(NoEmptyFileRule)),
        RuleV2Provider::new(|| Box::new(NoEmptyFirstLineInClassBodyRule::new())),
        RuleV2Provider::new(|| Box::new(NoEmptyFirstLineInMethodBlockRule)),
        RuleV2Provider::new(|| Box::new(NoLineBreakAfterElseRule)),
        RuleV2Provider::new(|| Box::new(NoLineBreakBeforeAssignmentRule)),
        RuleV2Provider::new(|| Box::new(NoMultipleSpacesRule)),
        RuleV2Provider::new(|| Box::new(NoSemicolonsRule)),
        RuleV2Provider::new(|| Box::new(NoSingleLineBlockCommentRule)),
        RuleV2Provider::new(|| Box::new(NoTrailingSpacesRule)),
        RuleV2Provider::new(|| Box::new(NoUnitReturnRule)),
        RuleV2Provider::new(|| Box::new(NoUnusedImportsRule::new())),
        RuleV2Provider::new(|| Box::new(NoWildcardImportsRule::new())),
        RuleV2Provider::new(|| Box::new(NullableTypeSpacingRule)),
        RuleV2Provider::new(|| Box::new(PackageNameRule)),
        RuleV2Provider::new(|| Box::new(ParameterListSpacingRule::new())),
        RuleV2Provider::new(|| Box::new(ParameterListWrappingRule::new())),
        RuleV2Provider::new(|| Box::new(ParameterWrappingRule::new())),
        RuleV2Provider::new(|| Box::new(PropertyNamingRule::new())),
        RuleV2Provider::new(|| Box::new(PropertyWrappingRule::new())),
        RuleV2Provider::new(|| Box::new(SpacingAroundAngleBracketsRule)),
        RuleV2Provider::new(|| Box::new(SpacingAroundColonRule)),
        RuleV2Provider::new(|| Box::new(SpacingAroundCommaRule)),
        RuleV2Provider::new(|| Box::new(SpacingAroundCurlyRule::new())),
        RuleV2Provider::new(|| Box::new(SpacingAroundDotRule)),
        RuleV2Provider::new(|| Box::new(SpacingAroundDoubleColonRule)),
        RuleV2Provider::new(|| Box::new(SpacingAroundKeywordRule)),
        RuleV2Provider::new(|| Box::new(SpacingAroundOperatorsRule)),
        RuleV2Provider::new(|| Box::new(SpacingAroundParensRule::default())),
        RuleV2Provider::new(|| Box::new(SpacingAroundRangeOperatorRule)),
        RuleV2Provider::new(|| Box::new(SpacingAroundSquareBracketsRule)),
        RuleV2Provider::new(|| Box::new(SpacingAroundUnaryOperatorRule)),
        RuleV2Provider::new(|| Box::new(SpacingBetweenDeclarationsWithAnnotationsRule)),
        RuleV2Provider::new(|| Box::new(SpacingBetweenDeclarationsWithCommentsRule)),
        RuleV2Provider::new(|| Box::new(SpacingBetweenFunctionNameAndOpeningParenthesisRule)),
        RuleV2Provider::new(|| Box::new(StatementWrappingRule::new())),
        RuleV2Provider::new(|| Box::new(StringTemplateIndentRule::new())),
        RuleV2Provider::new(|| Box::new(StringTemplateRule)),
        RuleV2Provider::new(|| Box::new(ThenSpacingRule)),
        RuleV2Provider::new(|| Box::new(TrailingCommaOnCallSiteRule::new())),
        RuleV2Provider::new(|| Box::new(TrailingCommaOnDeclarationSiteRule::new())),
        RuleV2Provider::new(|| Box::new(TryCatchFinallySpacingRule::new())),
        RuleV2Provider::new(|| Box::new(TypeArgumentCommentRule)),
        RuleV2Provider::new(|| Box::new(TypeArgumentListSpacingRule::new())),
        RuleV2Provider::new(|| Box::new(TypeParameterCommentRule)),
        RuleV2Provider::new(|| Box::new(TypeParameterListSpacingRule::new())),
        RuleV2Provider::new(|| Box::new(UnnecessaryParenthesesBeforeTrailingLambdaRule)),
        RuleV2Provider::new(|| Box::new(ValueArgumentCommentRule)),
        RuleV2Provider::new(|| Box::new(ValueParameterCommentRule)),
        RuleV2Provider::new(|| Box::new(WhenEntryBracing::new())),
        RuleV2Provider::new(|| Box::new(WrappingRule::new())),
    ]
}

/// The provider of a ported standard rule by id (`standard:` prefix optional).
pub fn standard_rule_provider(id: &str) -> Option<RuleV2Provider> {
    STANDARD_RULE_PROVIDERS
        .iter()
        .find(|p| {
            let val = p.rule_id().value();
            val == id || val.strip_prefix("standard:") == Some(id)
        })
        .cloned()
}
