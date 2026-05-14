# Agent-first contracts

## Why agents matter

`hyperliquid-cli` is designed to be used by both humans in a terminal and AI agents in automated workflows. The same binary serves both audiences through a set of output contracts that make the CLI predictable and parseable.

## Design decisions

### JSON as the default for non-TTY

When stdout is piped or not a terminal, or when `HYPERLIQUID_AGENT=1` is set, the CLI defaults to JSON output. This means:

```bash
# These produce identical structured output
hyperliquid --format json mids
hyperliquid mids | jq .
HYPERLIQUID_AGENT=1 hyperliquid mids
```

Agents don't need to remember `--format json` — the CLI detects the context.

### Field projection with `--select`

`--select coin,price` filters JSON output to only the named fields. This reduces context window consumption for LLM agents:

```bash
# Full output might have 20+ fields
hyperliquid --format json perps list

# Agent only needs name and max leverage
hyperliquid --format json --select name,max_leverage perps list
```

### Result limiting with `--max-results`

`--max-results <N>` caps the number of items in lists and maps client-side. Combined with `--results-only`, agents get precisely bounded output:

```bash
hyperliquid --format json --results-only --max-results 5 perps list
```

### Structured error envelopes

Every error is a JSON object with an `error` key. Agents can parse errors without special-casing:

```json
{"error":"Authentication required. Run `hyperliquid setup` to configure your wallet."}
```

### Machine-readable schemas

The `schema` command exposes every command's contract as JSON:

```bash
hyperliquid --format json schema orders create
```

This includes the command path, group, lifecycle, risk, auth requirements, dry-run policy, confirmation policy, input argument metadata, and JSON Schema for the command's payload.

### Stable key conventions

- All JSON keys are snake_case
- Financial values use string-encoded decimals (`"50000.5"` not `50000.5`)
- Keys are never renamed in backward-incompatible ways without a major version bump
- `--results-only` strips envelope wrappers to return bare data arrays

### Watch and subscription bounds

Agent-facing streaming output must be bounded:

- `--max-ticks` for snapshot watch mode
- `--max-events` and `--idle-timeout-ms` for WebSocket subscriptions
- `HYPERLIQUID_WATCH_MAX_TICKS` environment variable for default bounds

### Untrusted remote data

All API/protocol text surfaced in errors is sanitized and prefixed with `[untrusted remote data]`. This prevents prompt injection attacks where a malicious API response could contain instructions that an LLM agent might follow.
