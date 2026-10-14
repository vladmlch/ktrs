//! `--changed-since <ref>` of `ktrs fmt` and `ktrs lint`, on a git repository built here.

mod ktlint_support;

use std::io::Cursor;

use ktlint_support::common::git;
use ktlint_support::{Project, Run};
use ktrs_cli::ktrs_fmt::{parse_fmt_args, run_with as run_fmt};
use ktrs_cli::ktrs_lint::{parse_lint_args, run_with as run_lint};

/// Unformatted for ktfmt and a `no-multi-spaces` violation for ktlint.
const BEFORE: &str = "val  a = 1\n";
const CHANGED: &str = "val  b = 2\n";
const FORMATTED: &str = "val b = 2\n";

/// Tag `base`: `A.kt`, `src/B.kt`, `src/D.kt`, `Gone.kt`, with `build/` ignored. Since then: `src/B.kt` changed in
/// a commit, `src/D.kt` changed and `Gone.kt` deleted without one, `src/C.kt` untracked, `build/E.kt` ignored.
fn repository() -> Project {
    let files = [("A.kt", BEFORE), ("src/B.kt", BEFORE), ("src/D.kt", BEFORE), ("Gone.kt", BEFORE), (".gitignore", "build/\n")];
    let project = Project::new("changed-since", &files);
    let commit = |message: &str| {
        git(project.dir(), &["add", "."]);
        git(project.dir(), &["commit", "-q", "-m", message]);
    };
    git(project.dir(), &["init", "-q"]);
    commit("base");
    git(project.dir(), &["tag", "base"]);
    project.write("src/B.kt", CHANGED);
    commit("change B");
    project.write("src/D.kt", CHANGED);
    project.write("src/C.kt", CHANGED);
    project.write("build/E.kt", CHANGED);
    std::fs::remove_file(project.dir().join("Gone.kt")).unwrap();
    project
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| a.to_string()).collect()
}

fn path(project: &Project, file: &str) -> String {
    file.split('/').fold(project.dir().to_path_buf(), |dir, name| dir.join(name)).display().to_string()
}

/// `ktrs fmt <args>` in the project: the usage error, or the exit code and stdout's lines.
fn fmt(project: &Project, args: &[&str]) -> Result<(i32, Vec<String>), String> {
    let mut out = Vec::new();
    let exit_code = run_fmt(&parse_fmt_args(&strings(args))?, project.dir(), Cursor::new(Vec::new()), &mut out, Vec::new())?;
    Ok((exit_code, String::from_utf8(out).unwrap().lines().map(str::to_owned).collect()))
}

/// Usage errors (`Err`) print to stderr and exit 2 here.
fn lint(project: &Project, args: &[&str]) -> Run {
    project.run_cli(args, b"", |cli, args| {
        run_lint(cli, args).unwrap_or_else(|message| {
            cli.console.err(&message);
            2
        })
    })
}

#[test]
fn fmt_check_lists_only_the_changed_files() {
    let project = repository();
    let dir = project.dir().display().to_string();
    let changed = vec![path(&project, "src/B.kt"), path(&project, "src/C.kt"), path(&project, "src/D.kt")];
    assert_eq!(fmt(&project, &["--check", "--changed-since", "base", &dir]), Ok((1, changed)));
    assert_eq!(fmt(&project, &["--check", &dir]).unwrap().1.len(), 5, "A.kt and build/E.kt without the option");
}

#[test]
fn fmt_intersects_the_changed_files_with_the_paths() {
    let project = repository();
    let (a, c) = (path(&project, "A.kt"), path(&project, "src/C.kt"));
    assert_eq!(fmt(&project, &["--check", "--changed-since=base", &a, &c]), Ok((1, vec![c])));
    assert_eq!(fmt(&project, &["--check", "--changed-since=base", &a]), Ok((0, vec![])), "nothing left is not an error");
}

#[test]
fn fmt_writes_only_the_changed_files() {
    let project = repository();
    let dir = project.dir().display().to_string();
    assert_eq!(fmt(&project, &["--changed-since", "base", &dir]), Ok((0, vec![])));
    assert_eq!(project.read("A.kt"), BEFORE);
    assert_eq!(project.read("build/E.kt"), CHANGED);
    for file in ["src/B.kt", "src/C.kt", "src/D.kt"] {
        assert_eq!(project.read(file), FORMATTED, "{file}");
    }
}

#[test]
fn fmt_changed_since_head_is_what_is_not_committed() {
    let project = repository();
    let dir = project.dir().display().to_string();
    let changed = vec![path(&project, "src/C.kt"), path(&project, "src/D.kt")];
    assert_eq!(fmt(&project, &["--check", "--changed-since", "HEAD", &dir]), Ok((1, changed)));
}

