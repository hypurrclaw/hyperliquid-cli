# Error and output pipeline

Every command goes through the same output pipeline (`src/output/mod.rs`) and the same error type (`CliError` in `src/errors.rs`). The pipeline guarantees three things: the agent contract holds, exit codes are stable, and remote-supplied strings are sanitized.

## Purpose

- One `OutputFormat` enum that renders any command result as pretty, table, or JSON.
- One `CliError` enum that maps every error variant to a stable exit code.
- One sanitization boundary that labels untrusted remote text.

## Key files

| File | Purpose |
|------|---------|
| `src/output/mod.rs` | `OutputFormat`, ANSI color theme, JSON projection (`--select`), result caps (`--max-results`), error routing |
| `src/errors/` | `CliError` and its variant spec table (`mod.rs`), remote-text classifier (`classify.rs`), transport failure naming (`transport.rs`), recovery commands, JSON envelope, human rendering |
| `src/response_sanitization.rs` | The `Untrusted` type and the `[untrusted remote data]` label |

## Output formats

```rust
pub enum OutputFormat { Pretty, Table, Json }
```

Effective precedence is: explicit `--format`, then `HYPERLIQUID_FORMAT`, then dynamic defaults (pretty on a TTY, JSON for non-TTY stdout or when `HYPERLIQUID_AGENT=1`). Pretty output may use ANSI color. Table and JSON output must remain uncolored. Agents should always request JSON.

The color theme:

| Color | Meaning |
|-------|---------|
| Cyan  | Headers |
| Green | Positive (profit, gains) |
| Red   | Negative (loss, declines) |
| Gray  | Muted/secondary text |
| Yellow| Warnings |

## JSON projection and capping

- `--select coin,price` filters JSON output to those fields. Unknown fields are omitted silently when the output shape is dynamic.
- `--results-only` strips envelope metadata; harmless for commands that already return bare arrays/objects.
- `--max-results N` caps the top-level array length for context control.
- For streaming commands, JSON is emitted as NDJSON (one JSON object per line). `--select` and `--max-results` apply per line.

See [features/agent-output-contract](../features/agent-output-contract.md) for the full surface.

## Error routing

```mermaid
graph LR
    Cmd[Command handler] -->|Result| Map[errors::map_*]
    Map -->|CliError| Format{output format}
    Format -->|pretty| Stderr1["stderr: red prefix + message"]
    Format -->|table| Stderr2["stderr: no ANSI"]
    Format -->|json| Stdout["stdout: {\"error\": \"...\"}"]
    Map -->|exit_code()| Exit[process::exit]
```

JSON-mode errors print to **stdout** as a single JSON object so agents that capture stdout never miss them. Pretty and table errors print to stderr.

`Completed in X.XXs` is printed to stderr after a successful command so it never pollutes JSON output.

## CliError variants

| Variant | Exit | When |
|---------|------|------|
| `Internal(anyhow::Error)` | 1 | Unexpected anyhow error |
| `Configuration(String)` | 2 | Invalid config/env |
| (clap usage)              | 2 | Argument parse errors |
| `AuthRequired`            | 10 | No wallet configured |
| `InvalidAuth(String)`     | 10 | Bad key format / expired creds |
| `AuthRejected(Untrusted)` | 10 | Remote service rejected the credentials |
| `OwsWalletNotFound`       | 10 | OWS selector unknown |
| `OwsNoChainAccount`       | 10 | OWS wallet has no Hyperliquid/EVM account |
| `RateLimited { .. }`      | 11 | API rate limited |
| `Unavailable(String)`     | 12 | Network/API unreachable |
| `Timeout(String)`         | 12 | Exceeded a bound |
| `Unsupported(String)`     | 13 | Bad asset/DEX/parameter |
| `ConfirmationRequired`    | 13 | Can't prompt for a required confirmation |
| `AssetNotFound { asset, suggestions }` | 13 | Unknown asset with close matches |
| `AssetNotFoundNoSuggestion`     | 13 | Unknown asset with no matches |
| `PartialResults(String)`  | 15 | Some items failed in a batch |

The full table including precise messages lives in [reference/exit-codes](../reference/exit-codes.md).

## Untrusted remote data

`src/response_sanitization.rs` exposes `Untrusted::from_remote(raw)`, which strips ANSI/control sequences; its `Display` prefixes the text with `[untrusted remote data]`. Error variants that carry remote text (`ExchangeRejected`, `AuthRejected`, `ApiStatus.remote`) take `Untrusted`, so unlabelled remote text does not type-check; `CliError::internal_with_remote` builds `Internal` errors that quote it. Agents that parse error output should preserve the label intact in artifacts and summaries.

## Entry points for modification

- To add a new error variant: add a `CliError` arm in `src/errors/mod.rs`, give it a thiserror message, and add one row in `CliError::spec()` (exit code, category, reason, retryability). Add a snapshot test in `tests/error_exit_codes.rs`.
- To teach the CLI a new rate-limit, auth or user-state phrasing from the exchange: edit the phrase tables at the top of `src/errors/classify.rs`.
- To change the JSON projection logic: see the projection helpers near the top of `src/output/mod.rs` and add coverage in `tests/output_contracts.rs`.
- To add a new untrusted-string surface: route it through `Untrusted::from_remote` and add a test in `tests/security_contracts.rs`.

See also: [systems/input-hardening](input-hardening.md), [features/agent-output-contract](../features/agent-output-contract.md).
