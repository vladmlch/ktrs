//! Behavior of the ec4j port that ktlint depends on, with expectations read off the decompiled
//! ec4j-core 1.2.0 (`Glob`, `EditorConfigParser`, `Section.Builder.applyDefaults`, `ResourcePropertiesService`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ktrs_editorconfig::{
    ErrorType, Glob, NoCache, PropertyTypeRegistry, ResourcePropertiesService, parse,
};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("ktrs-ec4j-{}-{nanos}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn write(&self, relative_dir: &str, content: &str) {
        let dir = self.0.join(relative_dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".editorconfig"), content).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn registry() -> PropertyTypeRegistry {
    PropertyTypeRegistry::with_defaults([])
}

/// `name = source` of the effective properties of `file`, in map order.
fn query(dir: &TempDir, file: &str, keep_unset: bool) -> Vec<String> {
    let registry = registry();
    let service = ResourcePropertiesService {
        cache: &NoCache,
        config_file_name: ".editorconfig",
        default_editor_configs: Vec::new(),
        keep_unset,
        registry: &registry,
        root_directories: Vec::new(),
    };
    let properties = service.query_properties(&dir.0.join(file)).unwrap();
    properties
        .properties()
        .iter()
        .map(|p| format!("{} = {}", p.name(), p.source_value().unwrap_or("null")))
        .collect()
}

/// The properties of the only section of `text`.
fn section(text: &str) -> Vec<String> {
    let editor_config = parse(text, "test", &registry()).unwrap();
    assert_eq!(editor_config.sections().len(), 1);
    editor_config.sections()[0]
        .properties()
        .iter()
        .map(|p| format!("{} = {}", p.name(), p.source_value().unwrap_or("null")))
        .collect()
}

#[test]
fn glob_semantics() {
    assert!(
        Glob::new("*").is_match("a/b/C.kt"),
        "no slash: last segment only"
    );
    assert!(
        !Glob::new("src/*").is_match("src/a/C.kt"),
        "* does not cross /"
    );
    assert!(Glob::new("src/**").is_match("src/a/C.kt"), "** crosses /");
    assert!(Glob::new("**.kt").is_match("a/b/C.kt"));
    assert!(
        Glob::new("a/**/b.kt").is_match("a/b.kt"),
        "/**/ matches a single /"
    );
    assert!(Glob::new("?.kt").is_match("x/A.kt"));
    assert!(!Glob::new("?.kt").is_match("AB.kt"));
    assert!(Glob::new("{a,b}.kt").is_match("b.kt"));
    assert!(
        Glob::new("{a.kt").is_match("{a.kt"),
        "unmatched brace is literal"
    );
    assert!(
        Glob::new("{single}.kt").is_match("{single}.kt"),
        "a brace without comma is literal"
    );
    assert!(Glob::new("n{-3..3}.kt").is_match("n2.kt"));
    assert!(!Glob::new("*.KT").is_match("a.kt"), "case sensitive");
}

#[test]
fn root_true_stops_the_lookup() {
    let dir = TempDir::new();
    dir.write("", "[*]\nouter = 1");
    dir.write("p", "root = true\n[*]\ninner = 1");
    assert_eq!(query(&dir, "p/A.kt", true), ["inner = 1"]);
    assert_eq!(query(&dir, "A.kt", true), ["outer = 1"]);
}

#[test]
fn nearer_file_and_later_section_win_and_keep_the_first_position() {
    let dir = TempDir::new();
    dir.write("", "[*]\na = outer\nb = outer");
    dir.write("p", "[*]\nb = inner-1\n[*.kt]\na = inner-2\nc = inner-2");
    assert_eq!(
        query(&dir, "p/A.kt", true),
        ["a = inner-2", "b = inner-1", "c = inner-2"]
    );
}

#[test]
fn unset_is_kept_or_removes_the_property() {
    let dir = TempDir::new();
    dir.write("", "[*]\na = 1\nb = 2");
    dir.write("p", "[*]\na = UNSET");
    assert_eq!(query(&dir, "p/A.kt", true), ["a = unset", "b = 2"]);
    assert_eq!(query(&dir, "p/A.kt", false), ["b = 2"]);
}

#[test]
fn indent_defaults_are_derived_per_section() {
    assert_eq!(
        section("[*]\nindent_style = tab"),
        ["indent_style = tab", "indent_size = tab"]
    );
    assert_eq!(
        section("[*]\nindent_style = Tab"),
        ["indent_style = Tab"],
        "the source value is compared case-sensitively"
    );
    assert_eq!(
        section("[*]\nindent_size = 2"),
        ["indent_size = 2", "tab_width = 2"]
    );
    assert_eq!(
        section("[*]\nindent_size = tab\ntab_width = 3"),
        ["indent_size = 3", "tab_width = 3"]
    );
    // Across sections: the second section derives indent_size = tab by itself, overriding the first's 2.
    let dir = TempDir::new();
    dir.write("", "[*]\nindent_size = 2\n[*.kt]\nindent_style = tab");
    assert_eq!(
        query(&dir, "A.kt", true),
        ["indent_size = tab", "tab_width = 2", "indent_style = tab"]
    );
}

