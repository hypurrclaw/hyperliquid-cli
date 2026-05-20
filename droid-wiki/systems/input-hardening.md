# Input hardening

Agent-supplied payloads can be arbitrarily nested or huge. `src/input_hardening.rs` clamps them so a hostile or malformed file cannot exhaust memory or escape the working directory.

## Key file

`src/input_hardening.rs` (~430 lines). The module exports `FilePolicy`, `read_json_file`, path validation helpers, and reusable regexes (via `regex_lite`).

## FilePolicy

```rust
pub struct FilePolicy {
    label: &'static str,
    allow_stdin: bool,
    max_bytes: u64,
}
```

Constructor presets:

- `FilePolicy::payload()` — `label: "payload"`, `allow_stdin: true`, `max_bytes: 1 MiB`.

Tightening any of these limits is intentional. The 1 MiB cap is enough for batch order JSON files in practice but too small for accidental dumps of `target/`.

## JSON shape limits

When `read_json_file` parses the file contents:

| Limit | Value |
|-------|-------|
| Max bytes | 1 MiB (1048576) |
| Max nesting depth | 64 |
| Max keys per object | 4096 |
| Max string length | 64 KiB |

Violations surface as `CliError::Unsupported` (exit 13) with a sanitized message.

## Path validation

`Component`-walking guards reject:

- `..` traversal
- Absolute paths outside the working directory unless explicitly allowed
- Symlinks that escape the policy's root

This is used by `--payload-file <PATH>` and any other command that opens a caller-specified file. Stdin (`-`) is treated as a special token and only accepted when `allow_stdin` is true.

## How commands wire it in

Commands that accept raw payloads (`orders create`, `orders modify`, `transfer send-asset`, etc.) call `read_json_file(path, FilePolicy::payload())` before deserializing into the typed action shape. The decoded JSON is also passed through `serde_json::Value` once for shape validation before being typed-deserialized — that way size and depth limits run against the parsed structure, not just the raw bytes.

See [features/raw-payload](../features/raw-payload.md) for the user-facing contract.

## Entry points for modification

- To loosen size limits for a specific command, add a new `FilePolicy` constructor (`FilePolicy::large_batch()` or similar) rather than raising `DEFAULT_MAX_JSON_FILE_BYTES` globally.
- To add a new validation rule, extend `read_json_file` and add a test under `tests/security_contracts.rs`.
- To accept binary payload formats, build a parallel reader; do not weaken JSON limits.
