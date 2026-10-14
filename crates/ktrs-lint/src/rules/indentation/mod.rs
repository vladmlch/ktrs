//! Port of ktlint-ruleset-standard `IndentationRule.kt`, split in upstream order: this file (the rule, its
//! `IndentContext` and companion, `beforeVisitChildNodes`), `visit_declarations.rs` (`visitValueArgument` ..
//! `visitObjectDeclaration`), `visit_expressions.rs` (`visitBinaryExpression` .. `visitTryCatchFinally`),
//! `context.rs` (`skipLeadingWhitespaceCommentsAndAnnotations` .. `afterLastNode`), `new_line.rs`
//! (`visitNewLineIndentation` .. `isPrecededByComment`) and `string_template_indenter.rs`. Trace logging is dropped.

mod context;
mod new_line;
mod string_template_indenter;
mod visit_declarations;
mod visit_expressions;

use std::sync::{Arc, LazyLock};

use ktrs_ast::{Ast, NodeId};
use ktrs_editorconfig::PropertyType;
use ktrs_editorconfig::property_type::boolean_value_parser;
use ktrs_syntax::SyntaxKind::{
    self, ANNOTATED_EXPRESSION, ARRAY_ACCESS_EXPRESSION, BINARY_EXPRESSION, BINARY_WITH_TYPE, CALL_EXPRESSION, CLASS,
    CLOSING_QUOTE, CONDITION, CONTEXT_PARAMETER_LIST, DELEGATED_SUPER_TYPE_ENTRY, DESTRUCTURING_DECLARATION,
    DOT_QUALIFIED_EXPRESSION, FOR, FUN, FUNCTION_LITERAL, IDENTIFIER, IF, IS_EXPRESSION, LBRACE, LBRACKET,
    LITERAL_STRING_TEMPLATE_ENTRY, LONG_STRING_TEMPLATE_ENTRY, LPAR, NULLABLE_TYPE, OBJECT_DECLARATION, PARENTHESIZED,
    POSTFIX_EXPRESSION, PREFIX_EXPRESSION, PRIMARY_CONSTRUCTOR, PROPERTY, PROPERTY_ACCESSOR, SAFE_ACCESS_EXPRESSION,
    SECONDARY_CONSTRUCTOR, STRING_TEMPLATE, SUPER_TYPE_LIST, TRY, TYPE_ARGUMENT_LIST, TYPE_CONSTRAINT_LIST,
    TYPE_PARAMETER_LIST, TYPE_REFERENCE, USER_TYPE, VALUE_ARGUMENT, VALUE_ARGUMENT_LIST, VALUE_PARAMETER,
    VALUE_PARAMETER_LIST, WHEN, WHEN_ENTRY, WHERE_KEYWORD, WHILE,
};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::editorconfig::{
    CODE_STYLE_PROPERTY, CodeStyleValue, EditorConfigProperty, INDENT_SIZE_PROPERTY, INDENT_STYLE_PROPERTY, KtlintVersion, PropertyRef,
};
use crate::element_type::{KDOC, TYPEALIAS};
use crate::indent_config::IndentConfig;
use crate::rule::{About, EditorConfig, Emit, RuleId, RuleV2, TraversalState, VisitorModifier};
use crate::rules::STANDARD_RULE_ABOUT;

use string_template_indenter::StringTemplateIndenter;

const KDOC_CONTINUATION_INDENT: &str = " ";
const TYPE_CONSTRAINT_CONTINUATION_INDENT: &str = "      "; // Length of keyword "where" plus separating space
const CHAINABLE_EXPRESSION: [SyntaxKind; 2] = [DOT_QUALIFIED_EXPRESSION, SAFE_ACCESS_EXPRESSION];

static INDENT_WHEN_ARROW_ON_NEW_LINE_TYPE: PropertyType<bool> = PropertyType {
    name: "ij_kotlin_indent_before_arrow_on_new_line",
    description: "Indent the arrow in a when-entry if the arrow starts on a new line.",
    parser: boolean_value_parser,
    possible_values: &["true", "false"],
    lower_casing: true,
};

