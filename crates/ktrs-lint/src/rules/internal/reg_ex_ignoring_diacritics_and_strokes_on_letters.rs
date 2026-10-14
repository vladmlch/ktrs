//! Port of `RegExIgnoringDiacriticsAndStrokesOnLetters.kt`, and a Kotlin `Regex` (Java `Pattern`) over the
//! `regex` crate.

use regex::Regex;

/// `String.regExIgnoringDiacriticsAndStrokesOnLetters()`: `A-Z` -> `\p{Lu}`, `a-z` -> `\p{Ll}`.
pub fn reg_ex_ignoring_diacritics_and_strokes_on_letters(pattern: &str) -> KotlinRegex {
    KotlinRegex::with_ascii_fallback(&pattern.replace("A-Z", r"\p{Lu}").replace("a-z", r"\p{Ll}"), pattern)
}

/// A Kotlin `Regex`, of which the rules only call `matches` (a full match).
#[derive(Debug)]
pub struct KotlinRegex {
    unicode: Regex,
    ascii: Option<Regex>,
}

impl KotlinRegex {
    pub fn new(pattern: &str) -> KotlinRegex {
        KotlinRegex {
            unicode: Regex::new(&format!("^(?:{})$", to_rust_syntax(pattern))).expect("PatternSyntaxException"),
            ascii: None,
        }
    }

    pub fn with_ascii_fallback(unicode_pattern: &str, ascii_pattern: &str) -> KotlinRegex {
        let unicode = Regex::new(&format!("^(?:{})$", to_rust_syntax(unicode_pattern))).expect("PatternSyntaxException");
        let ascii = Regex::new(&format!("^(?:{})$", to_rust_syntax(ascii_pattern))).ok();
        KotlinRegex { unicode, ascii }
    }

    /// `Regex.matches(input)` / `String.matches(regex)`.
    pub fn matches(&self, input: &str) -> bool {
        if input.is_ascii() {
            self.ascii
                .as_ref()
                .map_or_else(|| self.unicode.is_match(input), |re| re.is_match(input))
        } else {
            self.unicode.is_match(input)
        }
    }
}

/// Java `Pattern` syntax -> `regex` syntax for the constructs where the two differ in meaning: `\d` is ASCII
/// in Java (Unicode `Nd` in Rust) and `.` excludes every Java line terminator (only `\n` in Rust). Nested
/// classes are not supported; the rules have none.
fn to_rust_syntax(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut in_class = false;
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('d') => out.push_str(if in_class { "0-9" } else { "[0-9]" }),
                Some(escaped) => {
                    out.push('\\');
                    out.push(escaped);
                }
                None => out.push('\\'),
            },
            '[' if !in_class => {
                in_class = true;
                out.push(c);
            }
            ']' if in_class => {
                in_class = false;
                out.push(c);
            }
            '.' if !in_class => out.push_str("[^\n\r\u{85}\u{2028}\u{2029}]"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_semantics() {
        let class_name = reg_ex_ignoring_diacritics_and_strokes_on_letters("[A-Z][A-Za-z\\d]*");
        assert!(class_name.matches("ŸëšThïs123"));
        assert!(class_name.matches("FooBar123"));
        assert!(!class_name.matches("Foo١")); // Arabic-Indic digit: not `\d` in Java
        assert!(!class_name.matches("fooBar"));
        let backticked = KotlinRegex::new("`.*`");
        assert!(backticked.matches("`a b`"));
        assert!(!backticked.matches("`a\rb`"));
        assert!(!backticked.matches("x`a`"));
    }
}
