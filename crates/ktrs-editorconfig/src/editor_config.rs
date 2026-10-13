//! Port of ec4j `model/Property.java`, `model/Section.java` and `model/EditorConfig.java` (builders
//! folded in: `Section` is built in place, `apply_defaults`/`close_section` are the builder methods).

use crate::glob::Glob;
use crate::property_type::{self, AnyPropertyType, PropertyType, PropertyValue};

/// `Property`: a lowercased name, the registered type (if any) and the value as loaded.
#[derive(Clone, Debug)]
pub struct Property {
    name: String,
    type_: Option<&'static dyn AnyPropertyType>,
    value: PropertyValue<()>,
}

impl PartialEq for Property {
    fn eq(&self, other: &Property) -> bool {
        self.name == other.name && self.value == other.value
    }
}

impl Eq for Property {}

impl Property {
    /// `Property.builder().type(type).name(name).value(value).build()` (`value(String)` parses with the type).
    pub fn new(
        name: &str,
        type_: Option<&'static dyn AnyPropertyType>,
        value: Option<&str>,
    ) -> Property {
        let value = match type_ {
            None => PropertyValue::valid(value, value.map(|_| ())),
            Some(t) => t.parse_erased(value),
        };
        Property {
            name: name.to_owned(),
            type_,
            value,
        }
    }

    /// `Property.builder()...value(PropertyValue)`.
    pub fn with_value(
        name: &str,
        type_: Option<&'static dyn AnyPropertyType>,
        value: PropertyValue<()>,
    ) -> Property {
        Property {
            name: name.to_owned(),
            type_,
            value,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source_value(&self) -> Option<&str> {
        self.value.source()
    }

    pub fn type_(&self) -> Option<&'static dyn AnyPropertyType> {
        self.type_
    }

    /// `getValueAs()` for a property of type `t`: the parsed value, or the `RuntimeException` ec4j throws
    /// when the loaded value is invalid.
    pub fn get_value_as<T: Clone>(&self, t: &PropertyType<T>) -> Option<T> {
        match self.value.error_message() {
            None => {
                if parsed_by(self.type_, t)
                    && let Some(value) = self.value.typed::<T>()
                {
                    return Some(value.clone());
                }
                t.parse(self.value.source()).into_parsed()
            }
            Some(error) => panic!("RuntimeException: {error}"),
        }
    }

    pub fn is_unset(&self) -> bool {
        self.value.is_unset()
    }

    pub fn is_valid(&self) -> bool {
        self.value.is_valid()
    }

    pub fn value(&self) -> &PropertyValue<()> {
        &self.value
    }
}

/// Whether the stored value was parsed by `t` itself: `type_` records the type the registry resolved
/// for the property's name (and `Property::new` parses with it), which need not be the `t` a caller
/// passes — a same-named `PropertyType` of the same `T` could parse differently, and only the same
/// `PropertyType` object is guaranteed to produce the stored value.
fn parsed_by<T>(property_type: Option<&'static dyn AnyPropertyType>, t: &PropertyType<T>) -> bool {
    property_type.is_some_and(|pt| {
        std::ptr::eq(
            pt as *const dyn AnyPropertyType as *const (),
            t as *const PropertyType<T> as *const (),
        )
    })
}

impl std::fmt::Display for Property {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} = {}",
            self.name,
            self.value.source().unwrap_or("null")
        )
    }
}

/// `Section`: a glob (none for the preamble) and its properties in insertion order (a `LinkedHashMap`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    glob: Option<Glob>,
    properties: Vec<Property>,
}

impl Section {
    pub(crate) fn open() -> Section {
        Section {
            glob: None,
            properties: Vec::new(),
        }
    }

    pub fn glob(&self) -> Option<&Glob> {
        self.glob.as_ref()
    }

    pub fn properties(&self) -> &[Property] {
        &self.properties
    }

    pub fn get(&self, name: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.name == name)
    }

    pub fn is_match(&self, file_path: &str) -> bool {
        self.glob
            .as_ref()
            .is_none_or(|glob| glob.is_match(file_path))
    }

    /// `Builder.applyDefaults()` (ec4j applies the spec's `indent_size`/`tab_width` defaults per section).
    pub(crate) fn apply_defaults(&mut self) {
        let source =
            |s: &Section, name: &str| s.get(name).map(|p| p.value.source().map(str::to_owned));
        let indent_style = source(self, property_type::INDENT_STYLE.name);
        let mut indent_size = source(self, property_type::INDENT_SIZE.name);
        let mut tab_width = source(self, property_type::TAB_WIDTH.name);
        // Version.CURRENT (0.12.0-final) >= 0.10.0.
        if indent_style
            .as_ref()
            .is_some_and(|s| s.as_deref() == Some("tab"))
            && indent_size.is_none()
        {
            self.property(Property::new(
                property_type::INDENT_SIZE.name,
                Some(&property_type::INDENT_SIZE),
                Some("tab"),
            ));
            indent_size = Some(Some("tab".to_owned()));
        }
        if let Some(size) = indent_size.as_ref().filter(|s| s.as_deref() != Some("tab"))
            && tab_width.is_none()
        {
            self.property(Property::new(
                property_type::TAB_WIDTH.name,
                Some(&property_type::TAB_WIDTH),
                size.as_deref(),
            ));
            tab_width = Some(size.clone());
        }
        if indent_size
            .as_ref()
            .is_some_and(|s| s.as_deref() == Some("tab"))
            && let Some(width) = tab_width
        {
            self.property(Property::new(
                property_type::INDENT_SIZE.name,
                Some(&property_type::INDENT_SIZE),
                width.as_deref(),
            ));
        }
    }

    /// `Builder.property(property)`: `LinkedHashMap.put`, so a redefinition keeps its first position.
    pub(crate) fn property(&mut self, property: Property) {
        match self.properties.iter_mut().find(|p| p.name == property.name) {
            Some(existing) => *existing = property,
            None => self.properties.push(property),
        }
    }

    pub(crate) fn set_glob(&mut self, glob: Glob) {
        self.glob = Some(glob);
    }

    fn remove(&mut self, name: &str) -> Option<Property> {
        let index = self.properties.iter().position(|p| p.name == name)?;
        Some(self.properties.remove(index))
    }
}

/// `EditorConfig`: the `root` flag (null when absent) and the sections that have a glob.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorConfig {
    root: Option<bool>,
    sections: Vec<Section>,
}

impl EditorConfig {
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    pub fn has_root_property(&self) -> bool {
        self.root.is_some()
    }

    pub fn is_root(&self) -> bool {
        self.root == Some(true)
    }

    /// `Section.Builder.closeSection()`: the preamble only contributes `root`, other sections are kept.
    pub(crate) fn close_section(&mut self, mut section: Section) {
        if section.glob.is_none() {
            if let Some(root_prop) = section.remove(property_type::ROOT.name) {
                self.root = Some(
                    root_prop
                        .value
                        .source()
                        .is_some_and(|s| s.eq_ignore_ascii_case("true")),
                );
            }
        } else {
            self.sections.push(section);
        }
    }
}