// Up until (and including) Intellij IDEA version `2024.1.6` the arrow on a newline was not indented. In version `2024.2`
// the default behavior was changed to true. Disable by default to keep backward compatibility with older ktlint and IDEA
// versions.
pub static INDENT_WHEN_ARROW_ON_NEW_LINE: LazyLock<EditorConfigProperty<bool>> =
    LazyLock::new(|| EditorConfigProperty::new(&INDENT_WHEN_ARROW_ON_NEW_LINE_TYPE, false));

pub struct IndentationRule {
    code_style: CodeStyleValue,
    indent_config: IndentConfig,
    cached_indent: Arc<str>,
    indent_when_arrow_on_new_line: bool,
    indent_context_stack: Vec<IndentContext>,
    string_template_indenter: Option<StringTemplateIndenter>,
    traversal: TraversalState,
    ktlint_version: KtlintVersion,
}

impl IndentationRule {
    pub fn new() -> IndentationRule {
        IndentationRule {
            code_style: CODE_STYLE_PROPERTY.default_value,
            indent_config: IndentConfig::default_indent_config(),
            cached_indent: Arc::from(""),
            indent_when_arrow_on_new_line: INDENT_WHEN_ARROW_ON_NEW_LINE.default_value,
            indent_context_stack: Vec::new(),
            string_template_indenter: None,
            traversal: TraversalState::default(),
            ktlint_version: KtlintVersion::default(),
        }
    }

    fn is_official(&self) -> bool {
        self.code_style == CodeStyleValue::KtlintOfficial
    }
}

