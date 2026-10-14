//! Ports of ktlint-rule-engine `internal/EditorConfigDefaultsLoaderTest.kt` and
//! `internal/ThreadSafeEditorConfigCacheTest.kt`. The Java cache test counts the calls of a mock loader;
//! here the cache reads real files, so "loaded once" shows as the same `Arc` and as a file edit that
//! only becomes visible after `reload_if_exists`/`clear`.

mod engine_editorconfig_support;

use std::sync::Arc;

use engine_editorconfig_support::TestFileSystem;
use ktrs_editorconfig::property_type::{
    END_OF_LINE, INDENT_SIZE, INDENT_STYLE, INSERT_FINAL_NEWLINE, MAX_LINE_LENGTH, TAB_WIDTH,
};
use ktrs_editorconfig::{AnyPropertyType, Cache, PropertyTypeRegistry, parse};
use ktrs_lint::EditorConfigDefaults;
use ktrs_lint::engine::ThreadSafeEditorConfigCache;

const SOME_EDITOR_CONFIG: &str = "[*.kt]\nsome-property = some-property-value";

fn ec4j_property_types_used_by_ktlint() -> [&'static dyn AnyPropertyType; 6] {
    [
        &END_OF_LINE,
        &INDENT_SIZE,
        &INDENT_STYLE,
        &INSERT_FINAL_NEWLINE,
        &MAX_LINE_LENGTH,
        &TAB_WIDTH,
    ]
}

fn some_editor_config_defaults() -> EditorConfigDefaults {
    let registry = PropertyTypeRegistry::with_defaults(ec4j_property_types_used_by_ktlint());
    EditorConfigDefaults {
        value: Arc::new(parse(SOME_EDITOR_CONFIG, "some", &registry).unwrap()),
    }
}

fn load(path: Option<&std::path::Path>) -> EditorConfigDefaults {
    EditorConfigDefaults::load(path, &ec4j_property_types_used_by_ktlint()).unwrap()
}

#[test]
fn given_a_null_path_then_return_empty_editor_config_default() {
    assert_eq!(load(None), EditorConfigDefaults::empty());
}

#[test]
fn given_an_empty_path_then_return_empty_editor_config_default() {
    assert_eq!(
        load(Some(std::path::Path::new(""))),
        EditorConfigDefaults::empty()
    );
}

#[test]
fn given_an_non_existing_path_then_return_empty_editor_config_default() {
    let fs = TestFileSystem::new();
    assert_eq!(
        load(Some(&fs.resolve("path/to/non/existing/file.kt"))),
        EditorConfigDefaults::empty()
    );
}

#[test]
fn given_an_existing_editor_config_file_then_load_all_settings_from_it() {
    for file_name in [".editorconfig", "some-alternative-file-name"] {
        let fs = TestFileSystem::new();
        let some_path_to_directory = "some/path/to/directory";
        fs.write_file(some_path_to_directory, file_name, SOME_EDITOR_CONFIG);
        let actual = load(Some(
            &fs.resolve(&format!("{some_path_to_directory}/{file_name}")),
        ));
        assert_eq!(
            actual,
            some_editor_config_defaults(),
            "file name {file_name}"
        );
        assert_ne!(actual, EditorConfigDefaults::empty());
    }
}

#[test]
fn given_an_existing_directory_containing_an_editor_config_file_then_load_all_settings_from_it() {
    let fs = TestFileSystem::new();
    let some_path_to_directory = "some/path/to/directory";
    fs.write_editor_config_file(some_path_to_directory, SOME_EDITOR_CONFIG);
    assert_eq!(
        load(Some(&fs.resolve(some_path_to_directory))),
        some_editor_config_defaults()
    );
}

fn edit_config(id: &str) -> String {
    format!("[*.kt]\nsome-property = {id}")
}

fn registry() -> PropertyTypeRegistry {
    PropertyTypeRegistry::with_defaults([])
}

#[test]
fn given_a_file_which_is_requested_multiple_times_then_it_is_read_only_once_and_then_stored_into_and_retrieved_from_the_cache()
 {
    let cache = ThreadSafeEditorConfigCache::default();
    let fs = TestFileSystem::new();
    fs.write_root_editor_config_file(&edit_config("edit-config-1"));
    let file_1 = fs.resolve(".editorconfig");
    let first = cache.get(&file_1, &registry()).unwrap();
    std::fs::write(&file_1, edit_config("changed")).unwrap();
    let actual = [
        cache.get(&file_1, &registry()).unwrap(),
        cache.get(&file_1, &registry()).unwrap(),
    ];
    for a in actual {
        assert!(Arc::ptr_eq(&first, &a), "served from the cache");
    }
    assert_eq!(
        *first,
        parse(&edit_config("edit-config-1"), "", &registry()).unwrap()
    );
}

