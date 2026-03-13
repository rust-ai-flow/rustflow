//! WASM plugin loader for RustFlow.
//!
//! This crate provides the infrastructure for loading and executing WebAssembly
//! plugins that extend RustFlow with custom tools and behaviours.
//!
//! # Status
//! The WASM runtime integration (e.g. `wasmtime` or `wasmer`) is not yet wired
//! in. `PluginLoader` currently holds the intended API surface; the actual WASM
//! host-function bindings will be added in a follow-up.

pub mod error;
pub mod loader;
pub mod manifest;

pub use error::PluginError;
pub use loader::PluginLoader;
pub use manifest::PluginManifest;
