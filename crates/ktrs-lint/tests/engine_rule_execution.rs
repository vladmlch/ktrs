//! Ports of ktlint-rule-engine `internal/RuleExecutionContextTest.kt`, `internal/VisitorProviderTest.kt`,
//! `internal/RuleProviderSorterTest.kt` and `api/DisabledRulesTest.kt`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ktrs_ast::{Ast, NodeId};
use ktrs_lint::editorconfig::{
    KTLINT_VERSION_PROPERTY, KtlintVersion, RuleExecution, create_rule_execution_editor_config_property,
};
use ktrs_lint::engine::visitor_provider::{VisitorProvider, get_sorted_rule_providers};
use ktrs_lint::rule::TokenSet;
use ktrs_lint::{
    About, AutocorrectDecision, Code, EditorConfig, EditorConfigOverride, Emit, KtLintException,
    KtLintRuleEngine, LintError, RuleId, RuleV2, RuleV2Provider,
};
use ktrs_syntax::SyntaxKind::FILE;

const SOME_RULE_ID: RuleId = RuleId("standard:some-rule-id");
const SOME_ABOUT: About = About {
    maintainer: "some-maintainer",
    repository_url: "some-repository-url",
    issue_tracker_url: "some-issue-tracker-url",
};
const SOME_EXCEPTION_MESSAGE: &str = "some-exception-message";

struct ThrowingRule {
    in_before_first_node: bool,
}

impl RuleV2 for ThrowingRule {
    fn rule_id(&self) -> RuleId {
        SOME_RULE_ID
    }

    fn about(&self) -> About {
        SOME_ABOUT
    }

    fn before_first_node(&mut self, _editor_config: &EditorConfig) {
        if self.in_before_first_node {
            panic!("{SOME_EXCEPTION_MESSAGE}");
        }
    }

    fn after_last_node(&mut self) {
        if !self.in_before_first_node {
            panic!("{SOME_EXCEPTION_MESSAGE}");
        }
    }
}

fn assert_wrapped_in_ktlint_rule_exception(in_before_first_node: bool) {
    let engine = KtLintRuleEngine::new(vec![RuleV2Provider::new(move || {
        Box::new(ThrowingRule {
            in_before_first_node,
        }) as Box<dyn RuleV2>
    })]);
    let result = engine.format(&Code::from_snippet("val foo = \"foo\"", false), &mut |_| {
        AutocorrectDecision::AllowAutocorrect
    });
    let Err(KtLintException::Rule(e)) = result else {
        panic!("expected a KtLintRuleException, got {result:?}")
    };
    assert_eq!(
        e.message,
        "Rule 'standard:some-rule-id' throws exception in file '<stdin>' at position (0:0)\n   Rule maintainer: some-maintainer\n   \
         Issue tracker  : some-issue-tracker-url\n   Repository     : some-repository-url"
    );
    assert_eq!(
        (e.line, e.col, e.rule_id.as_str()),
        (0, 0, "standard:some-rule-id")
    );
    assert_eq!(e.cause, SOME_EXCEPTION_MESSAGE);
}

#[test]
fn given_a_rule_that_throws_an_exception_in_the_before_first_node_callback_then_it_is_wrapped_inside_a_ktlint_rule_exception()
 {
    assert_wrapped_in_ktlint_rule_exception(true);
}

#[test]
fn given_a_rule_that_throws_an_exception_in_the_after_last_node_callback_then_it_is_wrapped_inside_a_ktlint_rule_exception()
 {
    assert_wrapped_in_ktlint_rule_exception(false);
}

#[test]
fn when_no_runnable_rules_are_found_for_the_root_node_the_visit_function_on_the_root_node_is_not_executed()
 {
    assert!(VisitorProvider::new(&[]).rules().is_empty());
}

/// `RuleProviderSorterTest.R`: must never be invoked.
struct R {
    rule_id: RuleId,
    experimental: bool,
}

impl RuleV2 for R {
    fn rule_id(&self) -> RuleId {
        self.rule_id
    }

