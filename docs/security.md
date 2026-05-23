# Security Defaults

RustFlow currently uses policy checks around built-in tools and a Wasmtime
runtime for plugins. These boundaries reduce accidental exposure, but they are
not a durable host sandbox or process isolation layer.

## File Access

`file_read`, `file_write`, and shell working directories use
`SecurityPolicy.fs` / `FsPolicy`.

- `allowed_dirs` defaults to an empty list, which means the current working
  directory only.
- Paths are made absolute, existing paths are canonicalized, and new paths are
  checked through the nearest existing ancestor before the allowed-directory
  check.
- Existing symlinks are rejected unless `allow_symlinks` is enabled.
- Reads and writes share `max_file_size`, which defaults to 50 MB.
- Default blocked path patterns include `/etc/shadow`, `/etc/passwd`,
  `/etc/sudoers`, `.ssh`, `.gnupg`, `.aws/credentials`, and `.env`.

## Network Access

The `http` tool uses `SecurityPolicy.network` / `NetworkPolicy`.

- Only `http` and `https` URLs are accepted.
- Redirects are not followed automatically.
- Localhost names, loopback, link-local, unspecified, and known cloud metadata
  addresses are blocked by default.
- Hostnames are checked before DNS resolution; resolved IP addresses are also
  checked unless `allow_local_targets` is enabled.
- Response bodies are capped by `max_http_response_size`, which defaults to
  10 MB.

This is not an OS-level egress firewall. Enable local targets only for trusted
workflows that intentionally call local services.

## Shell Execution

The `shell` tool uses `SecurityPolicy.shell` / `ShellPolicy`.

- Shell execution is disabled by default.
- `--allow-shell` for the CLI, server, and playground only enables the tool. It
  does not enable shell parsing mode.
- Direct execution is the default. `shell: true` requires `allow_shell_mode`.
- Direct shell interpreter execution such as `sh`, `bash`, `zsh`, `cmd`,
  `powershell`, or `pwsh` is rejected unless shell parsing mode is allowed.
- `allowed_commands`, when non-empty, is an executable whitelist for direct
  execution.
- Child processes start with a cleared environment and inherit only configured
  keys. By default this is `PATH` on Unix, plus `SystemRoot` and `PATHEXT` on
  Windows.
- User-provided environment variables are denied unless their keys are listed in
  `allowed_env_keys`.
- Dynamic loader variables such as `LD_PRELOAD`, `LD_LIBRARY_PATH`,
  `DYLD_INSERT_LIBRARIES`, and `DYLD_LIBRARY_PATH` are filtered.
- Output is capped at 1 MB by default, and requested timeouts are clamped to a
  300 second default maximum.

## Environment Access

The `env` tool uses `SecurityPolicy.env` / `EnvPolicy`.

- Dumping all environment variables is disabled by default.
- Reading a named variable is allowed, but sensitive names are redacted
  case-insensitively when they contain patterns such as `KEY`, `SECRET`,
  `PASSWORD`, `PASSWD`, `TOKEN`, `CREDENTIAL`, or `PRIVATE`.

## Doctor Security Checks

`rustflow doctor --security` performs read-only checks for
`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GLM_API_KEY`, `.env`, and
`RUSTFLOW_RUN_STORE_DIR`. It reports credential variables only as `set` or
`not set`; it never prints secret values or key prefixes.

`rustflow doctor --full` uses the same no-secret provider key reporting.

## WASM Plugins

`PluginLoader::load_file` reads the supplied `.wasm` path and loads the bytes
with Wasmtime.

- Plugin file loading is separate from `FsPolicy`; the loader does not enforce
  `allowed_dirs` or a manifest grant.
- RustFlow links the `rustflow.log` host import. Other required imports fail
  instantiation unless the loader is extended to provide them.
- Manifest and tool-result strings are read through checked linear-memory ABI
  helpers.
- The current loader does not configure fuel, epoch interruption, memory limits,
  or per-plugin wall-clock timeouts.
- `add_search_path` only stores paths for future discovery. It is not a
  security grant mechanism.

Use trusted plugin paths and constrain who can configure plugin loading.
