//! Port of ktlint-rule-engine `internal/EditorConfigLoader.kt` (with `EditorConfigLoaderEc4j`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ktrs_editorconfig::property_type::TAB_WIDTH;
use ktrs_editorconfig::{
    AnyPropertyType, EditorConfig as Ec4jEditorConfig, ParseException, Property,
    PropertyTypeRegistry, ResourcePropertiesService,
};

use crate::editorconfig::{
    CODE_STYLE_PROPERTY, END_OF_LINE_PROPERTY, EXPERIMENTAL_RULES_EXECUTION_PROPERTY, EditorConfig,
    INDENT_SIZE_PROPERTY, PropertyRef, to_property_with_parsed_value,
};
use crate::engine::editor_config_cache::THREAD_SAFE_EDITOR_CONFIG_CACHE;
use crate::engine::editor_config_defaults::{EditorConfigDefaults, EditorConfigOverride};
use crate::engine::formatter_tags::{
    FORMATTER_TAG_OFF_ENABLED_PROPERTY, FORMATTER_TAG_ON_ENABLED_PROPERTY,
    FORMATTER_TAGS_ENABLED_PROPERTY,
};

/// File extensions for which the `.editorconfig` lookup is done; a code snippet is looked up as if it were
/// `<cwd>/.kt`.
pub const SUPPORTED_FILES: [&str; 2] = [".kt", ".kts"];

const TAB_WIDTH_PROPERTY_NAME: &str = "tab_width";

/// `EditorConfigLoaderEc4j`: ec4j's loader, aware of the standard types plus those of the rules.
#[derive(Clone, Debug)]
pub struct EditorConfigLoaderEc4j {
    registry: PropertyTypeRegistry,
}

