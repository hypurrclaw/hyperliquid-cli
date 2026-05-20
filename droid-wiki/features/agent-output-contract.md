# Agent output contract

The CLI is designed for the agent loop: every data command speaks a stable JSON contract with snake_case keys, supports field projection, and produces machine-readable schemas. This page documents the precise contract.

## Format precedence

Effective output format is resolved as:

1. Explicit `--format pretty|table|json`
2. Environment variable `HYPERLIQUID_FORMAT=pretty|table|json`
3. Dynamic default: pretty on a TTY; **JSON for non-TTY stdout or when `HYPERLIQUID_AGENT=1`**

Setting `HYPERLIQUID_AGENT=1` also disables interactive prompts and suppresses the passive update notice.

## Global agent flags

| Flag | Purpose |
|------|---------|
| `--format pretty\|table\|json` | Force output format |
| `--select coin,price` | Comma-separated JSON field projection. Unknown fields are silently omitted on dynamic shapes. |
| `--results-only` | Strip envelope/metadata from JSON. Harmless for commands that already return bare arrays. |
| `--max-results N` | Cap top-level array length for context control |
| `--dry-run` | Preview supported mutations without side effects |
| `--no-update-check` | Skip the passive update notice |

These flags are declared globally in `src/main.rs` and applied uniformly through `src/output/mod.rs`.

## Stable JSON keys

- All keys are **snake_case**.
- Keys are stable across patch releases; minor releases may add new fields but not rename or remove existing ones.
- Numeric financial values are serialized as strings via `rust_decimal::Decimal`'s `serde-str` feature to avoid float drift.
- Optional values serialize as `null` rather than being omitted (so agents can pattern-match on shape).

## Error envelope

In JSON mode, errors go to **stdout** as a single JSON object:

```json
{"error": "Authentication required. Run `hyperliquid setup` to configure your wallet."}
```

The process exits with the structured exit code (see [reference/exit-codes](../reference/exit-codes.md)). Pretty and table modes route errors to stderr.

## Untrusted remote text

Any string returned by the exchange or HTTP layer is wrapped with `[untrusted remote data]` and stripped of ANSI/control sequences before display. Agents should preserve the label in any artifacts they emit.

## Streaming and watch bounds

For `subscribe` and `--watch`, agents must bound the stream:

| Flag | Applies to |
|------|------------|
| `--max-events N` | `subscribe *` |
| `--idle-timeout-ms MS` | `subscribe *` |
| `--max-ticks N` | snapshot `--watch` |
| `HYPERLIQUID_WATCH_MAX_TICKS` | snapshot `--watch` env override |

JSON streams are **NDJSON** — one JSON object per line. `--select` and `--max-results` apply per line; do not assume a single document.

## Recommended agent prelude

```bash
export HYPERLIQUID_AGENT=1
export HYPERLIQUID_FORMAT=json
export HYPERLIQUID_NO_UPDATE_CHECK=1

hyperliquid --select coin,price --max-results 10 mids
hyperliquid --results-only perps list
hyperliquid schema orders create
```

## Schema authority

If schema metadata and README prose disagree, treat schema `input_kind`, `risk`, `dry_run`, and `confirmation` as authoritative. See [schema-discovery](schema-discovery.md).

## Entry points for modification

- To extend the JSON projection grammar, edit the projection helpers in `src/output/mod.rs` and add tests under `tests/output_contracts.rs`.
- To add a new global agent flag, declare it in `src/main.rs`'s `Cli` struct and thread it through `src/cli_runtime.rs`.

See also: [dry-run](dry-run.md), [systems/error-and-output](../systems/error-and-output.md), [SKILL.md](https://github.com/hypurrclaw/hyperliquid-cli/blob/main/SKILL.md).
