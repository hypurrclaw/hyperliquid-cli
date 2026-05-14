# Dependencies

## Runtime dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `hypersdk` | 0.2 | Hyperliquid API client, types, signing, WebSocket |
| `clap` | 4 | CLI argument parsing (derive API) |
| `tokio` | 1 | Async runtime (multi-thread, time, io-util, process) |
| `reqwest` | 0.13 | HTTP client for API calls |
| `alloy` | 1.5.2 | Ethereum types, EIP-712, Solidity ABI, signing |
| `alloy-signer-local` | 1.8 | Local keystore support |
| `alloy-primitives` | 1 | Ethereum primitive types |
| `serde` / `serde_json` | 1 | Serialization framework |
| `rust_decimal` | 1 | Financial precision (prices, sizes, amounts) |
| `tabwriter` | 1 | ANSI-aware column alignment for pretty output |
| `tabled` | 0.20 | Bordered table rendering |
| `crossterm` | 0.29 | Terminal control (alternate screen, raw mode) |
| `strsim` | 0.11 | Levenshtein distance for fuzzy asset matching |
| `dirs` | 6 | Platform config/data directory resolution |
| `chrono` | 0.4 | Timestamp formatting and parsing |
| `futures` | 0.3 | Async stream combinators |
| `either` | 1 | Either type for polymorphic returns |
| `rusqlite` | 0.38 | SQLite with bundled compilation |
| `aes-gcm` | 0.10 | AES-256-GCM for account encryption |
| `base64` | 0.22 | Base64 encoding for encrypted blobs |
| `hex` | 0.4 | Hex encoding for keys and addresses |
| `rand` | 0.9 | Random nonce generation for encryption |
| `sha2` | 0.10 | SHA-256 for passphrase key derivation |
| `rpassword` | 7 | Hidden password/key prompts |
| `keyring` | 3 | OS keychain integration (macOS, Linux, Windows) |
| `ows-lib` | 1.3.2 | Open Wallet Standard library |
| `regex-lite` | 0.1 | Lightweight regex for input validation |
| `anyhow` | 1 | Flexible error handling |
| `thiserror` | 2 | Derive macros for error types |
| `rmp-serde` | 1 | MessagePack serialization (used in hypersdk) |

## Dev dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `assert_cmd` | 2 | CLI integration testing (run binary, check output) |
| `predicates` | 3 | Output assertions for integration tests |
| `tempfile` | 3 | Isolated temp directories for test state |
| `wiremock` | 0.6 | HTTP mock server for API simulation |

## Build toolchain

- Rust edition 2024
- Minimum Rust version: 1.93
- CI uses `dtolnay/rust-toolchain@stable` with `clippy` and `rustfmt` components
- Release builds use `--locked` for reproducible builds
