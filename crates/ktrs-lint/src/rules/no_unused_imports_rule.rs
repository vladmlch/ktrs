//! Port of ktlint-ruleset-standard `NoUnusedImportsRule.kt`. Opt-in (`OnlyWhenEnabledInEditorconfig`), as an
//! import is occasionally falsely marked as unused (ktlint#3038); it ignores ktlint suppressions, so imports only
//! used in suppressed code are not reported.

use std::borrow::Cow;
use std::collections::HashSet;

use ktrs_ast::psi::{self, ImportPath, KtDotQualifiedExpression, KtImportDirective, KtPackageDirective};
use ktrs_ast::{Ast, NodeId};
use ktrs_syntax::SyntaxKind::{
    BY_KEYWORD, CALL_EXPRESSION, DOT_QUALIFIED_EXPRESSION, FILE, IDENTIFIER, IMPORT_DIRECTIVE, KDOC_MARKDOWN_LINK,
    OPERATION_REFERENCE, PACKAGE_DIRECTIVE, REFERENCE_EXPRESSION,
};

use crate::ast_node_edit::AstNodeEdit;
use crate::ast_node_extension::AstNodeExtension;
use crate::rule::{About, Emit, RuleId, RuleV2, TokenSet};
use crate::rules::STANDARD_RULE_ABOUT;
use crate::rules::internal::kotlin_string::{is_blank, remove_surrounding, substring_after, substring_before, substring_before_last, trim};

const VISITED_TYPES: TokenSet = TokenSet::create(&[
    PACKAGE_DIRECTIVE,
    IMPORT_DIRECTIVE,
    DOT_QUALIFIED_EXPRESSION,
    KDOC_MARKDOWN_LINK,
    REFERENCE_EXPRESSION,
    OPERATION_REFERENCE,
    BY_KEYWORD,
]);

pub struct NoUnusedImportsRule {
    /// The set of `Reference(text, inDotQualifiedExpression)`, split by the flag: `[false]`, `[true]`.
    r#ref: [HashSet<String>; 2],
    /// A `LinkedHashSet`: its order is the emit order.
    parent_expressions: Vec<String>,
    /// A `LinkedHashMap`.
    imports: Vec<(ImportPath, NodeId)>,
    /// `importPath.pathStr.removeBackticksAndTrim()` of each of `imports`, rendered once.
    import_paths: Vec<String>,
    package_name: String,
    found_by_keyword: bool,
}

impl NoUnusedImportsRule {
    pub fn new() -> NoUnusedImportsRule {
        NoUnusedImportsRule {
            r#ref: [HashSet::from(["*".to_owned()]), HashSet::new()],
            parent_expressions: Vec::new(),
            imports: Vec::new(),
            import_paths: Vec::new(),
            package_name: String::new(),
            found_by_keyword: false,
        }
    }
}

impl Default for NoUnusedImportsRule {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleV2 for NoUnusedImportsRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:no-unused-imports")
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(VISITED_TYPES)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn is_only_when_enabled_in_editorconfig(&self) -> bool {
        true
    }

    fn ignores_ktlint_suppressions(&self) -> bool {
        true
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        match ast.element_type(node) {
            PACKAGE_DIRECTIVE => {
                let package_directive = KtPackageDirective::of(ast, node);
                self.package_name = package_directive.qualified_name(ast);
            }

            IMPORT_DIRECTIVE => {
                let import_path = KtImportDirective::of(ast, node).import_path(ast).expect("NullPointerException: importPath");
                if self.imports.iter().any(|(it, _)| *it == import_path) {
                    // Emit directly when same import occurs more than once
                    emit(ast, ast.start_offset(node), "Unused import", true).if_autocorrect_allowed(|| psi::delete(ast, node));
                } else {
                    self.import_paths.push(remove_backticks_and_trim(&import_path.path_str()).into_owned());
                    self.imports.push((import_path, node));
                }
            }

            DOT_QUALIFIED_EXPRESSION => {
                if self.is_expression_for_static_import_with_existing_parent_import(ast, node) {
                    let parent = substring_before_last(&ast.text(node), "(").to_owned();
                    if !self.parent_expressions.contains(&parent) {
                        self.parent_expressions.push(parent);
                    }
                }
            }

            KDOC_MARKDOWN_LINK => {
                let text = ast.text(node);
                let link_text = remove_backticks_and_trim(remove_surrounding(&text, "[", "]"));
                self.add_reference(substring_before(&link_text, "."), false);
                self.add_reference(link_text.rsplit('.').next().unwrap_or_default(), false);
            }

            REFERENCE_EXPRESSION | OPERATION_REFERENCE => {
                if !ast.is_part_of(node, IMPORT_DIRECTIVE) {
                    let identifier = if !ast.is_leaf_element(node) { ast.find_child_by_type(node, IDENTIFIER) } else { Some(node) };
                    if let Some(text) = identifier.map(|it| ast.leaf_text(it)).filter(|it| !is_blank(it)) {
                        self.add_reference(&remove_backticks_and_trim(text), is_parent_dot_qualified_expression_or_null(ast, node));
                    }
                }
            }

            BY_KEYWORD => {
                self.found_by_keyword = true;
            }

            _ => {}
        }
    }

