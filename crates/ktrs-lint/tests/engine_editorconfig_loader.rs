//! Port of ktlint-rule-engine `internal/EditorConfigLoaderTest.kt`, first half (file lookup and parsing);
//! the null-path, non-Kotlin, stdin and override cases are in `engine_editorconfig_loader_override.rs`.

mod engine_editorconfig_support;

use engine_editorconfig_support::{
    TestFileSystem, assert_contains, assert_not_contains, convert_to_property_values,
};
use ktrs_lint::engine::editor_config_loader::{EditorConfigLoader, EditorConfigLoaderEc4j};
use ktrs_lint::{EditorConfigDefaults, EditorConfigOverride};

fn create_editor_config_loader() -> EditorConfigLoader {
    EditorConfigLoader::new(
        EditorConfigLoaderEc4j::new(&[]),
        EditorConfigDefaults::empty(),
        EditorConfigOverride::empty(),
    )
}

#[test]
fn given_an_editorconfig_file_in_the_project_root_and_the_file_to_be_linted_is_in_a_subdirectory_not_containing_an_editorconfig_file()
 {
    let editorconfig_contents = [
        "[*]\nindent_size = 2",
        "root = true\n[*]\nindent_size = 2",
        // When multiple globs match, the value of the last one wins.
        "[*]\nindent_size = 4\n[*.{kt,kts}]\nindent_size = 2",
        "[*.{kt,kts}]\nindent_size = 4\n[*]\nindent_size = 2",
    ];
    for editorconfig_content in editorconfig_contents {
        let fs = TestFileSystem::new();
        fs.write_root_editor_config_file(editorconfig_content);
        let editor_config = create_editor_config_loader()
            .load(Some(&fs.resolve("some-subdirectory/test.kt")))
            .unwrap();
        assert_contains(
            &convert_to_property_values(&editor_config),
            &["indent_size = 2"],
        );
    }
}

#[test]
fn given_editorconfig_files_at_different_levels_in_the_project_hierarchy_and_an_intermediate_editorconfig_file_contains_the_root_is_true_property_then_stop_reading_the_parent_editorconfig()
 {
    let some_relative_project_directory = "some-project-directory";
    let some_relative_project_sub_directory = "some-project-directory/some-project-sub-directory";
    let fs = TestFileSystem::new();
    fs.write_root_editor_config_file("root = true\n[*]\nsome_property_1 = some_value_1");
    fs.write_editor_config_file(
        some_relative_project_directory,
        "root = true\n[*.{kt,kts}]\nsome_property_2 = some_value_2",
    );
    fs.write_editor_config_file(
        some_relative_project_sub_directory,
        "[*]\nindent_size = 2\nsome_property_3 = some_value_3",
    );

    let actual = convert_to_property_values(
        &create_editor_config_loader()
            .load(Some(&fs.resolve(&format!(
                "{some_relative_project_sub_directory}/test.kt"
            ))))
            .unwrap(),
    );
    assert_contains(
        &actual,
        &[
            "some_property_2 = some_value_2",
            "some_property_3 = some_value_3",
        ],
    );
    assert_not_contains(&actual, &["some_property_1 = some_value_1"]);

    let actual = convert_to_property_values(
        &create_editor_config_loader()
            .load(Some(&fs.resolve(&format!(
                "{some_relative_project_directory}/test.kt"
            ))))
            .unwrap(),
    );
    assert_contains(&actual, &["some_property_2 = some_value_2"]);
    assert_not_contains(&actual, &["some_property_1 = some_value_1"]);

    let actual = convert_to_property_values(
        &create_editor_config_loader()
            .load(Some(&fs.resolve("test.kt")))
            .unwrap(),
    );
    assert_contains(&actual, &["some_property_1 = some_value_1"]);
}

#[test]
fn should_parse_properties_with_and_without_spaces_before_or_after_the_equals() {
    let fs = TestFileSystem::new();
    fs.write_root_editor_config_file(
        "[*.{kt,kts}]\nsome_property_1 = some_value_1\nsome_property_2= some_value_2\nsome_property_3 =some_value_3\nsome_property_4=some_value_4",
    );
    let actual = convert_to_property_values(
        &create_editor_config_loader()
            .load(Some(&fs.resolve("test.kt")))
            .unwrap(),
    );
    assert_contains(
        &actual,
        &[
            "some_property_1 = some_value_1",
            "some_property_2 = some_value_2",
            "some_property_3 = some_value_3",
            "some_property_4 = some_value_4",
        ],
    );
}

