use std::collections::BTreeSet;

use ktrs_lint::editorconfig::KtlintVersion;
use ktrs_lint::rule_provider::rule_providers_in;
use ktrs_lint::rules::standard_rule_providers;

fn rule_ids(version: KtlintVersion) -> Vec<String> {
    rule_providers_in(&standard_rule_providers(), version).iter().map(|p| p.rule_id().value().to_owned()).collect()
}

// StandardRuleSetProvider registers 105 rules in ktlint 2.0.0-ALPHA-4 and 101 in 1.8.0; a duplicate would run a
// rule twice per file.
#[test]
fn standard_rules_are_registered_once_each_per_version() {
    for (version, count) in [(KtlintVersion::V2_0, 105), (KtlintVersion::V1_8, 101)] {
        let ids = rule_ids(version);
        let unique: BTreeSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "duplicate rule ids registered for {version:?}");
        assert_eq!(ids.len(), count, "{version:?}");
    }
}

#[test]
fn rule_set_membership_differs_as_between_the_releases() {
    let (v1, v2): (BTreeSet<String>, BTreeSet<String>) =
        (rule_ids(KtlintVersion::V1_8).into_iter().collect(), rule_ids(KtlintVersion::V2_0).into_iter().collect());
    let only_1_8: Vec<&str> = v1.difference(&v2).map(String::as_str).collect();
    let only_2_0: Vec<&str> = v2.difference(&v1).map(String::as_str).collect();
    assert_eq!(
        only_1_8,
        ["standard:condition-wrapping", "standard:context-receiver-list-wrapping", "standard:discouraged-comment-location"]
    );
    assert_eq!(
        only_2_0,
        [
            "standard:blank-line-before-file-annotation",
            "standard:blank-line-before-imports",
            "standard:blank-line-before-package",
            "standard:call-expression-wrapping",
            "standard:context-parameter-list-wrapping",
            "standard:lambda-return",
            "standard:no-blank-line-at-start-of-file",
        ]
    );
}

#[test]
fn standard_rule_provider_lookup() {
    use ktrs_lint::rules::standard_rule_provider;

    // With prefix
    let p1 = standard_rule_provider("standard:annotation-spacing");
    assert!(p1.is_some());
    assert_eq!(p1.unwrap().rule_id().value(), "standard:annotation-spacing");

    // Without prefix
    let p2 = standard_rule_provider("annotation-spacing");
    assert!(p2.is_some());
    assert_eq!(p2.unwrap().rule_id().value(), "standard:annotation-spacing");

    // Non-existent rule
    assert!(standard_rule_provider("nonexistent-rule").is_none());
    assert!(standard_rule_provider("standard:nonexistent-rule").is_none());
    assert!(standard_rule_provider("custom:annotation-spacing").is_none());
    assert!(standard_rule_provider("").is_none());
    assert!(standard_rule_provider("standard:").is_none());
    assert!(standard_rule_provider(":").is_none());
    assert!(standard_rule_provider("standard:standard:annotation-spacing").is_none());
    assert!(standard_rule_provider("annotation-spacing:extra").is_none());
}