    fn is_experimental(&self) -> bool {
        self.experimental
    }

    fn before_visit_child_nodes(&mut self, _ast: &mut Ast, _node: NodeId, _emit: &mut Emit<'_>) {
        panic!(
            "UnsupportedOperationException: Rule should never be really invoked because that is not the aim of this unit test."
        );
    }
}

fn sorted_rule_ids(rules: &[(&'static str, bool)]) -> Vec<&'static str> {
    let providers: Vec<RuleV2Provider> = rules
        .iter()
        .map(|&(id, experimental)| {
            RuleV2Provider::new(move || {
                Box::new(R {
                    rule_id: RuleId(id),
                    experimental,
                }) as Box<dyn RuleV2>
            })
        })
        .collect();
    get_sorted_rule_providers(&providers)
        .iter()
        .map(|p| p.rule_id().value())
        .collect()
}

#[test]
fn multiple_normal_rules_in_the_same_rule_set_are_run_in_alphabetical_order() {
    assert_eq!(
        sorted_rule_ids(&[("standard:rule-b", false), ("standard:rule-a", false)]),
        ["standard:rule-a", "standard:rule-b"]
    );
}

#[test]
fn multiple_normal_rules_in_different_rule_sets_are_run_in_alphabetical_order_but_grouped_standard_experimental_and_custom()
 {
    let actual = sorted_rule_ids(&[
        ("standard:rule-b", true),
        ("standard:rule-a", true),
        ("custom-rule-set-a:rule-b", false),
        ("custom-rule-set-a:rule-a", false),
        ("standard:rule-d", false),
        ("standard:rule-c", false),
        ("custom-rule-set-b:rule-b", false),
        ("custom-rule-set-b:rule-a", false),
    ]);
    assert_eq!(
        actual,
        [
            "standard:rule-a",
            "standard:rule-b",
            "standard:rule-c",
            "standard:rule-d",
            "custom-rule-set-a:rule-a",
            "custom-rule-set-a:rule-b",
            "custom-rule-set-b:rule-a",
            "custom-rule-set-b:rule-b",
        ]
    );
}

const SOME_NO_VAR_RULE_VIOLATION: &str = "some-no-var-rule-violation";

/// `DisabledRulesTest.NoVarRule`: a violation at every node.
struct NoVarRule(RuleId);

impl RuleV2 for NoVarRule {
    fn rule_id(&self) -> RuleId {
        self.0
    }

    fn before_visit_child_nodes(&mut self, ast: &mut Ast, node: NodeId, emit: &mut Emit<'_>) {
        emit(
            ast,
            ast.start_offset(node),
            SOME_NO_VAR_RULE_VIOLATION,
            false,
        );
    }
}

fn lint_no_var(
    rule_id: &'static str,
    editor_config_override: EditorConfigOverride,
) -> Vec<LintError> {
    let provider =
        RuleV2Provider::new(move || Box::new(NoVarRule(RuleId(rule_id))) as Box<dyn RuleV2>);
    let engine = KtLintRuleEngine::with_editor_config(
        vec![provider],
        Default::default(),
        editor_config_override,
    );
    let mut errors = Vec::new();
    engine
        .lint(&Code::from_snippet("var foo", false), &mut |e| {
            errors.push(e.clone())
        })
        .unwrap();
    errors
}

#[test]
fn given_some_code_and_an_enabled_standard_rule_resulting_in_a_violation_then_the_violation_is_reported()
 {
    let expected = LintError {
        line: 1,
        col: 1,
        rule_id: RuleId("test:some-rule-id"),
        detail: SOME_NO_VAR_RULE_VIOLATION.to_owned(),
        can_be_auto_corrected: false,
    };
    assert!(lint_no_var("test:some-rule-id", EditorConfigOverride::empty()).contains(&expected));
}

#[test]
fn given_a_rule_that_is_disabled_via_property_ktlint_some_rule_id_then_no_violation_is_reported() {
    for (rule_id, disabled_rule_id) in [
        ("standard:no-var", "standard:no-var"),
        ("custom:no-var", "custom:no-var"),
    ] {
        let editor_config_override = EditorConfigOverride::from(vec![(
            create_rule_execution_editor_config_property(disabled_rule_id, RuleExecution::Disabled)
                .into(),
            Some("disabled".to_owned()),
        )]);
        assert_eq!(
            lint_no_var(rule_id, editor_config_override),
            [],
            "{rule_id}"
        );
    }
}

struct ThrowingVisitRule;

impl RuleV2 for ThrowingVisitRule {
    fn rule_id(&self) -> RuleId {
        SOME_RULE_ID
    }

    fn about(&self) -> About {
        SOME_ABOUT
    }

    fn before_visit_child_nodes(&mut self, _ast: &mut Ast, _node: NodeId, _emit: &mut Emit<'_>) {
        panic!("boom in visit");
    }
}

#[test]
fn given_a_rule_that_throws_in_visit_in_1_8_mode_then_it_is_wrapped_in_ktlint_rule_exception() {
    let editor_config_override = EditorConfigOverride::empty().with(&KTLINT_VERSION_PROPERTY, KtlintVersion::V1_8);
    let engine = KtLintRuleEngine::with_editor_config(
        vec![RuleV2Provider::new(|| Box::new(ThrowingVisitRule) as Box<dyn RuleV2>)],
        Default::default(),
        editor_config_override,
    );
    let result = engine.lint(&Code::from_snippet("val x = 1", false), &mut |_| {});
    let Err(KtLintException::Rule(e)) = result else {
        panic!("expected KtLintRuleException, got {result:?}");
    };
    assert_eq!(e.rule_id, SOME_RULE_ID.value());
    assert_eq!(e.cause, "boom in visit");
    assert_eq!((e.line, e.col), (1, 1));
}

struct FilteredAfterRule {
    after_visited_count: Arc<AtomicUsize>,
}

impl RuleV2 for FilteredAfterRule {
    fn rule_id(&self) -> RuleId {
        RuleId("standard:filtered-after")
    }

    fn visits_after_child_nodes(&self) -> bool {
        true
    }

    fn visited_after_types(&self) -> Option<TokenSet> {
        Some(TokenSet::create(&[FILE]))
    }

    fn after_visit_child_nodes(&mut self, _ast: &mut Ast, _node: NodeId, _emit: &mut Emit<'_>) {
        self.after_visited_count.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn visited_after_types_filters_calls_to_after_visit_child_nodes() {
    let counter_2_0 = Arc::new(AtomicUsize::new(0));
    let c2 = Arc::clone(&counter_2_0);
    let engine_2_0 = KtLintRuleEngine::new(vec![RuleV2Provider::new(move || {
        Box::new(FilteredAfterRule {
            after_visited_count: Arc::clone(&c2),
        }) as Box<dyn RuleV2>
    })]);
    engine_2_0
        .lint(&Code::from_snippet("val x = 1\nval y = 2", false), &mut |_| {})
        .unwrap();
    // Only FILE should be visited in after_visit_child_nodes
    assert_eq!(counter_2_0.load(Ordering::SeqCst), 1);

    let counter_1_8 = Arc::new(AtomicUsize::new(0));
    let c18 = Arc::clone(&counter_1_8);
    let override_1_8 =
        EditorConfigOverride::empty().with(&KTLINT_VERSION_PROPERTY, KtlintVersion::V1_8);
    let engine_1_8 = KtLintRuleEngine::with_editor_config(
        vec![RuleV2Provider::new(move || {
            Box::new(FilteredAfterRule {
                after_visited_count: Arc::clone(&c18),
            }) as Box<dyn RuleV2>
        })],
        Default::default(),
        override_1_8,
    );
    engine_1_8
        .lint(&Code::from_snippet("val x = 1\nval y = 2", false), &mut |_| {})
        .unwrap();
    // Only FILE should be visited in 1.8 too
    assert_eq!(counter_1_8.load(Ordering::SeqCst), 1);
}
