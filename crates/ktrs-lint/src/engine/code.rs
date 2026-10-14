//! Port of ktlint-rule-engine `api/Code.kt`, `api/LintError.kt`, `api/KtLintParseException.kt` and
//! `api/KtLintRuleException.kt`.

use std::path::{Path, PathBuf};

use crate::rule::RuleId;

pub const STDIN_FILE: &str = "<stdin>";

/// A block of code, from a file (whose `.editorconfig` files apply) or a snippet.
#[derive(Clone, Debug)]
pub struct Code {
    pub content: String,
    pub file_name: Option<String>,
    pub file_path: Option<PathBuf>,
    pub script: bool,
    pub is_std_in: bool,
}

impl Code {
    pub fn file_name_or_stdin(&self) -> String {
        match (&self.file_name, self.is_std_in) {
            (Some(name), _) => name.clone(),
            (None, true) => STDIN_FILE.to_owned(),
            (None, false) => String::new(),
        }
    }

    pub fn file_path_or_stdin(&self) -> String {
        match (&self.file_path, self.is_std_in) {
            (Some(path), _) => path.to_string_lossy().into_owned(),
            (None, true) => STDIN_FILE.to_owned(),
            (None, false) => String::new(),
        }
    }

    /// `Code.fromFile(file)`: `readText()` decodes UTF-8, replacing malformed input.
    pub fn from_file(file: &Path) -> std::io::Result<Code> {
        let bytes = std::fs::read(file)?;
        // A valid file moves into the `String` without copying; malformed input is replaced as
        // `from_utf8_lossy` does (on the same bytes `as_bytes` returns).
        let content = match String::from_utf8(bytes) {
            Ok(content) => content,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        };
        Ok(Code::from_file_content(file, content))
    }

    /// [`Code::from_file`] with the content already read.
    pub fn from_file_content(file: &Path, content: String) -> Code {
        let file_name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Code {
            content,
            script: ends_with_kts(&file_name),
            file_name: Some(file_name),
            file_path: Some(file.to_path_buf()),
            is_std_in: false,
        }
    }

    /// `Code.fromSnippet(content, script)`: no `.editorconfig` file is associated with it.
    pub fn from_snippet(content: &str, script: bool) -> Code {
        Code {
            content: content.to_owned(),
            file_name: None,
            file_path: None,
            script,
            is_std_in: true,
        }
    }

    /// `Code.fromSnippetWithPath(content, virtualPath)`: the `.editorconfig` files on `virtual_path` apply;
    /// the file itself is never read.
    pub fn from_snippet_with_path(content: &str, virtual_path: Option<&Path>) -> Code {
        Code {
            content: content.to_owned(),
            file_name: None,
            script: virtual_path.is_some_and(|p| ends_with_kts(&p.to_string_lossy())),
            file_path: virtual_path.map(Path::to_path_buf),
            is_std_in: true,
        }
    }

    /// `Code.fromStdin()`.
    pub fn from_stdin() -> std::io::Result<Code> {
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::stdin(), &mut bytes)?;
        let content = match String::from_utf8(bytes) {
            Ok(content) => content,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        };
        // `Code::from_snippet_with_path(&content, None)`, without copying `content` again.
        Ok(Code {
            content,
            file_name: None,
            file_path: None,
            script: false,
            is_std_in: true,
        })
    }

    /// The file name the PSI file gets: the path, or `File.kt`/`File.kts`.
    pub(crate) fn psi_file_name(&self) -> String {
        match &self.file_path {
            Some(path) => path.to_string_lossy().into_owned(),
            None if self.script => "File.kts".to_owned(),
            None => "File.kt".to_owned(),
        }
    }
}

fn ends_with_kts(name: &str) -> bool {
    name.len() >= 4
        && name.is_char_boundary(name.len() - 4)
        && name[name.len() - 4..].eq_ignore_ascii_case(".kts")
}

/// `LintError` (`@Poko`: equality over all fields).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LintError {
    pub line: usize,
    pub col: usize,
    pub rule_id: RuleId,
    pub detail: String,
    pub can_be_auto_corrected: bool,
}

/// `KtLintParseException`: the code has a `PsiErrorElement`; no rule ran. Its Java message is
/// `"$line:$col $message"`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KtLintParseException {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

/// `KtLintRuleException`: a rule threw (a Rust panic). `cause` is the panic message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KtLintRuleException {
    pub line: usize,
    pub col: usize,
    pub rule_id: String,
    pub message: String,
    pub cause: String,
}

/// What `lint`/`format` throw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KtLintException {
    Parse(KtLintParseException),
    Rule(KtLintRuleException),
    /// An `.editorconfig` file could not be read or has a syntax error.
    EditorConfig(ktrs_editorconfig::ParseException),
    /// ktlint 1.8: an `IllegalStateException` from its `RunAfterRuleFilter` (a required rule is disabled).
    IllegalState(String),
    /// The parser's `AssertionError` (an `Error`: no `catch (e: Exception)` of the CLI stops it).
    MissedTokens(ktrs_syntax::MissedTokens),
}

impl From<KtLintParseException> for KtLintException {
    fn from(e: KtLintParseException) -> KtLintException {
        KtLintException::Parse(e)
    }
}

impl From<KtLintRuleException> for KtLintException {
    fn from(e: KtLintRuleException) -> KtLintException {
        KtLintException::Rule(e)
    }
}

impl From<ktrs_editorconfig::ParseException> for KtLintException {
    fn from(e: ktrs_editorconfig::ParseException) -> KtLintException {
        KtLintException::EditorConfig(e)
    }
}

impl std::fmt::Display for KtLintException {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KtLintException::Parse(e) => write!(f, "{}:{} {}", e.line, e.col, e.message),
            KtLintException::Rule(e) => f.write_str(&e.message),
            KtLintException::EditorConfig(e) => write!(f, "{e}"),
            KtLintException::IllegalState(message) => f.write_str(message),
            KtLintException::MissedTokens(e) => f.write_str(&e.message()),
        }
    }
}