#[test]
fn given_that_multiple_files_are_stored_into_the_cache_and_one_of_those_files_is_requested_another_time_then_this_file_is_still_being_retrieved_from_the_cache()
 {
    let cache = ThreadSafeEditorConfigCache::default();
    let fs = TestFileSystem::new();
    fs.write_editor_config_file("1", &edit_config("edit-config-1"));
    fs.write_editor_config_file("2", &edit_config("edit-config-2"));
    let (file_1, file_2) = (fs.resolve("1/.editorconfig"), fs.resolve("2/.editorconfig"));
    let a1 = cache.get(&file_1, &registry()).unwrap();
    let a2 = cache.get(&file_2, &registry()).unwrap();
    let a3 = cache.get(&file_1, &registry()).unwrap();
    assert!(Arc::ptr_eq(&a1, &a3));
    assert_eq!(
        *a1,
        parse(&edit_config("edit-config-1"), "", &registry()).unwrap()
    );
    assert_eq!(
        *a2,
        parse(&edit_config("edit-config-2"), "", &registry()).unwrap()
    );
}

#[test]
fn given_that_a_file_is_stored_in_the_cache_and_then_file_is_explicitly_reloaded() {
    let cache = ThreadSafeEditorConfigCache::default();
    let fs = TestFileSystem::new();
    fs.write_root_editor_config_file(&edit_config("edit-config-1"));
    let file_1 = fs.resolve(".editorconfig");
    cache.get(&file_1, &registry()).unwrap();
    std::fs::write(&file_1, edit_config("edit-config-2")).unwrap();
    cache.reload_if_exists(&file_1).unwrap();
    assert_eq!(
        *cache.get(&file_1, &registry()).unwrap(),
        parse(&edit_config("edit-config-2"), "", &registry()).unwrap()
    );
    std::fs::write(&file_1, edit_config("edit-config-3")).unwrap();
    cache.reload_if_exists(&file_1).unwrap();
    assert_eq!(
        *cache.get(&file_1, &registry()).unwrap(),
        parse(&edit_config("edit-config-3"), "", &registry()).unwrap()
    );
    // A file that is not cached is not loaded by a reload.
    let not_cached = fs.resolve("other/.editorconfig");
    cache.reload_if_exists(&not_cached).unwrap();
    assert_eq!(cache.get_paths(), vec![file_1]);
}

#[test]
fn given_that_a_file_is_stored_in_the_cache_and_then_the_cache_is_cleared_and_the_file_is_requested_again_then_the_file_is_to_be_reloaded()
 {
    let cache = ThreadSafeEditorConfigCache::default();
    let fs = TestFileSystem::new();
    fs.write_root_editor_config_file(&edit_config("edit-config-1"));
    let file_1 = fs.resolve(".editorconfig");
    let first = cache.get(&file_1, &registry()).unwrap();
    cache.clear();
    std::fs::write(&file_1, edit_config("edit-config-2")).unwrap();
    let second = cache.get(&file_1, &registry()).unwrap();
    let third = cache.get(&file_1, &registry()).unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    assert!(Arc::ptr_eq(&second, &third));
    assert_eq!(
        *second,
        parse(&edit_config("edit-config-2"), "", &registry()).unwrap()
    );
}

#[test]
fn given_that_a_file_is_stored_in_the_cache_and_then_reloaded_with_syntax_error_it_is_evicted() {
    let cache = ThreadSafeEditorConfigCache::default();
    let fs = TestFileSystem::new();
    fs.write_root_editor_config_file(&edit_config("edit-config-1"));
    let file_1 = fs.resolve(".editorconfig");
    cache.get(&file_1, &registry()).unwrap();
    assert_eq!(cache.get_paths(), vec![file_1.clone()]);

    // Overwrite with invalid editorconfig (syntax error)
    std::fs::write(&file_1, "[unclosed section\nkey = val\n").unwrap();
    let result = cache.reload_if_exists(&file_1);
    assert!(result.is_err(), "reload should fail on invalid syntax");

    // The stale entry must be gone from cache
    assert!(cache.get_paths().is_empty());

    // Subsequent get should also fail rather than returning stale cached config
    assert!(cache.get(&file_1, &registry()).is_err());
}