#[test]
fn should_parse_unset_values() {
    let fs = TestFileSystem::new();
    fs.write_root_editor_config_file("[*.{kt,kts}]\nsome_property_1 = unset");
    let actual = convert_to_property_values(
        &create_editor_config_loader()
            .load(Some(&fs.resolve("test.kt")))
            .unwrap(),
    );
    assert_contains(&actual, &["some_property_1 = unset"]);
}

#[test]
fn given_a_property_with_a_comma_separated_list_of_values_with_or_without_spaces_around_the_comma_the_return_the_value_inclusive_those_spaces()
 {
    let fs = TestFileSystem::new();
    fs.write_root_editor_config_file(
        "[*.{kt,kts}]\nsome_property_1=some_value_1,some_value_2\nsome_property_2=some_value_1 ,some_value_2\n\
         some_property_3=some_value_1, some_value_2\nsome_property_4=some_value_1 , some_value_2",
    );
    let actual = convert_to_property_values(
        &create_editor_config_loader()
            .load(Some(&fs.resolve("test.kt")))
            .unwrap(),
    );
    assert_contains(
        &actual,
        &[
            "some_property_1 = some_value_1,some_value_2",
            "some_property_2 = some_value_1 ,some_value_2",
            "some_property_3 = some_value_1, some_value_2",
            "some_property_4 = some_value_1 , some_value_2",
        ],
    );
}

#[test]
fn should_support_editorconfig_globs_when_loading_properties_for_file_specified_under_such_glob() {
    let fs = TestFileSystem::new();
    fs.write_root_editor_config_file(
        "[*.{kt,kts}]\nsome_property_1 = some_value_1\nsome_property_2 = some_value_2\n\n\
         [api/*.{kt,kts}]\nsome_property_1 = some_value_2\nsome_property_3 = some_value_3",
    );
    let actual = convert_to_property_values(
        &create_editor_config_loader()
            .load(Some(&fs.resolve("api/test.kt")))
            .unwrap(),
    );
    assert_contains(
        &actual,
        &[
            "some_property_1 = some_value_2",
            "some_property_2 = some_value_2",
            "some_property_3 = some_value_3",
        ],
    );
}

#[test]
fn bounded_dir_chains_eviction_works_across_many_directories() {
    let fs = TestFileSystem::new();
    fs.write_root_editor_config_file("root = true\n[*]\nindent_size = 2");
    let loader = create_editor_config_loader();
    for i in 0..600 {
        let file_path = fs.resolve(&format!("dir_{i}/Test.kt"));
        let actual = loader.load(Some(&file_path)).unwrap();
        assert_contains(
            &convert_to_property_values(&actual),
            &["indent_size = 2"],
        );
    }
    assert_eq!(loader.dir_chains_len(), 512);
    let file_path = fs.resolve("dir_0/Test.kt");
    let actual = loader.load(Some(&file_path)).unwrap();
    assert_contains(
        &convert_to_property_values(&actual),
        &["indent_size = 2"],
    );
    assert_eq!(loader.dir_chains_len(), 512);
}

#[test]
fn clear_dir_chains_empties_memoization() {
    let fs = TestFileSystem::new();
    fs.write_root_editor_config_file("root = true\n[*]\nindent_size = 2");
    let loader = create_editor_config_loader();
    let file_path = fs.resolve("some_dir/Test.kt");
    let _ = loader.load(Some(&file_path)).unwrap();
    assert_eq!(loader.dir_chains_len(), 1);
    loader.clear_dir_chains();
    assert_eq!(loader.dir_chains_len(), 0);
    let actual = loader.load(Some(&file_path)).unwrap();
    assert_contains(
        &convert_to_property_values(&actual),
        &["indent_size = 2"],
    );
    assert_eq!(loader.dir_chains_len(), 1);
}