impl Default for IndentationRule {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleV2 for IndentationRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:indent")
    }

    fn visitor_modifiers(&self) -> &'static [VisitorModifier] {
        const MODIFIERS: &[VisitorModifier] = &[
            VisitorModifier::RunAsLateAsPossible,
            VisitorModifier::run_after("standard:class-signature"),
            VisitorModifier::run_after("standard:function-signature"),
            VisitorModifier::run_after("standard:trailing-comma-on-call-site"),
            VisitorModifier::run_after("standard:trailing-comma-on-declaration-site"),
        ];
        MODIFIERS
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn uses_editor_config_properties(&self) -> Vec<PropertyRef> {
        vec![
            PropertyRef::from(&*CODE_STYLE_PROPERTY),
            PropertyRef::from(&*INDENT_SIZE_PROPERTY),
            PropertyRef::from(&*INDENT_STYLE_PROPERTY),
            PropertyRef::from(&*INDENT_WHEN_ARROW_ON_NEW_LINE),
        ]
    }

    fn traversal_state(&self) -> Option<TraversalState> {
        Some(self.traversal)
    }

    fn before_first_node(&mut self, editor_config: &EditorConfig) {
        self.code_style = editor_config.get(&CODE_STYLE_PROPERTY);
        self.indent_config = IndentConfig::new(editor_config.get(&INDENT_STYLE_PROPERTY), editor_config.get(&INDENT_SIZE_PROPERTY));
        self.cached_indent = Arc::from(self.indent_config.indent.as_str());
        if self.indent_config.disabled() {
            self.traversal.stop_traversal_of_ast();
        }
        self.indent_when_arrow_on_new_line = editor_config.get(&INDENT_WHEN_ARROW_ON_NEW_LINE);
        self.ktlint_version = KtlintVersion::of(editor_config);
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if ast.is_root(node) {
            // File should not start with a whitespace
            if let Some(whitespace_without_newline) = ast.next_leaf(node).filter(|&it| ast.is_white_space_without_newline(it)) {
                emit(ast, ast.start_offset(node), "Unexpected indentation", true).if_autocorrect_allowed(|| ast.remove(whitespace_without_newline));
            }
            let no_indent_zone = start_no_indent_zone(ast, node);
            self.indent_context_stack.push(no_indent_zone);
        }

        let element_type = ast.element_type(node);
        let parent_type = || ast.parent(node).map(|it| ast.element_type(it));
        let next_code_sibling_type = || ast.next_code_sibling(node).map(|it| ast.element_type(it));
        let ast_ref: &Ast = ast;
        if ast.is_white_space_with_newline(node) {
            if let Some(last) = self.indent_context_stack.last_mut().filter(|it| !it.activated) {
                last.activated = true;
            }
            self.visit_new_line_indentation(ast, node, emit);
        } else if matches!(element_type, CONTEXT_PARAMETER_LIST | LONG_STRING_TEMPLATE_ENTRY | STRING_TEMPLATE | VALUE_ARGUMENT_LIST) {
            self.start_indent_context(ast_ref, node, Args::default().last_child_indent(""));
        } else if (element_type == SUPER_TYPE_LIST && !is_preceded_by_comment(ast_ref, node))
            || (ast.is_part_of_comment(node) && next_code_sibling_type() == Some(SUPER_TYPE_LIST))
        {
            if self.is_official() {
                let super_type_list =
                    if ast.is_part_of_comment(node) { ast.next_code_leaf(node).expect("NullPointerException: nextCodeLeaf!!") } else { node };
                if is_part_of_class_with_a_multiline_primary_constructor(ast_ref, super_type_list) {
                    // Contrary to the default IntelliJ IDEA formatter, indent the super type call entry so that it looks better in case it
                    // is followed by another super type:
                    //      class Foo(
                    //          val bar1: Bar,
                    //          val bar2: Bar,
                    //      ) : FooBar(
                    //              bar1,
                    //              bar2
                    //          ),
                    //          BarFoo,
                    self.start_indent_context(ast_ref, node, Args::default().activated());
                }
            }
        } else if element_type == VALUE_ARGUMENT {
            self.visit_value_argument(ast_ref, node);
        } else if element_type == SECONDARY_CONSTRUCTOR {
            self.visit_secondary_constructor(ast_ref, node);
        } else if element_type == PARENTHESIZED {
            if self.is_official() {
                // Contrary to the IntelliJ IDEA default formatter, do not indent the closing RPAR
                self.start_indent_context(ast_ref, node, Args::default().last_child_indent(""));
            } else if ast.parent(node).and_then(|it| ast.parent(it)).map(|it| ast.element_type(it)) != Some(IF) {
                self.start_indent_context(ast_ref, node, Args::default());
            }
        } else if matches!(element_type, TYPE_ARGUMENT_LIST | TYPE_PARAMETER_LIST) {
            if self.is_official() {
                // Contrary to the IntelliJ IDEA default formatter, do not indent the closing angle bracket
                self.start_indent_context(ast_ref, node, Args::default().last_child_indent(""));
            } else {
                self.start_indent_context(ast_ref, node, Args::default());
            }
        } else if matches!(element_type, BINARY_WITH_TYPE | USER_TYPE) {
            self.start_indent_context(ast_ref, node, Args::default());
        } else if matches!(element_type, IS_EXPRESSION | PREFIX_EXPRESSION | POSTFIX_EXPRESSION) {
            self.start_indent_context(ast_ref, node, Args::default());
        } else if matches!(element_type, DELEGATED_SUPER_TYPE_ENTRY | ANNOTATED_EXPRESSION | TYPE_REFERENCE) {
            self.start_indent_context(ast_ref, node, Args::default().child_indent(""));
        } else if element_type == IF {
            self.visit_if(ast_ref, node);
        } else if element_type == LBRACE {
            self.visit_lbrace(ast_ref, node);
        } else if element_type == VALUE_PARAMETER_LIST && parent_type() != Some(FUNCTION_LITERAL) {
            self.start_indent_context(ast_ref, node, Args::default().last_child_indent(""));
        } else if element_type == LPAR && next_code_sibling_type() == Some(CONDITION) {
            self.visit_lpar_before_condition(ast_ref, node);
        } else if element_type == VALUE_PARAMETER {
            self.visit_value_parameter(ast_ref, node);
        } else if element_type == FUN {
            self.visit_fun(ast_ref, node);
        } else if element_type == CLASS {
            self.visit_class(ast_ref, node);
        } else if element_type == OBJECT_DECLARATION {
            self.visit_object_declaration(ast_ref, node);
        } else if element_type == BINARY_EXPRESSION {
            self.visit_binary_expression(ast_ref, node);
        } else if CHAINABLE_EXPRESSION.contains(&element_type) {
            if self.is_official()
                && element_type == DOT_QUALIFIED_EXPRESSION
                && parent_type() == Some(ARRAY_ACCESS_EXPRESSION)
                && ast.parent(node).and_then(|it| ast.parent(it)).map(|it| ast.element_type(it)) == Some(CALL_EXPRESSION)
            {
                // Issue 1540: Deviate and fix from incorrect formatting in IntelliJ IDEA formatting and produce following:
                // val fooBar2 = foo
                //    .bar[0] {
                //        "foobar"
                //    }
                let parent = ast.parent(node).expect("NullPointerException: parent!!");
                let grand_parent = ast.parent(parent).expect("NullPointerException: parent!!.parent!!");
                self.start_indent_context(ast_ref, parent, Args::default().to(ast.last_child_leaf_or_self(grand_parent)));
            } else if is_elvis_operator(ast_ref, ast.prev_code_sibling(node)) {
                self.start_indent_context(ast_ref, node, Args::default());
            } else if parent_type().is_some_and(|it| CHAINABLE_EXPRESSION.contains(&it)) {
                // Multiple dot qualified expressions and/or safe expression on the same line should not increase the indent level
            } else {
                self.start_indent_context(ast_ref, node, Args::default());
            }
        } else if element_type == IDENTIFIER && parent_type() == Some(PROPERTY) {
            self.visit_identifier_in_property(ast_ref, node);
        } else if element_type == LITERAL_STRING_TEMPLATE_ENTRY && next_code_sibling_type() == Some(CLOSING_QUOTE) {
            self.visit_white_space_before_closing_quote(ast, node, emit);
        } else if element_type == WHEN {
            self.visit_when(ast_ref, node);
        } else if element_type == WHEN_ENTRY {
            self.visit_when_entry(ast_ref, node);
        } else if element_type == WHERE_KEYWORD && next_code_sibling_type() == Some(TYPE_CONSTRAINT_LIST) {
            self.visit_where_keyword_before_type_constraint_list(ast_ref, node);
        } else if element_type == KDOC {
            self.visit_kdoc(ast_ref, node);
        } else if matches!(element_type, PROPERTY_ACCESSOR | TYPEALIAS) {
            self.visit_property_accessor(ast_ref, node);
        } else if matches!(element_type, FOR | WHILE) {
            self.visit_conditional_loop(ast_ref, node);
        } else if element_type == LBRACKET {
            self.visit_l_bracket(ast_ref, node);
        } else if element_type == NULLABLE_TYPE {
            self.visit_nullable_type(ast_ref, node);
        } else if element_type == DESTRUCTURING_DECLARATION {
            self.visit_destructuring_declaration(ast_ref, node);
        } else if element_type == TRY {
            self.visit_try_catch_finally(ast_ref, node);
        }
    }

    fn after_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, _emit: &mut Emit<'_>) {
        self.after_visit_child_nodes_impl(ast, node);
    }

    fn visits_after_child_nodes(&self) -> bool {
        true
    }

    fn after_last_node(&mut self) {
        self.after_last_node_impl();
    }
}

