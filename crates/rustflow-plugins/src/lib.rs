//! WASM plugin system for RustFlow.
//!
//! Plugins are compiled WebAssembly modules that export custom tools.
//! Each tool implements the [`rustflow_tools::tool::Tool`] trait and can be
//! registered into a [`rustflow_tools::registry::ToolRegistry`] alongside the
//! built-in tools.
//!
//! # Plugin ABI
//!
//! A compliant plugin must:
//! - Export a linear memory named `"memory"`
//! - Import `"rustflow"` → `"log"(level: i32, ptr: i32, len: i32)` for logging
//! - Export `rustflow_alloc(size: i32) -> i32`
//! - Export `rustflow_dealloc(ptr: i32, size: i32)`
//! - Export `rustflow_plugin_manifest() -> i64` — returns packed (ptr << 32 | len)
//!   pointing to a UTF-8 JSON [`PluginManifest`] in linear memory
//! - Export `rustflow_tool_execute(tool_name_ptr, tool_name_len, input_ptr, input_len) -> i64`
//!   — returns packed (out_ptr << 32 | out_len) pointing to the output JSON,
//!   or 0 on error (the JSON may also contain `{"error": "..."}`)
//!
//! See [`crate::abi`] for the exact encoding of packed pointer/length pairs.

pub mod abi;
pub mod error;
pub mod instance;
pub mod loader;
pub mod manifest;
pub mod plugin_tool;

pub use error::PluginError;
pub use instance::PluginInstance;
pub use loader::PluginLoader;
pub use manifest::{PluginManifest, ToolManifest};
pub use plugin_tool::PluginTool;
