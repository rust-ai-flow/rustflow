use std::sync::{Arc, Mutex};

use wasmtime::{Engine, Instance, Linker, Memory, Module, Store};

use crate::abi::{self, FN_ALLOC, FN_DEALLOC, FN_EXECUTE, FN_MANIFEST, HOST_MODULE, MEMORY};
use crate::error::{PluginError, Result};
use crate::manifest::PluginManifest;

/// Host state threaded through the wasmtime `Store`.
pub struct HostState;

/// Inner state that must be accessed together under a mutex.
pub struct PluginInner {
    pub store: Store<HostState>,
    pub instance: Instance,
    pub memory: Memory,
}

/// A loaded, instantiated WASM plugin.
///
/// `PluginInstance` is `Send + Sync` and can be shared across threads via `Arc`.
/// All access to the WASM store is serialised through the inner `Mutex`.
pub struct PluginInstance {
    pub manifest: PluginManifest,
    inner: Arc<Mutex<PluginInner>>,
}

impl PluginInstance {
    /// Compile and instantiate a WASM plugin from raw bytes.
    ///
    /// Reads the manifest from the plugin's linear memory and returns a fully
    /// initialised `PluginInstance`.
    pub fn load(engine: &Engine, wasm_bytes: &[u8]) -> Result<Self> {
        let module = Module::new(engine, wasm_bytes).map_err(|e| PluginError::LoadFailed {
            name: "<unknown>".into(),
            reason: format!("compilation failed: {e}"),
        })?;

        let mut linker: Linker<HostState> = Linker::new(engine);

        // rustflow.log(level, msg_ptr, msg_len)
        linker
            .func_wrap(
                HOST_MODULE,
                "log",
                |mut caller: wasmtime::Caller<'_, HostState>,
                 level: i32,
                 ptr: i32,
                 len: i32| {
                    let mem = match caller.get_export(MEMORY) {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return,
                    };
                    let data = mem.data(&caller);
                    match abi::read_str(data, ptr as u32, len as u32) {
                        Ok(msg) => match level {
                            0 => tracing::error!(target: "plugin", "{msg}"),
                            1 => tracing::warn!(target: "plugin", "{msg}"),
                            2 => tracing::info!(target: "plugin", "{msg}"),
                            _ => tracing::debug!(target: "plugin", "{msg}"),
                        },
                        Err(e) => tracing::warn!("plugin log: invalid message: {e}"),
                    }
                },
            )
            .map_err(|e| PluginError::LoadFailed {
                name: "<unknown>".into(),
                reason: format!("linker definition failed: {e}"),
            })?;

        let mut store = Store::new(engine, HostState);
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| PluginError::LoadFailed {
                name: "<unknown>".into(),
                reason: format!("instantiation failed: {e}"),
            })?;

        let memory = instance
            .get_memory(&mut store, MEMORY)
            .ok_or_else(|| PluginError::InvalidManifest {
                name: "<unknown>".into(),
                reason: "plugin does not export 'memory'".into(),
            })?;

        // Read the manifest from WASM linear memory.
        let manifest = {
            let manifest_fn = instance
                .get_typed_func::<(), i64>(&mut store, FN_MANIFEST)
                .map_err(|e| PluginError::InvalidManifest {
                    name: "<unknown>".into(),
                    reason: format!("missing '{FN_MANIFEST}': {e}"),
                })?;

            let packed = manifest_fn
                .call(&mut store, ())
                .map_err(|e| PluginError::WasmTrap(e.to_string()))?;

            let (ptr, len) = abi::unpack_ptr_len(packed);
            let json = {
                let data = memory.data(&store);
                abi::read_str(data, ptr, len)?
            };

            serde_json::from_str::<PluginManifest>(&json).map_err(|e| {
                PluginError::InvalidManifest {
                    name: "<unknown>".into(),
                    reason: format!("manifest JSON invalid: {e} — got: {json}"),
                }
            })?
        };

