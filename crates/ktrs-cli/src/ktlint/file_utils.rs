//! Port of ktlint-cli `internal/FileUtils.kt`: patterns (globs with gitignore-style `!` negation, files,
//! directories) to the files to lint, walked in directory order from the patterns' common parent.

use std::cell::Cell;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Instant;

use crate::ktlint::java_glob::PathMatcher;
use crate::ktlint::jpath::{JPath, ON_WINDOWS};
use crate::ktlint::logger::{FILE_UTILS, Logger};

const NEGATION_PREFIX: &str = "!";
const DEFAULT_KOTLIN_FILE_EXTENSIONS: [&str; 2] = ["kt", "kts"];
pub const DEFAULT_PATTERNS: [&str; 2] = ["**/*.kt", "**/*.kts"];

/// `FileSystem.fileSequence(patterns, rootDir)`: each pattern is a glob, or a file or directory path
/// relative to `root_dir`, or absolute. `Err` is a `PatternSyntaxException` message (upstream crashes).
pub fn file_sequence(
    patterns: &[String],
    root_dir: &JPath,
    user_home: &str,
    logger: &Logger,
) -> Result<Vec<JPath>, String> {
    file_sequence_streaming(patterns, root_dir, user_home, logger).map(FileSequence::collect)
}

/// `file_sequence` split at the `walkFileTree`: the expansion runs eagerly, the walk lazily on `feed`, so a
/// consumer can process files while the tree is still walked (upstream materializes `result` first).
pub fn file_sequence_streaming(
    patterns: &[String],
    root_dir: &JPath,
    user_home: &str,
    logger: &Logger,
) -> Result<FileSequence, String> {
    if patterns.is_empty() {
        logger.trace(FILE_UTILS, || {
            "No patterns provided. Will not expand any globs.".to_owned()
        });
        return Ok(FileSequence {
            existing: Vec::new(),
            walk: None,
        });
    }
    let mut existing: Vec<JPath> = Vec::new();
    let (existing_files, patterns_exclusive_existing_files): (Vec<&String>, Vec<&String>) =
        patterns.iter().partition(|p| {
            root_dir
                .resolve(p)
                .is_some_and(|path| path.to_path_buf().is_file())
        });
    existing.extend(existing_files.iter().filter_map(|p| root_dir.resolve(p)));
    if !existing.is_empty() && patterns_exclusive_existing_files.is_empty() {
        return Ok(FileSequence {
            existing,
            walk: None,
        });
    }

    let globs = expand(
        &patterns_exclusive_existing_files,
        root_dir,
        user_home,
        logger,
    );
    let negated_path_matchers = globs
        .iter()
        .filter_map(|g| g.strip_prefix(NEGATION_PREFIX))
        .map(|g| get_path_matcher(g))
        .collect::<Result<Vec<_>, _>>()?;
    let mut include_globs: Vec<String> = globs
        .iter()
        .filter(|g| !g.starts_with(NEGATION_PREFIX))
        .cloned()
        .collect();
    if !negated_path_matchers.is_empty() && include_globs.is_empty() {
        logger.info(FILE_UTILS, || {
            format!(
                "A negate pattern is specified without an include pattern. As default, the include patterns '{}' are used.",
                java_list(&DEFAULT_PATTERNS)
            )
        });
        let defaults: Vec<String> = DEFAULT_PATTERNS.iter().map(|p| p.to_string()).collect();
        include_globs.extend(expand(
            &defaults.iter().collect::<Vec<_>>(),
            root_dir,
            user_home,
            logger,
        ));
    }
    let mut common_root_dir = root_dir.clone();
    for pattern in patterns {
        if let Some(pattern_dir) = root_dir.resolve(pattern) {
            common_root_dir = find_common_parent_dir(&common_root_dir, &pattern_dir.normalize());
        }
    }
    let path_matchers = include_globs
        .iter()
        .map(|g| get_path_matcher(g))
        .collect::<Result<Vec<_>, _>>()?;

    logger.debug(FILE_UTILS, || {
        format!("Start walkFileTree from directory: '{common_root_dir}'")
    });
    Ok(FileSequence {
        existing,
        walk: Some(Walk {
            common_root_dir,
            negated_path_matchers,
            path_matchers,
            logger: logger.clone(),
        }),
    })
}

/// `file_sequence`'s two parts: `existing` files resolved from the patterns themselves (yielded first, in
/// pattern order), then `walk`'s files under the patterns' common root directory (visit order).
pub struct FileSequence {
    pub existing: Vec<JPath>,
    /// `None` when every pattern resolved to a file: the sequence is `existing` alone.
    walk: Option<Walk>,
}

/// The matchers the `walkFileTree` applies to every visited path.
struct Walk {
    common_root_dir: JPath,
    negated_path_matchers: Vec<PathMatcher>,
    path_matchers: Vec<PathMatcher>,
    logger: Logger,
}

/// A yielded file and its path (`to_path_buf`, computed once — `ChangedFiles::contains` and
/// `Code::from_file` share it).
pub struct Discovered {
    pub file: JPath,
    pub path: PathBuf,
}

