//! Port of ktlint-rule-engine-core `editorconfig/EditorConfigProperty.kt` and `editorconfig/ec4j/EditorConfigProperty.kt`.

use std::borrow::Cow;
use std::fmt::Debug;
use std::ops::Deref;
use std::sync::{Arc, OnceLock};

use ktrs_editorconfig::{AnyPropertyType, Property, PropertyType, PropertyValue};

use crate::editorconfig::code_style::CodeStyleValue;

/// A type an `EditorConfigProperty<T>` can hold; `to_value_string` is Kotlin's `toString()` of it.
pub trait PropertyValueType: Clone + Debug + PartialEq + Send + Sync + 'static {
    fn to_value_string(&self) -> String;
}

impl PropertyValueType for i32 {
    fn to_value_string(&self) -> String {
        self.to_string()
    }
}

impl PropertyValueType for bool {
    fn to_value_string(&self) -> String {
        self.to_string()
    }
}

impl PropertyValueType for String {
    fn to_value_string(&self) -> String {
        self.clone()
    }
}

/// `Set<String>` (insertion ordered); `toString()` is `[a, b]`.
impl PropertyValueType for Vec<String> {
    fn to_value_string(&self) -> String {
        format!("[{}]", self.join(", "))
    }
}

/// Implements [`PropertyValueType`] for [`EnumValue`] enums (`toString()` is the constant's name).
#[macro_export]
macro_rules! enum_property_value_type {
    ($($enum:ty),*) => {
        $(impl $crate::editorconfig::PropertyValueType for $enum {
            fn to_value_string(&self) -> String {
                ktrs_editorconfig::EnumValue::name(*self).to_owned()
            }
        })*
    };
}

enum_property_value_type!(
    ktrs_editorconfig::EndOfLineValue,
    ktrs_editorconfig::IndentStyleValue
);

/// `propertyMapper(property, codeStyle)`.
pub type PropertyMapper<T> = fn(Option<&Property>, CodeStyleValue) -> Option<T>;

/// `EditorConfigProperty<T>`. Build one with [`EditorConfigProperty::new`] and struct-update syntax for
/// the Kotlin named arguments.
#[derive(Clone, Debug)]
pub struct EditorConfigProperty<T: PropertyValueType> {
    pub type_: &'static PropertyType<T>,
    pub default_value: T,
    pub ktlint_official_code_style_default_value: T,
    pub intellij_idea_code_style_default_value: T,
    pub android_studio_code_style_default_value: T,
    /// Maps the loaded property (or its absence) to a value; `None` falls back to the defaults.
    pub property_mapper: Option<PropertyMapper<T>>,
    pub property_writer: fn(&T) -> String,
    pub deprecation_warning: Option<&'static str>,
    pub deprecation_error: Option<&'static str>,
    pub name: Cow<'static, str>,
    /// Memoized `identity()` result (computed on first call; not an upstream field).
    pub identity_cache: OnceLock<String>,
}

impl<T: PropertyValueType> EditorConfigProperty<T> {
    /// `EditorConfigProperty(type, defaultValue)`: every code style defaults to `default_value`.
    pub fn new(type_: &'static PropertyType<T>, default_value: T) -> EditorConfigProperty<T> {
        EditorConfigProperty {
            type_,
            ktlint_official_code_style_default_value: default_value.clone(),
            intellij_idea_code_style_default_value: default_value.clone(),
            android_studio_code_style_default_value: default_value.clone(),
            default_value,
            property_mapper: None,
            property_writer: |value| value.to_value_string(),
            deprecation_warning: None,
            deprecation_error: None,
            name: Cow::Borrowed(type_.name),
            identity_cache: OnceLock::new(),
        }
    }

    /// `EditorConfig.getDefaultValue()` for the active code style.
    pub fn default_value_for(&self, code_style: CodeStyleValue) -> &T {
        match code_style {
            CodeStyleValue::AndroidStudio => &self.android_studio_code_style_default_value,
            CodeStyleValue::IntellijIdea => &self.intellij_idea_code_style_default_value,
            CodeStyleValue::KtlintOfficial => &self.ktlint_official_code_style_default_value,
        }
    }
}

/// An `EditorConfigProperty<*>`: what the engine needs without knowing `T`.
pub trait AnyEditorConfigProperty: Debug + Send + Sync {
    fn name(&self) -> &str;
    fn property_type(&self) -> &'static dyn AnyPropertyType;
    /// `propertyWriter(getDefaultValue())`.
    fn write_default_value(&self, code_style: CodeStyleValue) -> String;
    /// `type.parse(value)`, untyped.
    fn parse(&self, value: Option<&str>) -> PropertyValue<()>;
    /// What `@Poko` equality looks at, minus the lambdas (which are shared singletons upstream).
    fn identity(&self) -> &str;
}

impl<T: PropertyValueType> AnyEditorConfigProperty for EditorConfigProperty<T> {
    fn name(&self) -> &str {
        &self.name
    }

    fn property_type(&self) -> &'static dyn AnyPropertyType {
        self.type_
    }

    fn write_default_value(&self, code_style: CodeStyleValue) -> String {
        (self.property_writer)(self.default_value_for(code_style))
    }

    fn parse(&self, value: Option<&str>) -> PropertyValue<()> {
        self.type_.parse(value).erase()
    }

    fn identity(&self) -> &str {
        self.identity_cache
            .get_or_init(|| {
                format!(
                    "{}:{}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}",
                    self.name,
                    self.type_.name,
                    self.default_value,
                    self.ktlint_official_code_style_default_value,
                    self.intellij_idea_code_style_default_value,
                    self.android_studio_code_style_default_value,
                    self.deprecation_warning,
                    self.deprecation_error
                )
            })
            .as_str()
    }
}

/// A reference to an `EditorConfigProperty<*>`: a static one, or one created at runtime (rule execution
/// properties).
#[derive(Clone, Debug)]
pub enum PropertyRef {
    Static(&'static dyn AnyEditorConfigProperty),
    Shared(Arc<dyn AnyEditorConfigProperty>),
}

impl Deref for PropertyRef {
    type Target = dyn AnyEditorConfigProperty;

    fn deref(&self) -> &Self::Target {
        match self {
            PropertyRef::Static(p) => *p,
            PropertyRef::Shared(p) => p.as_ref(),
        }
    }
}

impl<T: PropertyValueType> From<&'static EditorConfigProperty<T>> for PropertyRef {
    fn from(p: &'static EditorConfigProperty<T>) -> PropertyRef {
        PropertyRef::Static(p)
    }
}

impl<T: PropertyValueType> From<EditorConfigProperty<T>> for PropertyRef {
    fn from(p: EditorConfigProperty<T>) -> PropertyRef {
        PropertyRef::Shared(Arc::new(p))
    }
}

/// `toPropertyWithValue(value: String)`.
pub fn to_property_with_value(property: &dyn AnyEditorConfigProperty, value: &str) -> Property {
    Property::new(property.name(), Some(property.property_type()), Some(value))
}

/// `toPropertyWithValue(value: PropertyValue)`.
pub fn to_property_with_parsed_value(
    property: &dyn AnyEditorConfigProperty,
    value: PropertyValue<()>,
) -> Property {
    Property::with_value(property.name(), Some(property.property_type()), value)
}