impl EditorConfigLoaderEc4j {
    pub fn new(property_types: &[&'static dyn AnyPropertyType]) -> EditorConfigLoaderEc4j {
        EditorConfigLoaderEc4j {
            registry: PropertyTypeRegistry::with_defaults(property_types.iter().copied()),
        }
    }

    pub fn registry(&self) -> &PropertyTypeRegistry {
        &self.registry
    }
}

/// The `.editorconfig` chain `ResourcePropertiesService::discover_chain` walks from a file in a
/// directory: `(dir, config)` pairs, nearest first.
type DirChain = Vec<(PathBuf, Arc<Ec4jEditorConfig>)>;

#[derive(Clone, Debug)]
pub struct EditorConfigLoader {
    editor_config_loader_ec4j: EditorConfigLoaderEc4j,
    editor_config_defaults: EditorConfigDefaults,
    editor_config_override: EditorConfigOverride,
    /// `discover_chain` results by the directory the walk starts at: the walk's `exists` stats and cache
    /// lookups are identical for every file in a directory, so it runs once per directory per loader.
    dir_chains: Arc<Mutex<HashMap<PathBuf, Arc<DirChain>>>>,
}

impl EditorConfigLoader {
    pub fn new(
        editor_config_loader_ec4j: EditorConfigLoaderEc4j,
        editor_config_defaults: EditorConfigDefaults,
        editor_config_override: EditorConfigOverride,
    ) -> EditorConfigLoader {
        EditorConfigLoader {
            editor_config_loader_ec4j,
            editor_config_defaults,
            editor_config_override,
            dir_chains: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// `load(filePath)`: the `.editorconfig` files from `file_path` upwards (up to `root = true`), then
    /// the defaults, then the overrides, plus the engine's own properties at their defaults.
    pub fn load(&self, file_path: Option<&Path>) -> Result<EditorConfig, ParseException> {
        let editor_config_path = match file_path {
            Some(path) => to_absolute_path(path),
            None => default_file_path(),
        };
        let service = self.create_resource_properties_service();
        let chain = self.dir_chain(&editor_config_path, &service)?;
        let mut properties = service
            .apply_chain(&editor_config_path, &chain)
            .into_properties();
        let source = |properties: &[Property], name: &str| {
            properties
                .iter()
                .find(|p| p.name() == name)
                .map(|p| p.source_value().map(str::to_owned))
        };
        let indent_size_override = self
            .editor_config_override
            .get(&PropertyRef::from(&*INDENT_SIZE_PROPERTY));
        // tab_width can't be overridden; keep it in sync with an overridden indent_size it equalled
        // (absent == absent counts as equal).
        if source(&properties, TAB_WIDTH_PROPERTY_NAME)
            == source(&properties, &INDENT_SIZE_PROPERTY.name)
            && let Some(indent_size) = indent_size_override
        {
            put(
                &mut properties,
                Property::new(
                    TAB_WIDTH_PROPERTY_NAME,
                    Some(&TAB_WIDTH),
                    indent_size.source(),
                ),
            );
        }
        for (key, value) in self.editor_config_override.properties() {
            put(
                &mut properties,
                to_property_with_parsed_value(&**key, value.clone()),
            );
        }
        Ok(
            EditorConfig::new(properties).add_properties_with_default_value_if_missing(&[
                PropertyRef::from(&*CODE_STYLE_PROPERTY),
                PropertyRef::from(&*END_OF_LINE_PROPERTY),
                PropertyRef::from(&*EXPERIMENTAL_RULES_EXECUTION_PROPERTY),
                PropertyRef::from(&*FORMATTER_TAGS_ENABLED_PROPERTY),
                PropertyRef::from(&*FORMATTER_TAG_OFF_ENABLED_PROPERTY),
                PropertyRef::from(&*FORMATTER_TAG_ON_ENABLED_PROPERTY),
            ]),
        )
    }

    /// The `discover_chain` walk's result for `resource`, memoized on `resource`'s directory: the walk
    /// (the `exists` stats, `cache` lookups and `root` checks) is the same for every file in it. Errors
    /// are not memoized (the Java cache throws before the put too).
    fn dir_chain(
        &self,
        resource: &Path,
        service: &ResourcePropertiesService<'_>,
    ) -> Result<Arc<DirChain>, ParseException> {
        let dir = resource.parent().map(Path::to_path_buf).unwrap_or_default();
        if let Some(chain) = self.dir_chains.lock().unwrap().get(&dir) {
            return Ok(chain.clone());
        }
        let chain = Arc::new(service.discover_chain(resource)?);
        self.dir_chains.lock().unwrap().insert(dir, chain.clone());
        Ok(chain)
    }

    /// Drops the memoized chains, which keep the `Arc<EditorConfig>` the cache returned when the walk
    /// ran: `KtLintRuleEngine::trim_memory`/`reload_editor_config_file` clear them with the cache.
    pub fn clear_dir_chains(&self) {
        self.dir_chains.lock().unwrap().clear();
    }

    fn create_resource_properties_service(&self) -> ResourcePropertiesService<'_> {
        let defaults = &self.editor_config_defaults;
        ResourcePropertiesService {
            cache: &*THREAD_SAFE_EDITOR_CONFIG_CACHE,
            config_file_name: ".editorconfig",
            default_editor_configs: if *defaults == EditorConfigDefaults::empty() {
                Vec::new()
            } else {
                vec![defaults.value.clone()]
            },
            keep_unset: true,
            registry: self.editor_config_loader_ec4j.registry(),
            root_directories: Vec::new(),
        }
    }
}

fn put(properties: &mut Vec<Property>, property: Property) {
    match properties.iter_mut().find(|p| p.name() == property.name()) {
        Some(existing) => *existing = property,
        None => properties.push(property),
    }
}

/// `Path.toAbsolutePath()`: joined onto the working directory, `..` kept.
fn to_absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}

/// `fileSystem.getPath(".").toAbsolutePath().resolve(".kt")`. The JVM path keeps the `.` segment and so
/// reads `<cwd>/.editorconfig` twice (as `<cwd>/.` and `<cwd>`); applying it once gives the same result.
fn default_file_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_default()
        .join(SUPPORTED_FILES[0])
}