impl FileSequence {
    /// Whether the sequence walks the file tree (a glob or directory pattern was among the patterns).
    pub fn has_walk(&self) -> bool {
        self.walk.is_some()
    }

    /// The sequence as a `Vec`, the walk run on this thread.
    pub fn collect(self) -> Vec<JPath> {
        let mut result = Vec::new();
        self.feed(&|| false, &mut |file| {
            result.push(file);
            true
        }, None);
        result
    }

    /// The sequence onto `tx`, the walk run on this thread: `include` can drop a file (the
    /// `--changed-since` intersection), `stop` (the consumer's `takeWhile` condition) or a dead receiver
    /// ends the feed early. `prune_dirs` (the ancestors of the files `include` keeps) skips the subtrees
    /// that can hold none.
    pub fn stream_into(
        self,
        tx: Sender<Discovered>,
        stop: &dyn Fn() -> bool,
        include: &dyn Fn(&JPath, &Path) -> bool,
        prune_dirs: Option<&HashSet<PathBuf>>,
    ) {
        self.feed(
            stop,
            &mut |file| {
                let path = file.to_path_buf();
                !include(&file, &path) || tx.send(Discovered { file, path }).is_ok()
            },
            prune_dirs,
        );
    }

    /// `yield_file`s the sequence until it is exhausted, `stop` holds or a yield returns `false` (`false`
    /// is returned then; `Discovered N files` logs only when the walk ran to its end).
    fn feed(
        self,
        stop: &dyn Fn() -> bool,
        yield_file: &mut dyn FnMut(JPath) -> bool,
        prune_dirs: Option<&HashSet<PathBuf>>,
    ) -> bool {
        let mut discovered = self.existing.len();
        for file in self.existing {
            if stop() || !yield_file(file) {
                return false;
            }
        }
        let Some(walk) = self.walk else { return true };
        let aborted = Cell::new(false);
        let start = Instant::now();
        let completed = walk_file_tree(
            &walk.common_root_dir,
            &walk.common_root_dir,
            &mut |jpath, path_str| {
                if !walk.negated_path_matchers.iter().any(|m| m.matches(path_str))
                    && walk.path_matchers.iter().any(|m| m.matches(path_str))
                {
                    walk.logger.trace(FILE_UTILS, || {
                        format!("- File: {path_str}: Include as it matches patterns")
                    });
                    discovered += 1;
                    if !yield_file(jpath.clone()) {
                        aborted.set(true);
                    }
                } else {
                    walk.logger
                        .trace(FILE_UTILS, || format!("- File: {path_str}: Ignore"));
                }
            },
            &|| aborted.get() || stop(),
            prune_dirs,
        );
        if completed {
            let duration = start.elapsed().as_millis();
            walk.logger.debug(FILE_UTILS, || {
                format!("Discovered {discovered} files to be processed in {duration} ms")
            });
        }
        completed
    }
}

