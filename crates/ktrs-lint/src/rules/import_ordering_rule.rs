//! Port of ktlint-ruleset-standard `ImportOrderingRule.kt`. The layout comes from `ij_kotlin_imports_layout`
//! (upstream KDoc: `*,java.**,javax.**,kotlin.**,^` is IntelliJ IDEA's order, `*` the ASCII order).

use std::sync::LazyLock;

use ktrs_ast::psi::{ImportPath, KtImportDirective};
use ktrs_ast::{Ast, NodeId};
use ktrs_editorconfig::{PropertyType, PropertyValue};
use ktrs_syntax::SyntaxKind::{BLOCK_COMMENT, EOL_COMMENT, IMPORT_DIRECTIVE, IMPORT_LIST, WHITE_SPACE};

use crate::ast_node_extension::AstNodeExtension;
use crate::editorconfig::{EditorConfigProperty, PropertyRef};
use crate::rule::{About, EditorConfig, Emit, RuleId, RuleV2, TokenSet};
use crate::rules::STANDARD_RULE_ABOUT;
use crate::rules::internal::importordering::{ImportSorter, PatternEntry, parse_imports_layout};
use crate::rules::internal::kotlin_string::is_blank;

const VISITED_TYPES: TokenSet = TokenSet::create(&[IMPORT_LIST]);

pub struct ImportOrderingRule {
    imports_layout: Vec<PatternEntry>,
    import_sorter: ImportSorter,
}

impl ImportOrderingRule {
    pub fn new() -> ImportOrderingRule {
        ImportOrderingRule { imports_layout: Vec::new(), import_sorter: ImportSorter::new(Vec::new()) }
    }
}

impl Default for ImportOrderingRule {
    fn default() -> Self {
        Self::new()
    }
}

impl RuleV2 for ImportOrderingRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:import-ordering")
    }

    fn visited_types(&self) -> Option<TokenSet> {
        Some(VISITED_TYPES)
    }

    fn about(&self) -> About {
        STANDARD_RULE_ABOUT
    }

    fn uses_editor_config_properties(&self) -> Vec<PropertyRef> {
        vec![PropertyRef::from(&*IJ_KOTLIN_IMPORTS_LAYOUT_PROPERTY)]
    }

    fn before_first_node(&mut self, editor_config: &EditorConfig) {
        self.imports_layout = editor_config.get(&IJ_KOTLIN_IMPORTS_LAYOUT_PROPERTY);
        self.import_sorter = ImportSorter::new(self.imports_layout.clone());
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        if ast.element_type(node) != IMPORT_LIST {
            return;
        }
        let mut children = Vec::new();
        ast.get_children(node, &mut children);
        if children.is_empty() {
            return;
        }
        // Get unique imports and blank lines
        let (auto_correct_duplicate_imports, imports) = get_unique_imports_and_blank_lines(ast, &children, emit);

        let has_comments = children.iter().any(|&it| matches!(ast.element_type(it), BLOCK_COMMENT | EOL_COMMENT));
        let sorted_imports = self.sorted_imports(ast, &imports);

        // insert blank lines wherever needed, based on the patterns between the indexes of each pair of imports
        let mut sorted_imports_with_spaces: Vec<NodeId> = Vec::new();
        let mut prev: Option<(NodeId, i32)> = None;
        for (current, index2) in sorted_imports {
            let index1 = prev.map_or(-1, |(_, index)| index);
            let has_blank_lines =
                ((index1 + 1)..index2).any(|i| self.import_sorter.patterns[i as usize].is_blank_line_entry());
            if has_blank_lines && prev.is_some() {
                sorted_imports_with_spaces.push(ast.new_leaf(WHITE_SPACE, "\n\n"));
            }
            sorted_imports_with_spaces.push(current);
            prev = Some((current, index2));
        }

        if has_comments {
            let message = format!("{} -- no autocorrection due to comments in the import list", self.error_message());
            emit(ast, ast.start_offset(node), &message, false);
        } else {
            let auto_correct_whitespace = has_too_much_whitespace(ast, &children) && !self.is_custom_layout();
            let auto_correct_sort_order = !imports_are_equal(ast, &imports, &sorted_imports_with_spaces);
            let mut autocorrect = auto_correct_duplicate_imports;
            if auto_correct_sort_order || auto_correct_whitespace {
                emit(ast, ast.start_offset(node), self.error_message(), true).if_autocorrect_allowed(|| autocorrect = true);
            }
            if autocorrect {
                let first = ast.first_child_node(node).expect("NullPointerException: firstChildNode");
                let last_next = ast.last_child_node(node).and_then(|last| ast.next_sibling(last));
                ast.remove_range(node, first, last_next);
                let (&last, rest) = sorted_imports_with_spaces
                    .split_last()
                    .expect("UnsupportedOperationException: Empty collection can't be reduced.");
                for (i, &current) in rest.iter().enumerate() {
                    let next = sorted_imports_with_spaces[i + 1];
                    ast.add_child(node, current, None);
                    if !ast.is_white_space(current) && !ast.is_white_space(next) {
                        let white_space = ast.new_leaf(WHITE_SPACE, "\n");
                        ast.add_child(node, white_space, None);
                    }
                }
                ast.add_child(node, last, None);
            }
        }
    }
}