    fn visits_after_child_nodes(&self) -> bool {
        true
    }

    fn visited_after_types(&self) -> Option<TokenSet> {
        Some(TokenSet::create(&[FILE]))
    }

    fn after_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if ast.element_type(node) != FILE {
            return;
        }
        let direct_calls: Vec<String> = self.r#ref[0].iter().cloned().collect();
        for parent in self.parent_expressions.clone() {
            let matching: Vec<(ImportPath, NodeId)> = self
                .imports
                .iter()
                .zip(&self.import_paths)
                .filter(|(_, import_path)| {
                    import_path.ends_with(&format!(".{parent}")) && !direct_calls.iter().any(|it| import_path.ends_with(&format!(".{it}")))
                })
                .map(|(import, _)| import.clone())
                .collect();
            for (import_path, import_node) in matching {
                emit(ast, ast.start_offset(import_node), "Unused import", true).if_autocorrect_allowed(|| {
                    // Keys are unique: a repeated import is emitted at once, never added.
                    if let Some(i) = self.imports.iter().position(|(p, n)| *p == import_path && *n == import_node) {
                        self.imports.remove(i);
                        self.import_paths.remove(i);
                    }
                    remove_import_directive(ast, import_node);
                });
            }
        }

        let referenced: HashSet<&str> = self.r#ref.iter().flatten().map(String::as_str).collect();
        let valid_import_paths: HashSet<&str> = self.import_paths.iter().map(String::as_str).collect();
        for &(_, node) in &self.imports {
            let import_directive = KtImportDirective::of(ast, node);
            let path = import_directive.import_path(ast);
            let name = path.as_ref().and_then(ImportPath::imported_name).map(|it| remove_backticks_and_trim(&it).into_owned());
            let path_str = path.expect("NullPointerException: importPath").path_str();
            let import_path = remove_backticks_and_trim(&path_str);
            let package_name = &self.package_name;
            if import_directive.alias_name(ast).is_none()
                && (package_name.is_empty() || import_path.starts_with(&format!("{package_name}.")))
                && !skip_chars(&import_path, package_name.chars().count() + 1).contains('.')
            {
                // Allow imports without alias for which the fully qualified path is equal to the package name
                // (ktlint#2821: marking an import from the same package led to compile failure).
            } else if name.as_ref().is_some_and(|name| {
                (!referenced.contains(name.as_str()) || !valid_import_paths.contains(import_path.as_ref()))
                    && !OPERATOR_SET.contains(&name.as_str())
                    && !is_component_n(name)
                    && !self.ignore_provide_delegate(&import_path)
            }) {
                emit(ast, ast.start_offset(node), "Unused import", true).if_autocorrect_allowed(|| {
                    match ast.next_sibling(node) {
                        None => {
                            // Last import
                            let whitespace = ast
                                .next_leaf(ast.last_child_leaf_or_self(node))
                                .filter(|&it| ast.is_white_space_with_newline(it));
                            if let Some(whitespace) = whitespace {
                                if ast.prev_leaf(node).is_none() {
                                    // Also the first import, not preceded by any text: all whitespace until the next is redundant
                                    ast.remove(whitespace);
                                } else {
                                    let text = ast.text(whitespace);
                                    let text_after_first_newline = substring_after(&text, "\n");
                                    if !is_blank(text_after_first_newline) {
                                        ast.replace_text_with(whitespace, text_after_first_newline);
                                    }
                                }
                            }
                        }
                        Some(next_sibling) => {
                            if ast.is_white_space_with_newline(next_sibling) {
                                ast.remove(next_sibling);
                            }
                        }
                    }
                    psi::delete(ast, import_directive.node());
                });
            }
        }
    }
}

impl NoUnusedImportsRule {
    /// `ref.add(Reference(text, inDotQualifiedExpression))`; allocates only for a new text.
    fn add_reference(&mut self, text: &str, in_dot_qualified_expression: bool) {
        let set = &mut self.r#ref[usize::from(in_dot_qualified_expression)];
        if !set.contains(text) {
            set.insert(text.to_owned());
        }
    }