        tracing::info!(
            plugin = %manifest.name,
            version = %manifest.version,
            tools = manifest.tools.len(),
            "plugin loaded"
        );

        Ok(Self {
            manifest,
            inner: Arc::new(Mutex::new(PluginInner {
                store,
                instance,
                memory,
            })),
        })
    }

    /// Execute a named tool with a JSON input value (synchronous).
    ///
    /// Intended to be called from `tokio::task::spawn_blocking`.
    pub fn execute_tool_sync(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let plugin_name = self.manifest.name.clone();
        let mut guard = self.inner.lock().map_err(|_| PluginError::ExecutionFailed {
            name: plugin_name.clone(),
            reason: "plugin lock poisoned".into(),
        })?;

        let PluginInner {
            ref mut store,
            ref instance,
            ref memory,
        } = *guard;

        // Look up exported functions.
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut *store, FN_ALLOC)
            .map_err(|e| PluginError::AbiViolation {
                reason: format!("missing '{FN_ALLOC}': {e}"),
            })?;

        let execute = instance
            .get_typed_func::<(i32, i32, i32, i32), i64>(&mut *store, FN_EXECUTE)
            .map_err(|e| PluginError::AbiViolation {
                reason: format!("missing '{FN_EXECUTE}': {e}"),
            })?;

        let dealloc = instance.get_typed_func::<(i32, i32), ()>(&mut *store, FN_DEALLOC).ok();

        // Write tool name into WASM memory.
        let tool_name_bytes = tool_name.as_bytes();
        let name_ptr = alloc
            .call(&mut *store, tool_name_bytes.len() as i32)
            .map_err(|e| PluginError::WasmTrap(e.to_string()))?;
        {
            let data = memory.data_mut(&mut *store);
            abi::write_bytes(data, name_ptr as u32, tool_name_bytes)?;
        }

        // Write input JSON into WASM memory.
        let input_json = serde_json::to_string(input).map_err(|e| PluginError::ExecutionFailed {
            name: plugin_name.clone(),
            reason: format!("failed to serialise input: {e}"),
        })?;
        let input_bytes = input_json.as_bytes();
        let input_ptr = alloc
            .call(&mut *store, input_bytes.len() as i32)
            .map_err(|e| PluginError::WasmTrap(e.to_string()))?;
        {
            let data = memory.data_mut(&mut *store);
            abi::write_bytes(data, input_ptr as u32, input_bytes)?;
        }

        // Call `rustflow_tool_execute`.
        let packed = execute
            .call(
                &mut *store,
                (
                    name_ptr,
                    tool_name_bytes.len() as i32,
                    input_ptr,
                    input_bytes.len() as i32,
                ),
            )
            .map_err(|e| PluginError::WasmTrap(e.to_string()))?;

        // Free input allocations (best-effort).
        if let Some(dealloc) = dealloc {
            let _ = dealloc.call(&mut *store, (name_ptr, tool_name_bytes.len() as i32));
            let _ = dealloc.call(&mut *store, (input_ptr, input_bytes.len() as i32));
        }

        // Null pointer means the plugin signalled an error.
        if packed == 0 {
            return Err(PluginError::ExecutionFailed {
                name: plugin_name,
                reason: format!("plugin returned null for tool '{tool_name}'"),
            });
        }

        let (out_ptr, out_len) = abi::unpack_ptr_len(packed);
        let out_json = {
            let data = memory.data(&*store);
            abi::read_str(data, out_ptr, out_len)?
        };

        let value: serde_json::Value =
            serde_json::from_str(&out_json).map_err(|e| PluginError::ExecutionFailed {
                name: plugin_name,
                reason: format!("plugin returned invalid JSON: {e} — got: {out_json}"),
            })?;

        // If the plugin returned an error object, propagate it.
        if let Some(err_msg) = value.get("error").and_then(|v| v.as_str()) {
            return Err(PluginError::ExecutionFailed {
                name: self.manifest.name.clone(),
                reason: err_msg.to_string(),
            });
        }

        Ok(value)
    }
}
