//! Port of ec4j `model/PropertyType.java`: property types, parsed values and the value parsers.

use std::any::Any;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

pub const UNSET: &str = "unset";

/// `PropertyType.PropertyValue`: the source text, its parsed value (a valid value may parse to null, e.g.
/// `indent_size = tab`) and the error of an invalid one.
#[derive(Clone)]
pub struct PropertyValue<T> {
    error_message: Option<String>,
    parsed: Option<T>,
    source: Option<String>,
    /// `parsed` kept through [`PropertyValue::erase`]; a pure cache [`PropertyValue::typed`] reads,
    /// so `PartialEq`/`Eq`/`Hash`/`Debug` ignore it exactly as the derives did.
    parsed_any: Option<Arc<dyn Any + Send + Sync>>,
}

/// What the derived `Debug` printed before `parsed_any` (which has no `Debug`) was added.
impl<T: Debug> Debug for PropertyValue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PropertyValue")
            .field("error_message", &self.error_message)
            .field("parsed", &self.parsed)
            .field("source", &self.source)
            .finish()
    }
}

/// What the derived `PartialEq` compared.
impl<T: PartialEq> PartialEq for PropertyValue<T> {
    fn eq(&self, other: &PropertyValue<T>) -> bool {
        self.error_message == other.error_message
            && self.parsed == other.parsed
            && self.source == other.source
    }
}

impl<T: Eq> Eq for PropertyValue<T> {}

/// What the derived `Hash` hashed.
impl<T: Hash> Hash for PropertyValue<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.error_message.hash(state);
        self.parsed.hash(state);
        self.source.hash(state);
    }
}

impl<T> PropertyValue<T> {
    pub fn unset() -> PropertyValue<T> {
        PropertyValue {
            error_message: None,
            parsed: None,
            source: Some(UNSET.to_owned()),
            parsed_any: None,
        }
    }

    pub fn invalid(source: Option<&str>, error_message: String) -> PropertyValue<T> {
        if source.is_some_and(|s| s.eq_ignore_ascii_case(UNSET)) {
            return PropertyValue::unset();
        }
        PropertyValue {
            error_message: Some(error_message),
            parsed: None,
            source: source.map(str::to_owned),
            parsed_any: None,
        }
    }

    pub fn valid(source: Option<&str>, value: Option<T>) -> PropertyValue<T> {
        if source.is_some_and(|s| s.eq_ignore_ascii_case(UNSET)) {
            return PropertyValue::unset();
        }
        PropertyValue {
            error_message: None,
            parsed: value,
            source: source.map(str::to_owned),
            parsed_any: None,
        }
    }

    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }

    pub fn parsed(&self) -> Option<&T> {
        self.parsed.as_ref()
    }

    pub fn into_parsed(self) -> Option<T> {
        self.parsed
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn is_unset(&self) -> bool {
        self.source.as_deref() == Some(UNSET)
    }

    pub fn is_valid(&self) -> bool {
        self.error_message.is_none()
    }
}

impl<T: Send + Sync + 'static> PropertyValue<T> {
    /// The untyped form a [`crate::Property`] stores; `parsed` only records whether a value was parsed
    /// while `parsed_any` keeps it for [`PropertyValue::typed`].
    pub fn erase(self) -> PropertyValue<()> {
        let parsed_any = self
            .parsed
            .map(|v| Arc::new(v) as Arc<dyn Any + Send + Sync>);
        PropertyValue {
            error_message: self.error_message,
            parsed: parsed_any.as_ref().map(|_| ()),
            source: self.source,
            parsed_any,
        }
    }
}

impl PropertyValue<()> {
    /// The value [`PropertyValue::erase`] kept, downcast to `U`: `None` when there is none or when it
    /// was parsed by a `PropertyType` of another `T`.
    pub fn typed<U: 'static>(&self) -> Option<&U> {
        self.parsed_any.as_ref()?.downcast_ref::<U>()
    }
}

/// `PropertyValueParser.parse(name, value)`.
pub type PropertyValueParser<T> = fn(&str, Option<&str>) -> PropertyValue<T>;

/// `PropertyType<T>`; `lower_casing` marks a `LowerCasingPropertyType` (only its `normalizeIfNeeded`
/// differs, which ec4j calls from its CLI alone, so values are never lowercased on load).
#[derive(Debug)]
pub struct PropertyType<T: 'static> {
    pub name: &'static str,
    pub description: &'static str,
    pub parser: PropertyValueParser<T>,
    pub possible_values: &'static [&'static str],
    pub lower_casing: bool,
}

impl<T> PropertyType<T> {
    pub fn parse(&self, value: Option<&str>) -> PropertyValue<T> {
        (self.parser)(self.name, value)
    }
}

