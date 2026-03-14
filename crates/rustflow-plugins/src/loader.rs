use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use wasmtime::Engine;

use crate::error::{PluginError, Result};
use crate::instance::PluginInstance;
use crate::manifest::PluginManifest;
use crate::plugin_tool::PluginTool;

/// Loads and manages WASM plugins, exposing each exported tool as a
/// [`PluginTool`] that implements the [`rustflow_tools::tool::Tool`] trait.
///
/// # Example
/// ```rust,ignore
/// let mut loader = PluginLoader::new();
/// let tools = loader.load_file("/path/to/plugin.wasm")?;
/// // Register tools in a ToolRegistry:
/// for tool in tools { registry.register(tool).ok(); }
/// ```
pub struct PluginLoader {
    engine: Engine,
    /// Loaded instances keyed by plugin name.
    instances: HashMap<String, Arc<PluginInstance>>,
    /// Search paths for future auto-discovery.
    search_paths: Vec<PathBuf>,
}

impl PluginLoader {
    /// Create a new loader with a default wasmtime `Engine`.
    pub fn new() -> Self {
        Self {
            engine: Engine::default(),
            instances: HashMap::new(),
            search_paths: vec![],
        }
    }

    /// Add a directory to search for plugins (used by future auto-discovery).
    pub fn add_search_path(&mut self, path: impl Into<PathBuf>) {
        self.search_paths.push(path.into());
    }

    /// Load a WASM plugin from raw bytes.
    ///
    /// Compiles, instantiates, and reads the embedded manifest.
    /// Returns the [`PluginTool`]s exported by the plugin.
    pub fn load_bytes(&mut self, wasm_bytes: &[u8]) -> Result<Vec<PluginTool>> {
        let instance = PluginInstance::load(&self.engine, wasm_bytes)?;
        self.register_instance(instance)
    }

    /// Load a WASM plugin from a `.wasm` file on disk.
    pub fn load_file(&mut self, path: impl AsRef<Path>) -> Result<Vec<PluginTool>> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(PluginError::NotFound {
                path: path.display().to_string(),
            });
        }
        let bytes = std::fs::read(path)?;
        self.load_bytes(&bytes)
    }

    /// Returns the manifest of a loaded plugin by name.
    pub fn manifest(&self, name: &str) -> Option<&PluginManifest> {
        self.instances.get(name).map(|i| &i.manifest)
    }

    /// Returns the manifests of all loaded plugins.
    pub fn all_manifests(&self) -> Vec<&PluginManifest> {
        self.instances.values().map(|i| &i.manifest).collect()
    }

    /// Returns true if a plugin with the given name has been loaded.
    pub fn contains(&self, name: &str) -> bool {
        self.instances.contains_key(name)
    }

    /// Number of loaded plugins.
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// True if no plugins are loaded.
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Unload a plugin by name.
    ///
    /// Any `PluginTool` instances already returned keep the `PluginInstance`
    /// alive via their `Arc` until they are also dropped.
    pub fn unload(&mut self, name: &str) -> bool {
        self.instances.remove(name).is_some()
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    fn register_instance(&mut self, instance: PluginInstance) -> Result<Vec<PluginTool>> {
        let plugin_name = instance.manifest.name.clone();

        if self.instances.contains_key(&plugin_name) {
            return Err(PluginError::LoadFailed {
                name: plugin_name,
                reason: "a plugin with this name is already loaded".into(),
            });
        }

        let instance = Arc::new(instance);

        let tools: Vec<PluginTool> = instance
            .manifest
            .tools
            .iter()
            .map(|t| {
                PluginTool::new(
                    t.name.clone(),
                    t.description.clone(),
                    t.parameters.clone(),
                    Arc::clone(&instance),
                )
            })
            .collect();

        self.instances.insert(plugin_name, instance);
        Ok(tools)
    }
}