/// `Files.walkFileTree` with ktlint's visitor: files (and links, which are not followed) are visited,
/// hidden directories other than the start are skipped. Listing order is the file system's, like the JVM's.
/// `stop` is checked between entries; `false` is returned when it ended the walk early. `prune` (the
/// ancestors of the wanted files) skips a directory's whole subtree when it can hold none.
fn walk_file_tree(
    dir: &JPath,
    start: &JPath,
    visit_file: &mut dyn FnMut(&JPath, &str),
    stop: &dyn Fn() -> bool,
    prune: Option<&HashSet<PathBuf>>,
) -> bool {
    let path = dir.to_path_buf();
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return true;
    };
    if !metadata.is_dir() {
        let path_str = dir.to_string();
        visit_file(dir, &path_str);
        return true;
    }
    if dir != start && is_hidden(dir, &metadata) {
        return true;
    }
    let Ok(entries) = fs::read_dir(&path) else {
        return true;
    };
    for entry in entries.flatten() {
        if stop() {
            return false;
        }
        if let Some(child) = dir.resolve(&entry.file_name().to_string_lossy()) {
            match entry.file_type() {
                Ok(file_type) if file_type.is_file() => {
                    let path_str = child.to_string();
                    visit_file(&child, &path_str);
                }
                Ok(file_type) if file_type.is_dir() => {
                    // `file_type` comes with the entry. The wanted set is canonical and the walked path may
                    // not be (a symlinked ancestor), so a directory is resolved before it can be pruned.
                    if prune.is_some_and(|dirs| {
                        fs::canonicalize(&child.to_path_buf()).is_ok_and(|dir| !dirs.contains(&dir))
                    }) {
                        continue;
                    }
                    if !walk_file_tree(&child, start, visit_file, stop, prune) {
                        return false;
                    }
                }
                _ => {
                    if !walk_file_tree(&child, start, visit_file, stop, prune) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

#[cfg(windows)]
fn is_hidden(_: &JPath, metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x2 != 0
}

#[cfg(not(windows))]
fn is_hidden(dir: &JPath, _: &fs::Metadata) -> bool {
    dir.file_name().is_some_and(|n| n.starts_with('.'))
}

fn get_path_matcher(glob: &str) -> Result<PathMatcher, String> {
    let glob = glob.strip_prefix("glob:").unwrap_or(glob);
    PathMatcher::new(glob, ON_WINDOWS)
}

fn find_common_parent_dir(this: &JPath, path: &JPath) -> JPath {
    if path.starts_with(this) {
        this.clone()
    } else if this.starts_with(path) {
        path.clone()
    } else {
        match path.parent() {
            Some(parent) => find_common_parent_dir(this, &parent),
            None => this.clone(),
        }
    }
}

fn expand(patterns: &[&String], root_dir: &JPath, user_home: &str, logger: &Logger) -> Vec<String> {
    patterns
        .iter()
        .filter_map(|p| {
            if ON_WINDOWS {
                normalize_windows_pattern(p, logger)
            } else {
                Some(p.to_string())
            }
        })
        .map(|p| expand_tilde_to_full_path(&p, user_home))
        .map(|p| if ON_WINDOWS { p.replace('\\', "/") } else { p })
        .flat_map(|p| to_glob(&p, root_dir))
        .collect()
}

fn to_glob(path: &str, root_dir: &JPath) -> Vec<String> {
    let negation = if path.starts_with(NEGATION_PREFIX) {
        NEGATION_PREFIX
    } else {
        ""
    };
    let path_without_negation_prefix = path.strip_prefix(NEGATION_PREFIX).unwrap_or(path);
    let expanded_patterns = match root_dir.resolve(path_without_negation_prefix) {
        Some(resolved) => {
            let resolved_path = resolved.normalize();
            if resolved_path.to_path_buf().is_dir() {
                expand_path_to_default_patterns(&resolved_path)
            } else {
                expand_double_star_patterns(&resolved_path.to_string())
            }
        }
        None if ON_WINDOWS => expand_double_star_patterns(path_without_negation_prefix),
        None => Vec::new(),
    };
    expanded_patterns
        .into_iter()
        .map(|original| {
            if ON_WINDOWS {
                let p = original.replace('\\', "/");
                let p = p.split_once(':').map_or(p.as_str(), |(_, after)| after);
                let p = p.strip_prefix('/').unwrap_or(p);
                if p.starts_with("**/") {
                    p.to_owned()
                } else {
                    format!("**/{p}")
                }
            } else {
                original
            }
        })
        .map(|p| format!("{negation}glob:{p}"))
        .collect()
}

/// For each `**` part (except a trailing one), also the path without it, recursively; insertion-ordered set.
fn expand_double_star_patterns(path: &str) -> Vec<String> {
    let mut paths = vec![path.to_owned()];
    let parts: Vec<&str> = path.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if *part != "**" || i == parts.len() - 1 {
            continue;
        }
        let expanded: Vec<&str> = parts
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, p)| *p)
            .collect();
        for p in expand_double_star_patterns(&expanded.join("/")) {
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
    }
    paths
}

fn normalize_windows_pattern(pattern: &str, logger: &Logger) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    let replaced = pattern.replace('\\', "/");
    for part in replaced.split('/').filter(|p| *p != ".") {
        if part == ".." {
            match parts.last() {
                None => {
                    logger.warn(FILE_UTILS, || {
                        format!("On WindowsOS the pattern '{pattern}' can not be used as it refers to a path outside of the current directory")
                    });
                    return None;
                }
                Some(last) if last.contains('*') => {
                    logger.warn(FILE_UTILS, || {
                        format!("On WindowsOS the pattern '{pattern}' can not be used as '/..' follows the wildcard pattern {last}")
                    });
                    return None;
                }
                Some(_) => {
                    parts.pop();
                }
            }
        } else {
            parts.push(part);
        }
    }
    Some(parts.join("/"))
}

fn expand_path_to_default_patterns(path: &JPath) -> Vec<String> {
    DEFAULT_KOTLIN_FILE_EXTENSIONS
        .iter()
        .flat_map(|ext| [format!("{path}/*.{ext}"), format!("{path}/**/*.{ext}")])
        .collect()
}

/// Replaces a leading `~` (after an optional `!`) with the user's home; not on Windows, where `~` occurs in
/// short (8.3) path names.
pub fn expand_tilde_to_full_path(path: &str, user_home: &str) -> String {
    if ON_WINDOWS {
        return path.to_owned();
    }
    match path.strip_prefix("!~") {
        Some(rest) => format!("{user_home}{rest}"),
        None => match path.strip_prefix('~') {
            Some(rest) => format!("{user_home}{rest}"),
            None => path.to_owned(),
        },
    }
}

/// `File.location(relative)`: relative to the working directory, or as is; `/`-separated.
pub fn location(path: &JPath, relative: bool, root_dir_path: &JPath) -> String {
    if relative {
        path.relative_to_or_self(root_dir_path).to_string()
    } else {
        path.to_string()
    }
}

/// Kotlin's `List.toString()`: `[a, b]`.
pub fn java_list<S: AsRef<str>>(items: &[S]) -> String {
    let items: Vec<&str> = items.iter().map(AsRef::as_ref).collect();
    format!("[{}]", items.join(", "))
}