#[test]
fn unknown_ref_and_missing_repository_are_usage_errors() {
    let project = repository();
    let dir = project.dir().display().to_string();
    let error = fmt(&project, &["--check", "--changed-since", "nope", &dir]).unwrap_err();
    assert!(error.starts_with("--changed-since: 'nope' is not a commit of this repository; in a shallow clone"), "{error}");
    assert!(fmt(&project, &["--changed-since", "--check", &dir]).unwrap_err().contains("needs a git ref"));
    let run = lint(&project, &["--changed-since", "nope"]);
    assert_eq!(run.exit_code, 2, "{run:?}");
    assert!(run.err.starts_with("--changed-since: 'nope' is not a commit of this repository"), "{run:?}");

    let plain = Project::new("no-repository", &[("A.kt", BEFORE)]);
    // The temp dir may itself be inside a repository on a developer's machine.
    if !plain.temp_dir().ancestors().any(|dir| dir.join(".git").exists()) {
        let error = fmt(&plain, &["--changed-since", "HEAD", &plain.dir().display().to_string()]).unwrap_err();
        assert!(error.contains("is not in a git repository"), "{error}");
        assert_eq!(lint(&plain, &["--changed-since", "HEAD"]).exit_code, 2);
    }
}

#[test]
fn lint_reports_only_the_changed_files() {
    let project = repository();
    let run = lint(&project, &["--changed-since", "base"]);
    assert_eq!(run.exit_code, 1, "{run:?}");
    for file in ["B", "C", "D"] {
        assert_line!(run.out, &format!(r"src.{file}\.kt:1:\d+:.*\(standard:no-multi-spaces\)"));
    }
    assert_no_line!(run.out, r"(A|Gone|build.E)\.kt:.*");
    let run = lint(&project, &["--changed-since", "base", "src/C.kt", "A.kt"]);
    assert_line!(run.out, r"src.C\.kt:1:\d+:.*");
    assert_no_line!(run.out, r"(A|src.B|src.D)\.kt:.*");
}

#[test]
fn lint_format_fixes_only_the_changed_files() {
    let project = repository();
    let run = lint(&project, &["--format", "--changed-since", "base"]);
    assert_eq!(run.exit_code, 0, "{run:?}");
    assert_eq!(project.read("A.kt"), BEFORE);
    assert_eq!(project.read("src/B.kt"), FORMATTED);
    assert_eq!(project.read("src/C.kt"), FORMATTED);
}

#[test]
fn lint_without_changed_files_passes_quietly() {
    let project = repository();
    git(project.dir(), &["add", "."]);
    git(project.dir(), &["commit", "-q", "-m", "all"]);
    let run = lint(&project, &["--changed-since", "HEAD"]);
    assert_eq!((run.exit_code, run.out.as_str(), run.err.as_str()), (0, "", ""), "{run:?}");
}

#[test]
fn lint_changed_since_needs_files_and_a_native_run() {
    assert!(parse_lint_args(&strings(&["--changed-since", "main", "-"])).is_err());
    assert_eq!(parse_lint_args(&strings(&["--changed-since=main"])).unwrap().ktrs_lint.changed_since.as_deref(), Some("main"));
    let project = repository();
    project.write_bytes("custom.jar", b"PK\x03\x04META-INF/services/io.github.ktlint.core.cli.ruleset.core.api.RuleSetV2Provider");
    let run = lint(&project, &["-R", "custom.jar", "--changed-since", "base"]);
    assert_eq!(run.exit_code, 2, "{run:?}");
    assert_eq!(run.err, "--changed-since is not available with 'custom.jar', a ktlint plugin JAR that runs on ktlint's jar");
}

#[cfg(unix)]
#[test]
fn changed_since_finds_symlinked_files_without_pruning_directory() {
    use std::os::unix::fs::symlink;
    let project = Project::new("changed-since-symlink", &[("other/target.kt", BEFORE), ("regular.kt", BEFORE)]);
    git(project.dir(), &["init", "-q"]);
    git(project.dir(), &["add", "."]);
    git(project.dir(), &["commit", "-q", "-m", "base"]);
    git(project.dir(), &["tag", "base"]);

    // Create a symlink in a new subdirectory: link_dir/link.kt -> ../other/target.kt
    std::fs::create_dir_all(project.dir().join("link_dir")).unwrap();
    symlink("../other/target.kt", project.dir().join("link_dir/link.kt")).unwrap();

    // The symlink link_dir/link.kt is untracked/changed in git. link_dir has no other files.
    let run = lint(&project, &["--changed-since", "base"]);
    assert_eq!(run.exit_code, 1);
    assert_line!(run.out, r"link_dir.link\.kt:1:\d+:.*\(standard:no-multi-spaces\)");
    assert_no_line!(run.out, r"(other.target|regular)\.kt:.*");
}