/// A `PropertyType<?>`: what the loader needs of a registered type.
pub trait AnyPropertyType: Debug + Send + Sync {
    fn name(&self) -> &'static str;
    fn parse_erased(&self, value: Option<&str>) -> PropertyValue<()>;
}

impl<T: Debug + Send + Sync> AnyPropertyType for PropertyType<T> {
    fn name(&self) -> &'static str {
        self.name
    }

    fn parse_erased(&self, value: Option<&str>) -> PropertyValue<()> {
        self.parse(value).erase()
    }
}

/// A Java enum usable with [`enum_value_parser`]: `Enum.valueOf(type, name)`.
pub trait EnumValue: Copy + Debug + Send + Sync + 'static {
    /// `enumType.getName()`, for error messages.
    const ENUM_TYPE_NAME: &'static str;
    const ENTRIES: &'static [Self];
    fn name(self) -> &'static str;

    fn value_of(name: &str) -> Option<Self> {
        Self::ENTRIES.iter().copied().find(|e| e.name() == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EndOfLineValue {
    Cr,
    Crlf,
    Lf,
}

impl EnumValue for EndOfLineValue {
    const ENUM_TYPE_NAME: &'static str = "org.ec4j.core.model.PropertyType$EndOfLineValue";
    const ENTRIES: &'static [Self] =
        &[EndOfLineValue::Cr, EndOfLineValue::Crlf, EndOfLineValue::Lf];

    fn name(self) -> &'static str {
        match self {
            EndOfLineValue::Cr => "cr",
            EndOfLineValue::Crlf => "crlf",
            EndOfLineValue::Lf => "lf",
        }
    }
}

impl EndOfLineValue {
    pub fn end_of_line_string(self) -> &'static str {
        match self {
            EndOfLineValue::Cr => "\r",
            EndOfLineValue::Crlf => "\r\n",
            EndOfLineValue::Lf => "\n",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IndentStyleValue {
    Space,
    Tab,
}

impl EnumValue for IndentStyleValue {
    const ENUM_TYPE_NAME: &'static str = "org.ec4j.core.model.PropertyType$IndentStyleValue";
    const ENTRIES: &'static [Self] = &[IndentStyleValue::Space, IndentStyleValue::Tab];

    fn name(self) -> &'static str {
        match self {
            IndentStyleValue::Space => "space",
            IndentStyleValue::Tab => "tab",
        }
    }
}

impl IndentStyleValue {
    pub fn indent_char(self) -> char {
        match self {
            IndentStyleValue::Space => ' ',
            IndentStyleValue::Tab => '\t',
        }
    }
}

const BOOLEAN_POSSIBLE_VALUES: &[&str] = &["true", "false"];

macro_rules! property_type {
    ($name:literal, $description:literal, $parser:expr, $values:expr, $lower:literal) => {
        PropertyType {
            name: $name,
            description: $description,
            parser: $parser,
            possible_values: $values,
            lower_casing: $lower,
        }
    };
}

pub static CHARSET: PropertyType<String> = property_type!(
    "charset",
    "set to latin1, utf-8, utf-8-bom, utf-16be or utf-16le to control the character set. Use of utf-8-bom is discouraged.",
    identity_value_parser,
    &[
        "utf-8",
        "utf-8-bom",
        "utf-16be",
        "utf-16le",
        "latin1",
        "tab"
    ],
    true
);
pub static END_OF_LINE: PropertyType<EndOfLineValue> = property_type!(
    "end_of_line",
    "set to lf, cr, or crlf to control how line breaks are represented.",
    enum_value_parser::<EndOfLineValue>,
    &["cr", "crlf", "lf"],
    true
);
pub static INDENT_SIZE: PropertyType<i32> = property_type!(
    "indent_size",
    "a whole number defining the number of columns used for each indentation level and the width of soft tabs (when supported). When set to tab, the parsed of tab_width (if specified) will be used.",
    indent_size_value_parser,
    &["1", "2", "3", "4", "5", "6", "7", "8", "tab"],
    true
);
pub static INDENT_STYLE: PropertyType<IndentStyleValue> = property_type!(
    "indent_style",
    "set to tab or space to use hard tabs or soft tabs respectively.",
    enum_value_parser::<IndentStyleValue>,
    &["space", "tab"],
    true
);
pub static INSERT_FINAL_NEWLINE: PropertyType<bool> = property_type!(
    "insert_final_newline",
    "set to true to ensure file ends with a newline when saving and false to ensure it doesn't.",
    boolean_value_parser,
    BOOLEAN_POSSIBLE_VALUES,
    true
);
pub static MAX_LINE_LENGTH: PropertyType<i32> = property_type!(
    "max_line_length",
    "forces hard line wrapping after the amount of characters specified. Use the `off` value to turn this feature off (use the editor settings).",
    max_line_length_value_parser,
    &["1", "2", "3", "4", "5", "6", "7", "8", "off"],
    true
);
pub static ROOT: PropertyType<bool> = property_type!(
    "root",
    "special property that should be specified at the top of the file outside of any sections. Set to true to stop .editorconfig files search on current file.",
    boolean_value_parser,
    BOOLEAN_POSSIBLE_VALUES,
    true
);
pub static TAB_WIDTH: PropertyType<i32> = property_type!(
    "tab_width",
    "a whole number defining the number of columns used to represent a tab character. This defaults to the parsed of indent_size and doesn't usually need to be specified.",
    positive_int_value_parser,
    &["1", "2", "3", "4", "5", "6", "7", "8"],
    false
);
pub static TRIM_TRAILING_WHITESPACE: PropertyType<bool> = property_type!(
    "trim_trailing_whitespace",
    "set to true to remove any whitespace characters preceding newline characters and false to ensure it doesn't.",
    boolean_value_parser,
    BOOLEAN_POSSIBLE_VALUES,
    true
);

/// `PropertyType.standardTypes()` (note: without `max_line_length`).
pub fn standard_types() -> [&'static dyn AnyPropertyType; 8] {
    [
        &CHARSET,
        &END_OF_LINE,
        &INDENT_SIZE,
        &INDENT_STYLE,
        &INSERT_FINAL_NEWLINE,
        &ROOT,
        &TAB_WIDTH,
        &TRIM_TRAILING_WHITESPACE,
    ]
}

pub fn boolean_value_parser(name: &str, value: Option<&str>) -> PropertyValue<bool> {
    match value {
        None => PropertyValue::invalid(
            None,
            format!("Property '{name}' expects a boolean; found: null"),
        ),
        Some(v) if v.eq_ignore_ascii_case("true") => PropertyValue::valid(value, Some(true)),
        Some(v) if v.eq_ignore_ascii_case("false") => PropertyValue::valid(value, Some(false)),
        Some(v) if v.eq_ignore_ascii_case(UNSET) => PropertyValue::unset(),
        Some(v) => PropertyValue::invalid(
            value,
            format!("Property '{name}' expects a boolean. The parsed '{v}' is not a boolean."),
        ),
    }
}

pub fn identity_value_parser(_name: &str, value: Option<&str>) -> PropertyValue<String> {
    PropertyValue::valid(value, value.map(str::to_owned))
}

pub fn indent_size_value_parser(name: &str, value: Option<&str>) -> PropertyValue<i32> {
    if value.is_some_and(|v| v.eq_ignore_ascii_case("tab")) {
        PropertyValue::valid(value, None)
    } else {
        positive_int_value_parser(name, value)
    }
}

pub fn max_line_length_value_parser(name: &str, value: Option<&str>) -> PropertyValue<i32> {
    if value.is_some_and(|v| v.eq_ignore_ascii_case("off")) {
        PropertyValue::valid(value, None)
    } else {
        positive_int_value_parser(name, value)
    }
}

pub fn positive_int_value_parser(name: &str, value: Option<&str>) -> PropertyValue<i32> {
    match value.and_then(parse_int) {
        Some(val) if val <= 0 => PropertyValue::invalid(
            value,
            format!(
                "Property '{name}' expects a positive integer; found '{}'",
                value.unwrap()
            ),
        ),
        Some(val) => PropertyValue::valid(value, Some(val)),
        None => PropertyValue::invalid(
            value,
            format!(
                "Property '{name}' expects an integer. The parsed '{}' is not an integer.",
                value.unwrap_or("null")
            ),
        ),
    }
}

/// `Integer.parseInt`: an optional sign, then ASCII digits (Rust's `i32::from_str` agrees).
pub fn parse_int(value: &str) -> Option<i32> {
    value.parse::<i32>().ok()
}

/// `EnumValueParser`: the value lowercased must name an enum constant.
pub fn enum_value_parser<E: EnumValue>(_name: &str, value: Option<&str>) -> PropertyValue<E> {
    match value {
        None => PropertyValue::invalid(
            None,
            format!("Cannot make enum {} out of null", E::ENUM_TYPE_NAME),
        ),
        Some(v) => match E::value_of(&v.to_lowercase()) {
            Some(e) => PropertyValue::valid(value, Some(e)),
            None => PropertyValue::invalid(
                value,
                format!("Unexpected parsed \"{v}\" for enum {}", E::ENUM_TYPE_NAME),
            ),
        },
    }
}
