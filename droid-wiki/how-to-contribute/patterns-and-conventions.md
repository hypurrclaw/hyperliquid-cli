# Patterns and conventions

## Financial precision

All prices, sizes, and amounts use `rust_decimal::Decimal`. Never introduce floats for financial values. The `rust_decimal` crate is configured with `serde` and `serde-str` features for JSON serialization.

```rust
// Correct
use rust_decimal::Decimal;
let price: Decimal = Decimal::from_str("50000.5")?;

// Wrong — never use f64 for prices
let price: f64 = 50000.5;
```

## Error handling

- Command handlers return `Result<(), anyhow::Error>`
- Domain-specific errors use `CliError` variants from `src/errors.rs`
- `from_anyhow()` recovers typed `CliError` from `anyhow::Error` chains
- Each `CliError` variant maps to a structured exit code (0-15)
- JSON mode prints errors as `{"error": "..."}` on stdout; pretty mode prints to stderr

```rust
// Returning a typed error
return Err(CliError::AssetNotFound {
    asset: "BT".into(),
    suggestions: "BTC, BLUR".into(),
}.into());
```

## Output formatting

Every data command supports `--format pretty|table|json`. Implement `TableData` trait from `src/output/mod.rs`:

```rust
impl TableData for MyOutput {
    fn headers(&self) -> Vec<&str> { vec!["Field1", "Field2"] }
    fn rows(&self) -> Vec<Vec<String>> { ... }
    fn to_json_value(&self) -> serde_json::Value { serde_json::to_value(self).unwrap() }
}
```

The output system handles format routing; commands just return the data struct.

## JSON stability

- All JSON keys are snake_case
- Use `#[serde(rename_all = "snake_case")]` on structs
- Use `#[serde(with = "rust_decimal::serde::str")]` for decimal fields
- Never change JSON key names in a backward-incompatible way

## Selector semantics

Three input classes with different resolution rules:

| Class | Resolution | Use |
|-------|-----------|-----|
| `ACCOUNT_SELECTOR` | Resolves to stored account alias, id, or `0x` address | Signer selection |
| `USER` | Resolves to `0x` address or stored account (aliases resolved to master) | Public lookups |
| `*_ADDRESS` | Must be explicit `0x` address, no alias resolution | Transfer recipients, vaults, validators |

## Response sanitization

All remote API/protocol strings surfaced in errors must go through `labelled_untrusted_text()` from `src/response_sanitization.rs`. This strips ANSI/control sequences and prepends `[untrusted remote data]`.

## Command contract authoring

Every command in `src/command_catalog.json` must declare:

- `lifecycle`: `read_only`, `streaming`, `interactive_local`, `live_mutating`, or `blocked_unsupported`
- `risk`: `none`, `local_state`, `local_secret`, `account_state`, or `funds_movement`
- `dry_run`: `not_supported` or `optional`
- `raw_payload`: `unsupported` or `dry_run_only`
- `confirmation`: `none` or `prompt`

See [`docs/registry-rollout-policy.md`](../../docs/registry-rollout-policy.md) for registry migration stage gates.

## Code organization

- Command modules go in `src/commands/` — one module per domain (orders, wallet, staking, etc.)
- Shared action-signing logic lives in `src/commands/actions.rs`
- The CLI definition and arg parsing stay in `src/main.rs`
- Command routing lives in `src/cli_runtime.rs`
- New systems-level abstractions go in `src/` root files

## Never commit

- Plaintext private keys
- Config files with secrets
- Keystore files
- OWS vault contents
- QA credential artifacts
- `.qa/` directory contents
