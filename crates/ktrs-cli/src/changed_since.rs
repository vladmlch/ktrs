//! `--changed-since <ref>` of `ktrs fmt` and `ktrs lint`: the files `git` reports as differing from the merge base
//! of `<ref>` and `HEAD` (committed, staged or not, deletions aside) and the untracked, unignored ones. A run
//! keeps the files of its paths that are among them.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const OPTION: &str = "--changed-since";

const SHALLOW_CLONE_HINT: &str = "in a shallow clone, fetch the history first (actions/checkout: `fetch-depth: 0`)";

/// Canonical paths of changed files, plus ancestors for walk pruning.
#[derive(Clone, Debug, Default)]
pub struct ChangedFiles {
    canonical: HashSet<PathBuf>,
    ancestors: HashSet<PathBuf>,
}

impl ChangedFiles {
    /// `Err` is a usage error message: no `git`, no repository at `working_dir`, or no such commit.
    pub fn since(reference: &str, working_dir: &Path) -> Result<ChangedFiles, String> {
        // Never a ref, and `git` would read it as an option.
        if reference.is_empty() || reference.starts_with('-') {
            return Err(format!("{OPTION} needs a git ref, not '{reference}'"));
        }
        let top_level = git(working_dir, &["rev-parse", "--show-toplevel"])
            .map_err(|e| failure(&format!("'{}' is not in a git repository", working_dir.display()), &e))?;
        let top_level = Path::new(top_level.trim_end_matches(['\r', '\n']));
        let commit = format!("{reference}^{{commit}}");
        git(top_level, &["rev-parse", "--verify", &commit])
            .map_err(|e| failure(&format!("'{reference}' is not a commit of this repository; {SHALLOW_CLONE_HINT}"), &e))?;
        let base = git(top_level, &["merge-base", &commit, "HEAD"])
            .map_err(|e| failure(&format!("'{reference}' and HEAD have no merge base; {SHALLOW_CLONE_HINT}"), &e))?;
        // Against the working tree: one diff covers the commits since the merge base and what is not committed.
        let tracked = git(top_level, &["diff", "--name-only", "-z", "--diff-filter=d", base.trim(), "--"])
            .map_err(|e| failure("git diff failed", &e))?;
        let untracked = git(top_level, &["ls-files", "--others", "--exclude-standard", "-z"])
            .map_err(|e| failure("git ls-files failed", &e))?;
        let files = tracked.split('\0').chain(untracked.split('\0')).filter(|path| !path.is_empty());
        let mut canonical = HashSet::new();
        let mut ancestors = HashSet::new();
        for path in files {
            let full_path = top_level.join(path);
            if let Some(canon) = canonical_entry(&full_path) {
                if let Some(canon_parent) = canon.parent() {
                    for ancestor in canon_parent.ancestors() {
                        ancestors.insert(ancestor.to_path_buf());
                    }
                }
                canonical.insert(canon);
            }
        }
        Ok(ChangedFiles { canonical, ancestors })
    }

    /// Canonicalizes `file`'s parent directory and looks it up: one `fs` call, meant to run once per file.
    pub fn contains(&self, file: &Path) -> bool {
        canonical_entry(file).is_some_and(|file| self.contains_canonical(&file))
    }

    /// `contains` on an already-canonicalized path.
    fn contains_canonical(&self, file: &Path) -> bool {
        self.canonical.contains(file)
    }

    /// Every directory that is an ancestor of a changed file (canonical, like `self.canonical`): a directory
    /// holds no changed file unless it is one of these, so a walk can skip any other subtree.
    pub fn ancestors(&self) -> HashSet<PathBuf> {
        self.ancestors.clone()
    }
}

/// Canonicalizes parent directories preserving the entry itself, so symlinked files can be identified
/// without dereferencing to their target files.
fn canonical_entry(path: &Path) -> Option<PathBuf> {
    let file_name = path.file_name()?;
    let parent = path.parent().unwrap_or(Path::new(""));
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let canon_dir = fs::canonicalize(parent).ok()?;
    Some(canon_dir.join(file_name))
}

/// `git -C <dir> <args>`: its stdout, or its stderr when it fails.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output().map_err(|e| format!("can not run `git`: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn failure(what: &str, git_message: &str) -> String {
    if git_message.is_empty() { format!("{OPTION}: {what}") } else { format!("{OPTION}: {what} ({git_message})") }
}
