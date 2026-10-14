//! Port of ktlint-rule-engine-core `editorconfig/EditorConfig.kt`: the loaded properties and typed access.

use std::collections::HashMap;

use ktrs_editorconfig::{Property, PropertyType, PropertyValue};

use crate::editorconfig::code_style::{CODE_STYLE_PROPERTY, CodeStyleValue};
use crate::editorconfig::editor_config_property::{
    EditorConfigProperty, PropertyRef, PropertyValueType, to_property_with_value,
};

/// `EditorConfig(properties: Map<String, Property>)`; the map keeps insertion order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorConfig {
    properties: Vec<Property>,
}

impl EditorConfig {
    pub fn new(properties: Vec<Property>) -> EditorConfig {
        let mut editor_config = EditorConfig::default();
        properties.into_iter().for_each(|p| editor_config.put(p));
        editor_config
    }

    fn put(&mut self, property: Property) {
        match self
            .properties
            .iter_mut()
            .find(|p| p.name() == property.name())
        {
            Some(existing) => *existing = property,
            None => self.properties.push(property),
        }
    }

    fn property(&self, name: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.name() == name)
    }

    /// Parsed without the warning-free default lookup, so an undefined code style logs nothing.
    fn code_style(&self) -> CodeStyleValue {
        self.get_property_value(CODE_STYLE_PROPERTY.type_, CODE_STYLE_PROPERTY.type_.name)
            .unwrap_or(CODE_STYLE_PROPERTY.default_value)
    }

    /// `get(editorConfigProperty)`: the value, remapped by the property's mapper, or the default of the
    /// active code style. Panics (a Kotlin exception) when the rule did not declare the property.
    pub fn get<T: PropertyValueType>(&self, editor_config_property: &EditorConfigProperty<T>) -> T {
        if let Some(deprecation_error) = editor_config_property.deprecation_error {
            panic!(
                "DeprecatedEditorConfigPropertyException: Property '{}' is disallowed: {deprecation_error}",
                editor_config_property.name
            );
        }
        let property = self.property(&editor_config_property.name).unwrap_or_else(|| {
            panic!(
                "IllegalStateException: Property '{}' can not be retrieved from this EditorConfig. Note that the EditorConfig which \
                 is provided to class 'io.github.ktlint.core.rule.engine.core.api.RuleV2' only contains the properties which are \
                 defined in the property 'usesEditorConfigProperties'.",
                editor_config_property.name
            )
        });
        if let Some(new_value) = editor_config_property
            .property_mapper
            .and_then(|mapper| mapper(Some(property), self.code_style()))
        {
            return new_value;
        }
        if property.is_unset() {
            editor_config_property
                .default_value_for(self.code_style())
                .clone()
        } else {
            // Parsed through the type by the type's own name (not the property's), as upstream does.
            self.get_parsed_value_or_default(editor_config_property)
                .unwrap_or_else(|| {
                    editor_config_property
                        .default_value_for(self.code_style())
                        .clone()
                })
        }
    }

    /// `getEditorConfigValueOrNull(propertyType, propertyName)`.
    pub fn get_editor_config_value_or_null<T: Clone>(
        &self,
        property_type: &PropertyType<T>,
        property_name: &str,
    ) -> Option<T> {
        self.get_property_value(property_type, property_name)
    }

    pub fn contains(&self, property_name: &str) -> bool {
        self.property(property_name).is_some()
    }

    fn get_parsed_value_or_default<T: PropertyValueType>(
        &self,
        p: &EditorConfigProperty<T>,
    ) -> Option<T> {
        let property = self.property(p.type_.name);
        let source = property.and_then(Property::source_value);
        let property_value = match property
            .filter(|property| property.is_parsed_by(p.type_))
            .and_then(|property| property.value().typed::<T>())
        {
            Some(value) => PropertyValue::valid(source, Some(value.clone())),
            None => p.type_.parse(source),
        };
        if property_value.is_valid() {
            property_value.into_parsed()
        } else {
            Some(p.default_value.clone())
        }
    }

    fn get_property_value<T: Clone>(
        &self,
        property_type: &PropertyType<T>,
        property_name: &str,
    ) -> Option<T> {
        let property = self.property(property_name);
        let source = property.and_then(Property::source_value);
        match property
            .filter(|property| property.is_parsed_by(property_type))
            .and_then(|property| property.value().typed::<T>())
        {
            Some(value) => PropertyValue::valid(source, Some(value.clone())).into_parsed(),
            None => property_type.parse(source).into_parsed(),
        }
    }

    /// `map(mapper)`: over the properties in order.
    pub fn map<T>(&self, mapper: impl FnMut(&Property) -> T) -> Vec<T> {
        self.properties.iter().map(mapper).collect()
    }

    /// Adds the given properties at their default for the active code style, unless already defined.
    pub fn add_properties_with_default_value_if_missing(
        &self,
        additional: &[PropertyRef],
    ) -> EditorConfig {
        let mut missing: Vec<&PropertyRef> = Vec::new();
        for p in additional.iter().filter(|p| !self.contains(p.name())) {
            if !missing.iter().any(|m| m.identity() == p.identity()) {
                missing.push(p);
            }
        }
        require_singular_identities(&missing);
        let code_style = self.code_style();
        let mut properties: Vec<Property> = missing
            .iter()
            .map(|p| to_property_with_value(&***p, &p.write_default_value(code_style)))
            .collect();
        properties.extend(self.properties.iter().cloned());
        EditorConfig::new(properties)
    }

    /// `filterBy`: despite its name, the whole config plus the given properties at their defaults.
    pub fn filter_by(&self, additional: &[PropertyRef]) -> EditorConfig {
        self.add_properties_with_default_value_if_missing(additional)
    }
}

fn require_singular_identities(editor_config_properties: &[&PropertyRef]) {
    let mut by_name: HashMap<&str, Vec<&PropertyRef>> = HashMap::new();
    editor_config_properties
        .iter()
        .for_each(|p| by_name.entry(p.name()).or_default().push(p));
    let mut clashes: Vec<String> = by_name
        .into_iter()
        .filter(|(_, properties)| properties.len() > 1)
        .map(|(name, properties)| {
            let list: Vec<String> = properties.iter().map(|p| format!("  - {p:?}")).collect();
            format!("Found multiple editorconfig properties with name '{name}' but having distinct identities:\n{}", list.join("\n"))
        })
        .collect();
    clashes.sort();
    assert!(
        clashes.is_empty(),
        "IllegalArgumentException: {}",
        clashes.join("\n")
    );
}
