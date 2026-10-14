//! Port of ktlint-rule-engine `ThreadSafeEditorConfigCache.kt`: every `.editorconfig` file is parsed once
//! per process, shared by all engines and threads.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, RwLock};

use ktrs_editorconfig::{Cache, EditorConfig, ParseException, PropertyTypeRegistry};

#[derive(Default)]
pub struct ThreadSafeEditorConfigCache {
    in_memory_map: RwLock<HashMap<PathBuf, (PropertyTypeRegistry, Arc<EditorConfig>)>>,
}

pub static THREAD_SAFE_EDITOR_CONFIG_CACHE: LazyLock<ThreadSafeEditorConfigCache> =
    LazyLock::new(ThreadSafeEditorConfigCache::default);

impl Cache for ThreadSafeEditorConfigCache {
    /// Parse errors are not cached (the Java `CacheValue` constructor throws before the put).
    fn get(
        &self,
        resource: &Path,
        registry: &PropertyTypeRegistry,
    ) -> Result<Arc<EditorConfig>, ParseException> {
        if let Some((_, cached)) = self.in_memory_map.read().unwrap().get(resource) {
            return Ok(cached.clone());
        }
        let edit_config = Arc::new(ktrs_editorconfig::load(resource, registry)?);
        self.in_memory_map.write().unwrap().insert(
            resource.to_path_buf(),
            (registry.clone(), edit_config.clone()),
        );
        Ok(edit_config)
    }
}

impl ThreadSafeEditorConfigCache {
    /// Reloads a cached `.editorconfig` file with the registry it was first loaded with.
    pub fn reload_if_exists(&self, resource: &Path) -> Result<(), ParseException> {
        let registry = match self.in_memory_map.read().unwrap().get(resource) {
            Some((registry, _)) => registry.clone(),
            None => return Ok(()),
        };
        match ktrs_editorconfig::load(resource, &registry) {
            Ok(edit_config) => {
                self.in_memory_map
                    .write()
                    .unwrap()
                    .insert(resource.to_path_buf(), (registry, Arc::new(edit_config)));
                Ok(())
            }
            Err(e) => {
                self.in_memory_map.write().unwrap().remove(resource);
                Err(e)
            }
        }
    }

    pub fn clear(&self) {
        self.in_memory_map.write().unwrap().clear();
    }

    pub fn get_paths(&self) -> Vec<PathBuf> {
        self.in_memory_map.read().unwrap().keys().cloned().collect()
    }
}
