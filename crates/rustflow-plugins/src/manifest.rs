use serde::{Deserialize, Serialize};

/// Metadata declared by a WASM plugin in its manifest file (`plugin.toml`
/// or embedded in the WASM custom section).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Plugin name (must be unique within a runtime).
    pub name: String,
    /// Semantic version string.
    pub version: String,
    /// Short description of the plugin.
    pub description: Option<String>,
    /// Names of the tool functions exported by this plugin.
    pub tools: Vec<String>,
    /// Minimum RustFlow API version required.
    pub min_api_version: Option<String>,
}

impl PluginManifest {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: None,
            tools: vec![],
            min_api_version: None,
        }
    }
}