impl Default for PluginLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflow_tools::tool::Tool;

    /// Minimal WAT plugin used for loader tests.
    ///
    /// Manifest JSON (147 bytes) at offset 0:
    /// {"name":"test-plugin","version":"0.1.0","description":"A test plugin",
    ///  "tools":[{"name":"greet","description":"Says hi","parameters":{"type":"object"}}]}
    ///
    /// Output JSON (19 bytes) at offset 2048:
    /// {"greeting":"hello!"}
    const TEST_WAT: &str = r#"(module
  (import "rustflow" "log" (func (param i32 i32 i32)))
  (memory (export "memory") 1)

  ;; Manifest JSON at offset 0 (152 bytes)
  (data (i32.const 0)
    "{\"name\":\"test-plugin\",\"version\":\"0.1.0\",\"description\":\"A test plugin\",\"tools\":[{\"name\":\"greet\",\"description\":\"Says hi\",\"parameters\":{\"type\":\"object\"}}]}")

  ;; Output JSON at offset 2048 (21 bytes)
  (data (i32.const 2048) "{\"greeting\":\"hello!\"}")

  ;; Bump-allocator heap starts at 4096
  (global $heap (mut i32) (i32.const 4096))

  (func (export "rustflow_alloc") (param $size i32) (result i32)
    (local $ptr i32)
    (local.set $ptr (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $size)))
    (local.get $ptr))

  (func (export "rustflow_dealloc") (param $ptr i32) (param $size i32))

  ;; Returns manifest at offset 0, length 152
  (func (export "rustflow_plugin_manifest") (result i64)
    (i64.const 152))

  ;; Returns fixed output at offset 2048, length 21
  (func (export "rustflow_tool_execute")
    (param $tool_ptr i32) (param $tool_len i32)
    (param $input_ptr i32) (param $input_len i32)
    (result i64)
    (i64.or
      (i64.shl (i64.const 2048) (i64.const 32))
      (i64.const 21)))
)"#;

    fn test_wasm() -> Vec<u8> {
        wat::parse_str(TEST_WAT).expect("WAT parse failed")
    }

    #[test]
    fn test_loader_new_is_empty() {
        let loader = PluginLoader::new();
        assert!(loader.is_empty());
        assert_eq!(loader.len(), 0);
    }

    #[test]
    fn test_load_bytes_succeeds() {
        let mut loader = PluginLoader::new();
        let tools = loader.load_bytes(&test_wasm()).expect("load failed");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name(), "greet");
        assert_eq!(loader.len(), 1);
        assert!(loader.contains("test-plugin"));
    }

    #[test]
    fn test_manifest_accessible() {
        let mut loader = PluginLoader::new();
        loader.load_bytes(&test_wasm()).unwrap();
        let manifest = loader.manifest("test-plugin").unwrap();
        assert_eq!(manifest.version, "0.1.0");
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools[0].name, "greet");
    }

    #[test]
    fn test_duplicate_load_rejected() {
        let mut loader = PluginLoader::new();
        loader.load_bytes(&test_wasm()).unwrap();
        let err = loader.load_bytes(&test_wasm()).unwrap_err();
        assert!(matches!(err, PluginError::LoadFailed { .. }));
    }

    #[test]
    fn test_unload() {
        let mut loader = PluginLoader::new();
        loader.load_bytes(&test_wasm()).unwrap();
        assert!(loader.unload("test-plugin"));
        assert!(!loader.contains("test-plugin"));
        assert!(loader.is_empty());
    }

    #[test]
    fn test_file_not_found() {
        let mut loader = PluginLoader::new();
        let err = loader.load_file("/nonexistent/path/plugin.wasm").unwrap_err();
        assert!(matches!(err, PluginError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_plugin_tool_execute() {
        let mut loader = PluginLoader::new();
        let tools = loader.load_bytes(&test_wasm()).unwrap();
        let tool = &tools[0];

        let ctx = rustflow_core::context::Context::new();
        let result = rustflow_tools::tool::Tool::execute(
            tool,
            serde_json::json!({"name": "World"}),
            &ctx,
        )
        .await
        .expect("execute failed");

        assert_eq!(result.inner()["greeting"], "hello!");
    }
}
