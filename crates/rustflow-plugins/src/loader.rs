use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::error::{PluginError, Result};
use crate::manifest::PluginManifest;

/// A loaded plugin instance (placeholder until WASM runtime is integrated).
#[derive(Debug)]
pub struct Plugin {
    pub manifest: PluginManifest,
    pub path: PathBuf,
}

/// Loads and manages WASM plugins.
///
/// # Usage
/// ```rust,ignore
/// let mut loader = PluginLoader::new();
/// loader.load("/path/to/plugin.wasm")?;
/// ```
pub struct PluginLoader {
    plugins: HashMap<String, Plugin>,
    search_paths: Vec<PathBuf>,
}

impl PluginLoader {
    /// Create a new loader with no search paths.
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            search_paths: vec![],
        }
    }

    /// Add a directory to search for plugins.
    pub fn add_search_path(&mut self, path: impl Into<PathBuf>) {
        self.search_paths.push(path.into());
    }

    /// Load a WASM plugin from an explicit file path.
    ///
    /// Currently validates that the file exists and records the plugin.
    /// Full WASM instantiation will be added when a WASM runtime is chosen.
    pub fn load(&mut self, path: impl AsRef<Path>) -> Result<&Plugin> {
        let path = path.as_ref().to_path_buf();

        if !path.exists() {
            return Err(PluginError::NotFound {
                path: path.display().to_string(),
            });
        }

        // TODO: parse the embedded manifest from the WASM custom section.
        // For now, derive the name from the filename.
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        info!(plugin = %name, path = %path.display(), "loading plugin");

        let manifest = PluginManifest::new(&name, "0.0.0");
        let plugin = Plugin {
            manifest,
            path: path.clone(),
        };

        self.plugins.insert(name.clone(), plugin);
        Ok(self.plugins.get(&name).unwrap())
    }

    /// Look up a loaded plugin by name.
    pub fn get(&self, name: &str) -> Option<&Plugin> {
        self.plugins.get(name)
    }

    /// List all loaded plugin manifests.
    pub fn list(&self) -> Vec<&PluginManifest> {
        self.plugins.values().map(|p| &p.manifest).collect()
    }

    /// Unload a plugin by name.
    pub fn unload(&mut self, name: &str) -> bool {
        if self.plugins.remove(name).is_some() {
            warn!(plugin = %name, "plugin unloaded");
            true
        } else {
            false
        }
    }
}

impl Default for PluginLoader {
    fn default() -> Self {
        Self::new()
    }
}