fn is_part_of_class_with_a_multiline_primary_constructor(ast: &Ast, node: NodeId) -> bool {
    ast.parent(node)
        .filter(|&it| ast.element_type(it) == CLASS)
        .and_then(|it| ast.find_child_by_type(it, PRIMARY_CONSTRUCTOR))
        .is_some_and(|it| ast.text_contains(it, '\n'))
}

/// `IndentContext`.
#[derive(Clone, Debug)]
struct IndentContext {
    /// The node on which the indent context starts.
    from_ast_node: NodeId,
    /// The node at which the indent context ends.
    to_ast_node: NodeId,
    /// Cumulative indentation for the node.
    node_indent: Arc<str>,
    /// Additional indentation for the first child node.
    first_child_indent: Arc<str>,
    /// Additional indentation for child nodes.
    child_indent: Arc<str>,
    /// Additional indentation for the last child node.
    last_child_indent: Arc<str>,
    /// True when the indentation level of this context is activated
    activated: bool,
    /// (element type, is leaf) of `from_ast_node` and `to_ast_node`: `afterLastNode`'s exception prints them via
    /// `ASTNode.toString()` but gets no arena.
    node_types: [(SyntaxKind, bool); 2],
}

/// The data class's `toString()`, for the `Stack should be empty` exception.
impl std::fmt::Display for IndentContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [from, to] = self.node_types.map(|(kind, is_leaf)| ktrs_ast::psi::element_to_string(kind, is_leaf));
        write!(
            f,
            "IndentContext(fromASTNode={from}, toASTNode={to}, nodeIndent={}, firstChildIndent={}, childIndent={}, \
             lastChildIndent={}, activated={})",
            self.node_indent, self.first_child_indent, self.child_indent, self.last_child_indent, self.activated
        )
    }
}