impl ImportOrderingRule {
    /// The import directives sorted by the [`ImportSorter`] (a stable sort, like `sortedWith`), each with
    /// its `findImportIndex`.
    fn sorted_imports(&self, ast: &Ast, imports: &[NodeId]) -> Vec<(NodeId, i32)> {
        let mut keyed: Vec<(NodeId, (i32, Vec<u16>))> = imports
            .iter()
            .filter(|&&it| KtImportDirective::is(ast, it))
            .map(|&it| (it, self.import_sorter.sort_key(&import_path(ast, it))))
            .collect();
        keyed.sort_by(|(_, a), (_, b)| a.cmp(b));
        keyed.into_iter().map(|(it, (index, _))| (it, index)).collect()
    }

    /// `ERROR_MESSAGES.getOrDefault(importsLayout, CUSTOM_ERROR_MESSAGE)`.
    fn error_message(&self) -> &'static str {
        if self.imports_layout == *IDEA_PATTERN {
            IDEA_ERROR_MESSAGE
        } else if self.imports_layout == *ASCII_PATTERN {
            ASCII_ERROR_MESSAGE
        } else {
            CUSTOM_ERROR_MESSAGE
        }
    }

    fn is_custom_layout(&self) -> bool {
        self.imports_layout != *IDEA_PATTERN && self.imports_layout != *ASCII_PATTERN
    }
}

/// `(psi as KtImportDirective).importPath!!`.
fn import_path(ast: &Ast, node: NodeId) -> ImportPath {
    KtImportDirective::of(ast, node).import_path(ast).expect("NullPointerException: importPath")
}

fn get_unique_imports_and_blank_lines(ast: &Ast, children: &[NodeId], emit: &mut Emit<'_>) -> (bool, Vec<NodeId>) {
    let mut auto_correct_duplicate_imports = false;
    let mut imports = Vec::new();
    let mut import_text_set: Vec<String> = Vec::new();

    for &current in children {
        if ast.is_white_space(current) && ast.newline_count(current) > 1 {
            imports.push(current);
        } else if ast.element_type(current) == IMPORT_DIRECTIVE {
            let text = ast.text(current);
            if !import_text_set.contains(&text) {
                import_text_set.push(text);
                imports.push(current);
            } else {
                emit(ast, ast.start_offset(current), &format!("Duplicate '{text}' found"), true)
                    .if_autocorrect_allowed(|| auto_correct_duplicate_imports = true);
            }
        }
    }

    (auto_correct_duplicate_imports, imports)
}

fn imports_are_equal(ast: &Ast, actual: &[NodeId], expected: &[NodeId]) -> bool {
    if actual.len() != expected.len() {
        return false;
    }
    actual.iter().zip(expected).all(|(&first, &second)| {
        if ast.is_white_space(first) && ast.is_white_space(second) {
            return ast.text(first) == ast.text(second);
        }
        first == second
    })
}

fn has_too_much_whitespace(ast: &Ast, nodes: &[NodeId]) -> bool {
    nodes.iter().any(|&it| ast.is_white_space_without_newline(it))
}

/// Alphabetical with capital letters before lower case letters, in a single group (Android's style guide).
static ASCII_PATTERN: LazyLock<Vec<PatternEntry>> = LazyLock::new(|| parse_imports_layout("*").unwrap());

/// IntelliJ IDEA's default: alphabetical, then `java`, `javax`, `kotlin` and aliases at the end.
static IDEA_PATTERN: LazyLock<Vec<PatternEntry>> =
    LazyLock::new(|| parse_imports_layout("*,java.**,javax.**,kotlin.**,^").unwrap());

const IDEA_ERROR_MESSAGE: &str = "Imports must be ordered in lexicographic order without any empty lines in-between with \"java\", \
     \"javax\", \"kotlin\" and aliases in the end";
const ASCII_ERROR_MESSAGE: &str = "Imports must be ordered in lexicographic order without any empty lines in-between";
const CUSTOM_ERROR_MESSAGE: &str = "Imports must be ordered according to the pattern specified in .editorconfig";

/// `EDITOR_CONFIG_PROPERTY_PARSER`; the `idea`/`ascii` deprecation warnings are not logged.
fn editor_config_property_parser(_name: &str, value: Option<&str>) -> PropertyValue<Vec<PatternEntry>> {
    match value {
        None => PropertyValue::invalid(value, "Import layout must contain at least one entry of a wildcard symbol (*)".to_owned()),
        Some(v) if is_blank(v) => {
            PropertyValue::invalid(value, "Import layout must contain at least one entry of a wildcard symbol (*)".to_owned())
        }
        Some("idea") => PropertyValue::valid(value, Some(IDEA_PATTERN.clone())),
        Some("ascii") => PropertyValue::valid(value, Some(ASCII_PATTERN.clone())),
        Some(v) => match parse_imports_layout(v) {
            Ok(layout) => PropertyValue::valid(value, Some(layout)),
            Err(_) => PropertyValue::invalid(value, format!("Unexpected imports layout: {v}")),
        },
    }
}

static IJ_KOTLIN_IMPORTS_LAYOUT_PROPERTY_TYPE: PropertyType<Vec<PatternEntry>> = PropertyType {
    name: "ij_kotlin_imports_layout",
    description: "Defines imports order layout for Kotlin files",
    parser: editor_config_property_parser,
    possible_values: &[],
    lower_casing: false,
};

pub static IJ_KOTLIN_IMPORTS_LAYOUT_PROPERTY: LazyLock<EditorConfigProperty<Vec<PatternEntry>>> = LazyLock::new(|| {
    EditorConfigProperty {
        android_studio_code_style_default_value: ASCII_PATTERN.clone(),
        property_writer: |it| it.iter().map(ToString::to_string).collect::<Vec<_>>().join(","),
        ..EditorConfigProperty::new(&IJ_KOTLIN_IMPORTS_LAYOUT_PROPERTY_TYPE, IDEA_PATTERN.clone())
    }
});