#[test]
fn a_tab_inside_a_value_is_a_syntax_error() {
    for text in [
        "[*]\nkey = value\t\n",
        "[*]\nkey = a\tb\n",
        "[*]\nkey\t= value\n",
    ] {
        let error = parse(text, "test", &registry()).expect_err(text);
        assert_eq!(
            error.error_type,
            ErrorType::ExpectedStringCharacter,
            "{text:?}"
        );
    }
    assert_eq!(
        section("[*]\n\tkey =\tvalue"),
        ["key = value"],
        "leading tabs are whitespace"
    );
}

#[test]
fn names_are_lowercased_and_values_keep_their_case() {
    assert_eq!(section("[*]\nSome_Key = VaLuE"), ["some_key = VaLuE"]);
}

#[test]
fn comments_need_a_preceding_space() {
    assert_eq!(
        section("[*]\na = v # comment\nb = v#no-comment\nc = v ; comment"),
        ["a = v", "b = v#no-comment", "c = v"]
    );
    assert_eq!(section("[*.kt] # comment\na = 1"), ["a = 1"]);
    assert!(
        Glob::new("a\\#b").is_match("a#b"),
        "escaped comment sign in a glob"
    );
}

#[test]
fn preamble_only_sets_root_and_properties_need_an_equals_sign() {
    let editor_config = parse("root = TRUE\nignored = 1\n[*]\na = 1", "test", &registry()).unwrap();
    assert!(editor_config.is_root());
    assert_eq!(editor_config.sections().len(), 1);
    let error = parse("[*]\nno-equals-sign\n", "test", &registry()).unwrap_err();
    assert_eq!(error.error_type, ErrorType::PropertyAssignmentMissing);
    let error = parse("[*.kt\n", "test", &registry()).unwrap_err();
    assert_eq!(error.error_type, ErrorType::GlobNotClosed);
}

#[test]
fn an_invalid_value_of_a_registered_type_is_no_error() {
    let editor_config = parse("[*]\nindent_size = abc\nfoo = abc", "test", &registry()).unwrap();
    let properties = editor_config.sections()[0].properties();
    assert!(
        !properties[0].is_valid(),
        "indent_size is typed and invalid"
    );
    assert!(
        properties[1].is_valid(),
        "an unregistered property is always valid"
    );
}

#[test]
fn property_is_parsed_by_and_apply_chain_defaults() {
    use ktrs_editorconfig::property_type::{INDENT_SIZE, INDENT_STYLE};
    use std::sync::Arc;

    let editor_config = parse("[*]\nindent_size = 4", "test", &registry()).unwrap();
    let prop = &editor_config.sections()[0].properties()[0];
    assert!(prop.is_parsed_by(&INDENT_SIZE));
    assert!(!prop.is_parsed_by(&INDENT_STYLE));

    let def1 = parse("[*]\nindent_size = 1\nprop1 = a\nprop2 = a", "def1", &registry()).unwrap();
    let def2 = parse("[*]\nindent_size = 2\nprop2 = b\nprop3 = b", "def2", &registry()).unwrap();
    let parent_config = parse("[*]\nindent_size = 8", "parent", &registry()).unwrap();
    let child_config = parse("[*]\nindent_size = 4", "child", &registry()).unwrap();

    let service = ResourcePropertiesService {
        cache: &NoCache,
        config_file_name: ".editorconfig",
        default_editor_configs: vec![Arc::new(def1), Arc::new(def2)],
        keep_unset: false,
        registry: &registry(),
        root_directories: Vec::new(),
    };
    // chain is nearest first: [child, parent]
    let chain = vec![
        (PathBuf::from("/root/child"), Arc::new(child_config)),
        (PathBuf::from("/root"), Arc::new(parent_config)),
    ];
    let res = service.apply_chain(std::path::Path::new("/root/child/file.kt"), &chain);

    let get_val = |name: &str| -> Option<String> {
        res.properties()
            .iter()
            .find(|p| p.name() == name)
            .and_then(|p| p.source_value())
            .map(|s| s.to_string())
    };

    // Child config overrides parent config and defaults
    assert_eq!(get_val("indent_size"), Some("4".to_string()));
    // Earlier default (def1) overrides later default (def2)
    assert_eq!(get_val("prop2"), Some("a".to_string()));
    // def1 is retained
    assert_eq!(get_val("prop1"), Some("a".to_string()));
    // def2 is retained when not overridden by def1
    assert_eq!(get_val("prop3"), Some("b".to_string()));
}
