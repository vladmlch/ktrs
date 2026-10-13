//! Port of ec4j `ResourcePropertiesService.java`, `ResourceProperties.java` and the `Cache` interface.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::editor_config::{EditorConfig, Property};
use crate::parser::{self, ParseException, PropertyTypeRegistry};

/// `Cache`: `.editorconfig` files by path, loaded on a miss.
pub trait Cache: Sync {
    fn get(
        &self,
        config_file: &Path,
        registry: &PropertyTypeRegistry,
    ) -> Result<Arc<EditorConfig>, ParseException>;
}

/// `Cache.Caches.none()`.
pub struct NoCache;

impl Cache for NoCache {
    fn get(
        &self,
        config_file: &Path,
        registry: &PropertyTypeRegistry,
    ) -> Result<Arc<EditorConfig>, ParseException> {
        parser::load(config_file, registry).map(Arc::new)
    }
}

/// `ResourceProperties`: the effective properties (a `LinkedHashMap`, so a later section overrides a
/// value in place) and the `.editorconfig` files that were read, nearest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceProperties {
    properties: Vec<Property>,
    editor_config_files: Vec<PathBuf>,
}

impl ResourceProperties {
    pub fn properties(&self) -> &[Property] {
        &self.properties
    }

    pub fn into_properties(self) -> Vec<Property> {
        self.properties
    }

    pub fn editor_config_files(&self) -> &[PathBuf] {
        &self.editor_config_files
    }

    fn property(&mut self, property: &Property) {
        match self
            .properties
            .iter_mut()
            .find(|p| p.name() == property.name())
        {
            Some(existing) => *existing = property.clone(),
            None => self.properties.push(property.clone()),
        }
    }

    fn remove_property(&mut self, property: &Property) {
        self.properties.retain(|p| p.name() != property.name());
    }
}

pub struct ResourcePropertiesService<'a> {
    pub cache: &'a dyn Cache,
    pub config_file_name: &'a str,
    pub default_editor_configs: Vec<Arc<EditorConfig>>,
    pub keep_unset: bool,
    pub registry: &'a PropertyTypeRegistry,
    pub root_directories: Vec<PathBuf>,
}

impl ResourcePropertiesService<'_> {
    /// `queryProperties(resource)`: `.editorconfig` files from the resource's directory upwards until a
    /// `root = true`; then the defaults (as if in the last directory); applied farthest first.
    pub fn query_properties(&self, resource: &Path) -> Result<ResourceProperties, ParseException> {
        let editor_configs = self.discover_chain(resource)?;
        Ok(self.apply_chain(resource, &editor_configs))
    }

    /// The lookup half of `query_properties`: `(directory, config)` pairs for the `.editorconfig` files
    /// found walking up from the resource's directory, nearest first. It depends on `resource`'s
    /// directory only, so every resource in one directory shares the chain.
    pub fn discover_chain(
        &self,
        resource: &Path,
    ) -> Result<Vec<(PathBuf, Arc<EditorConfig>)>, ParseException> {
        let mut editor_configs: Vec<(PathBuf, Arc<EditorConfig>)> = Vec::new();
        let mut root = false;
        let mut dir = resource.parent();
        while let Some(d) = dir.filter(|_| !root) {
            let config_file = d.join(self.config_file_name);
            if config_file.exists() {
                let config = self.cache.get(&config_file, self.registry)?;
                root = config.is_root();
                editor_configs.push((d.to_path_buf(), config));
            }
            root |= self.root_directories.iter().any(|r| r == d);
            dir = d.parent();
        }
        Ok(editor_configs)
    }

    /// The merge half of `query_properties` over a `discover_chain` chain: the defaults apply as if in
    /// the chain's last (farthest) directory — the resource's when it is empty — then every config's
    /// matching sections apply farthest first.
    pub fn apply_chain(
        &self,
        resource: &Path,
        chain: &[(PathBuf, Arc<EditorConfig>)],
    ) -> ResourceProperties {
        let mut result = ResourceProperties::default();
        result.editor_config_files = chain
            .iter()
            .map(|(d, _)| d.join(self.config_file_name))
            .collect();
        let mut editor_configs: Vec<(PathBuf, Arc<EditorConfig>)> = chain.to_vec();
        if !self.default_editor_configs.is_empty() {
            let last_dir = match editor_configs.last() {
                None => resource.parent().map(Path::to_path_buf).unwrap_or_default(),
                Some((d, _)) => d.clone(),
            };
            for ec in &self.default_editor_configs {
                editor_configs.push((last_dir.clone(), ec.clone()));
            }
        }
        for (editor_config_dir, config) in editor_configs.iter().rev() {
            let path = relativize(editor_config_dir, resource);
            for section in config.sections() {
                if !section.is_match(&path) {
                    continue;
                }
                if self.keep_unset {
                    section.properties().iter().for_each(|p| result.property(p));
                } else {
                    for prop in section.properties() {
                        if prop.is_unset() {
                            result.remove_property(prop);
                        } else {
                            result.property(prop);
                        }
                    }
                }
            }
        }
        result
    }
}

/// `editorConfigDir.relativize(resource).getPath()` as a `/`-separated string (`NioPath.toString` on
/// Windows replaces `\`).
fn relativize(dir: &Path, resource: &Path) -> String {
    let relative = resource.strip_prefix(dir).unwrap_or(resource);
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
