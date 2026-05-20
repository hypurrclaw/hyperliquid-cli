# Dependencies

From `Cargo.toml`, grouped by purpose. Versions reflect v0.11.0.

## SDK and networking

| Crate | Version | Role |
|-------|---------|------|
| `hypersdk` | 0.2 | Hyperliquid API types, signing helpers, WebSocket client |
| `reqwest` | 0.13 (`json`) | HTTP client for `/info` and `/exchange` |

## CLI and async

| Crate | Version | Role |
|-------|---------|------|
| `clap` | 4 (`derive`) | CLI argument parsing |
| `tokio` | 1 (`rt-multi-thread`, `macros`, `process`, `time`, `io-util`) | Async runtime |
| `futures` | 0.3 | Stream utilities |
| `either` | 1 | Sum types for command planning |
| `regex-lite` | 0.1 | Lightweight regex for input validation |

## Signing and crypto

| Crate | Version | Role |
|-------|---------|------|
| `alloy` | 2.0.4 (`dyn-abi`, `eip712`, `sol-types`, `signers`) | App-level EIP-712 typed data |
| `alloy-signer-local` | 2.0.4 (`keystore`) | Foundry keystore |
| `alloy-v1` (package = `alloy`) | 1.8 (`signers`) | v1 signer trait (`SignerSync`) for hypersdk compatibility |
| `alloy-signer-local-v1` (package = `alloy-signer-local`) | 1.8 (`keystore`) | v1 keystore feature anchor |
| `alloy-primitives` | 1 | Primitives shared by both Alloy versions |
| `aes-gcm` | 0.10 | AES-256-GCM encryption for the account DB |
| `sha2` | 0.11 | SHA-256 for KDF and checksum |
| `hex` | 0.4 | Hex encoding |
| `base64` | 0.22 | Base64 for storage blobs |
| `rand` | 0.10 | RNG |
| `ows-lib` | 1.3.2 | Open Wallet Standard vault and signing |
| `keyring` | 3 (`apple-native`, `linux-native`, `windows-native`) | OS keychain for account-DB key material |
| `rpassword` | 7 | Hidden prompts for secrets |
| `rmp-serde` | 1 | MessagePack for some hypersdk payloads |

### Dual Alloy pinning rationale

`hypersdk` 0.2 still re-exports Alloy 1's `PrivateKeySigner`, whose keystore helpers are feature-gated in `alloy-signer-local` 1.x. The crate otherwise imports the signer through hypersdk, so the v1 entries keep Cargo's `keystore` feature unified until hypersdk moves to Alloy 2. `src/lib.rs` ends with `extern crate alloy_signer_local_v1 as _;` as an explicit feature anchor.

## Storage

| Crate | Version | Role |
|-------|---------|------|
| `rusqlite` | 0.38 (`bundled`) | Account DB (SQLite, statically linked) |
| `dirs` | 6 | Config and vault directory resolution |

## Output

| Crate | Version | Role |
|-------|---------|------|
| `tabwriter` | 1 (`ansi_formatting`) | Pretty alignment |
| `tabled` | 0.20 | Bordered table mode |
| `crossterm` | 0.29 (`event-stream`) | Watch-mode alternate screen and key polling |

## Decimals and time

| Crate | Version | Role |
|-------|---------|------|
| `rust_decimal` | 1 (`serde`, `serde-str`) | Decimal type for all financial values |
| `chrono` | 0.4 (`serde`) | Timestamps |
| `strsim` | 0.11 | Fuzzy "did you mean?" suggestions for asset lookup |

## Errors and serialization

| Crate | Version | Role |
|-------|---------|------|
| `anyhow` | 1 | Internal error wrapping |
| `thiserror` | 2 | `CliError` derive |
| `serde`, `serde_json` | 1 / 1 | JSON serialization |

## Dev-only

| Crate | Version | Role |
|-------|---------|------|
| `assert_cmd` | 2 | Integration tests against the compiled binary |
| `predicates` | 3 | Assertion predicates |
| `tempfile` | 3 | Isolated test directories |
| `wiremock` | 0.6 | Mock Hyperliquid HTTP API |

## See also

- [systems/signing-and-wallets](../systems/signing-and-wallets.md) — dual Alloy compatibility details
- [background/design-decisions](../background/design-decisions.md)