impl IndentContext {
    fn node_types(ast: &Ast, from_ast_node: NodeId, to_ast_node: NodeId) -> [(SyntaxKind, bool); 2] {
        [from_ast_node, to_ast_node].map(|it| (ast.element_type(it), ast.is_leaf_element(it)))
    }

    #[allow(dead_code)]
    fn indent(&self) -> String {
        if self.activated { format!("{}{}", self.node_indent, self.child_indent) } else { self.node_indent.to_string() }
    }

    pub(crate) fn indent_arc(&self) -> Arc<str> {
        if self.activated { format!("{}{}", self.node_indent, self.child_indent).into() } else { self.node_indent.clone() }
    }
}

/// What `startIndentContext` returns, as far as callers use it (`fromASTNode`, `prevCodeLeaf()`).
#[derive(Clone, Copy)]
struct StartedIndentContext {
    from_ast_node: NodeId,
}

impl StartedIndentContext {
    fn prev_code_leaf(self, ast: &Ast) -> NodeId {
        ast.prev_code_leaf(self.from_ast_node).expect("NullPointerException: fromASTNode.prevCodeLeaf!!")
    }
}

/// The named arguments of `startIndentContext`; `None` takes the upstream default.
#[derive(Default)]
struct Args {
    to_ast_node: Option<NodeId>,
    node_indent: Option<Arc<str>>,
    child_indent: Option<Arc<str>>,
    first_child_indent: Option<Arc<str>>,
    last_child_indent: Option<Arc<str>>,
    activated: bool,
}

impl Args {
    fn to(mut self, to_ast_node: NodeId) -> Args {
        self.to_ast_node = Some(to_ast_node);
        self
    }

    fn node_indent(mut self, node_indent: impl Into<Arc<str>>) -> Args {
        self.node_indent = Some(node_indent.into());
        self
    }

    fn child_indent(mut self, child_indent: impl Into<Arc<str>>) -> Args {
        self.child_indent = Some(child_indent.into());
        self
    }

    fn first_child_indent(mut self, first_child_indent: impl Into<Arc<str>>) -> Args {
        self.first_child_indent = Some(first_child_indent.into());
        self
    }

    fn last_child_indent(mut self, last_child_indent: impl Into<Arc<str>>) -> Args {
        self.last_child_indent = Some(last_child_indent.into());
        self
    }

    fn activated(mut self) -> Args {
        self.activated = true;
        self
    }
}

use new_line::{is_elvis_operator, is_preceded_by_comment, start_no_indent_zone};
