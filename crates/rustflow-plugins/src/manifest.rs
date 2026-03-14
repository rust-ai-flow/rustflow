use serde::{Deserialize, Serialize};

/// Metadata for a single tool exported by a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    /// Tool name used for registration and dispatch.
    pub name: String,
    /// Human-readable description shown in tool listings.
    pub description: String,
    /// JSON Schema for the tool's input parameters.
    #[serde(default)]
    pub parameters: serde_json::Value,
}

/// Metadata declared by a WASM plugin, embedded in its linear memory and
/// returned by the `rustflow_plugin_manifest()` export.
///
/// The plugin serialises this as JSON and the host deserialises it on load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Plugin name — must be unique within a runtime.
    pub name: String,
    /// Semantic version string.
    pub version: String,
    /// Short description of the plugin.
    pub description: Option<String>,
    /// Exported tools: each entry drives creation of one `PluginTool`.
    pub tools: Vec<ToolManifest>,
    /// Minimum RustFlow API version required (informational).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_serde_roundtrip() {
        let m = PluginManifest {
            name: "my-plugin".into(),
            version: "1.0.0".into(),
            description: Some("A test plugin".into()),
            tools: vec![ToolManifest {
                name: "greet".into(),
                description: "Says hello".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": { "name": { "type": "string" } }
                }),
            }],
            min_api_version: None,
        };

        let json = serde_json::to_string(&m).unwrap();
        let m2: PluginManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m2.name, "my-plugin");
        assert_eq!(m2.tools.len(), 1);
        assert_eq!(m2.tools[0].name, "greet");
    }

    #[test]
    fn test_tool_manifest_default_parameters() {
        let json = r#"{"name":"t","description":"d"}"#;
        let t: ToolManifest = serde_json::from_str(json).unwrap();
        assert!(t.parameters.is_null());
    }
}