    fn ignore_provide_delegate(&self, import_path: &str) -> bool {
        if import_path.ends_with(".provideDelegate") {
            // Ignore provideDelegate if the `by` keyword is found anywhere in the file
            self.found_by_keyword
        } else {
            false
        }
    }

    fn is_expression_for_static_import_with_existing_parent_import(&self, ast: &Ast, node: NodeId) -> bool {
        // Without imports every path below returns false; skips building the text of every expression.
        if self.imports.is_empty() {
            return false;
        }
        // The uppercase test below sees the text's first char unless the last `(` starts the text.
        let first = ast.text_chunks(node).find(|chunk| !chunk.is_empty()).map(|chunk| chunk.as_bytes()[0]);
        if first.is_none_or(|b| b != b'(' && !b.is_ascii_uppercase()) {
            return false;
        }
        let text = ast.text(node);
        if !contains_method_call(&text) {
            return false;
        }

        let method_call_expression = substring_before_last(&text, "(");

        // Only check static imports; identified if they start with a capital letter indicating a class name
        // rather than a sub-package
        if !method_call_expression.is_empty() && !method_call_expression.as_bytes()[0].is_ascii_uppercase() {
            return false;
        }

        let paths = &self.import_paths;
        for import in paths.iter().filter(|it| it.ends_with(&format!(".{method_call_expression}"))) {
            let prefix = substring_before(import, method_call_expression);
            let count = paths.iter().filter(|it| it.starts_with(prefix)).count();
            // Parent import and static import both are present
            if count > 1 {
                return true;
            }
        }
        false
    }

    /// Whether the import being checked is present in the filtered import list.
    #[allow(dead_code)]
    fn is_a_valid_import(&self, import_path: &str) -> bool {
        self.import_paths.iter().any(|it| it == import_path)
    }
}

fn remove_import_directive(ast: &mut Ast, this: NodeId) {
    assert!(ast.element_type(this) == IMPORT_DIRECTIVE, "IllegalArgumentException: Failed requirement.");
    let parent = ast.parent(this);
    let neighbour = if parent.and_then(|p| ast.first_child_node(p)) == Some(this) {
        ast.next_sibling(this)
    } else if parent.and_then(|p| ast.last_child_node(p)) == Some(this) {
        ast.prev_sibling(this)
    } else {
        ast.next_leaf(this)
    };
    if let Some(whitespace) = neighbour.filter(|&it| ast.is_white_space_with_newline(it)) {
        ast.remove(whitespace);
    }
    ast.remove(this);
}

fn contains_method_call(text: &str) -> bool {
    text.rsplit('.').next().unwrap_or_default().contains('(')
}

/// `COMPONENT_N_REGEX` = `^component\d+$`.
fn is_component_n(name: &str) -> bool {
    name.strip_prefix("component").is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

fn is_parent_dot_qualified_expression_or_null(ast: &Ast, node: NodeId) -> bool {
    let call_expression_or_this = parent_call_expression_or_null(ast, node).unwrap_or(node);
    is_dot_qualified_expression(ast, call_expression_or_this)
}

fn parent_call_expression_or_null(ast: &Ast, node: NodeId) -> Option<NodeId> {
    ast.parent(node).filter(|&it| ast.element_type(it) == CALL_EXPRESSION)
}

fn is_dot_qualified_expression(ast: &Ast, node: NodeId) -> bool {
    ast.parent(node)
        .filter(|&it| ast.element_type(it) == DOT_QUALIFIED_EXPRESSION)
        .and_then(|it| KtDotQualifiedExpression::cast(ast, it))
        .is_some_and(|it| it.selector_expression(ast) == Some(node))
}

fn remove_backticks_and_trim(s: &str) -> Cow<'_, str> {
    if s.contains('`') { Cow::Owned(trim(&s.replace('`', "")).to_owned()) } else { Cow::Borrowed(trim(s)) }
}

/// `substring(n)`, `n` counted in chars (UTF-16 units upstream; the same cut for the callers' prefixes).
fn skip_chars(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or("", |(i, _)| &s[i..])
}

const OPERATOR_SET: &[&str] = &[
    // unary
    "unaryPlus", "unaryMinus", "not",
    // inc/dec
    "inc", "dec",
    // arithmetic
    "plus", "minus", "times", "div", "rem", "mod", "rangeTo", "rangeUntil",
    // in
    "contains",
    // indexed access
    "get", "set",
    // invoke
    "invoke",
    // (augmented) assignment
    "assign", "plusAssign", "minusAssign", "timesAssign", "divAssign", "modAssign",
    // (in)equality
    "equals",
    // comparison
    "compareTo",
    // iteration (https://github.com/shyiko/ktlint/issues/40)
    "iterator",
    // by (https://github.com/shyiko/ktlint/issues/54)
    "getValue", "setValue",
];
