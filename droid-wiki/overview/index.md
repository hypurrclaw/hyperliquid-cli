# Hyperliquid CLI

`hyperliquid-cli` is a single-binary Rust command-line tool that gives humans and AI agents a production-grade interface to the [Hyperliquid DEX](https://app.hyperliquid.xyz). Its binary name is `hyperliquid`, and it covers market data, perpetual and spot trading, transfers, staking, vaults, borrow/lend, builder fees, referrals, subaccounts, API/agent wallets, account abstraction, and WebSocket streaming.

The CLI is designed for the agent loop: every data command speaks stable JSON with field projection (`--select`), result caps (`--max-results`), and machine-readable schemas (`schema` subcommand). Mutating commands are gated by `--dry-run` previews, structured confirmation prompts, and a typed registry that records risk, reversibility, raw payload support, and live submission policy.

## What it does

- **Market data** — `mids`, `book`, `candles`, `funding`, `spread`, `status`, `meta`, perps and spot listings, HIP-3 DEX-qualified symbols, outcome markets, asset id decode and search
- **Trading** — limit, market, stop-loss, take-profit, stop-limit, take-limit, IOC, ALO, FOK; TWAP creation and cancellation; scaled and batched orders; position-attached TP/SL; modify, cancel, cancel-all, scheduled cancel-all
- **Position management** — list positions, update leverage, add/remove isolated margin
- **Wallet management** — OWS (Open Wallet Standard) vault as the only wallet backend, encrypted local account storage, BIP-39 mnemonic import, Foundry keystore support, multi-account selection
- **Funds movement** — spot↔perp transfer, USDC send, withdraw, send-asset (cross-context including HIP-3 DEXes), subaccount and spot subaccount transfers
- **Staking, vaults, borrow/lend** — validator delegate/undelegate, deposit/withdraw staking, vault deposit/withdraw, borrow/lend reserve supply/withdraw
- **Operational primitives** — builder fee approvals, referral set/register, API wallet (agent wallet) create/approve/list/revoke, account abstraction inspection/set
- **Streaming** — bounded WebSocket subscriptions (`subscribe trades|orderbook|candles|all-mids|orders|fills`) and terminal watch mode for snapshot commands
- **Tooling** — schema-driven self-description, dry-run previews, self-update from GitHub releases, install.sh checksum verification, structured feedback submission

## Key design principles

- **Three output formats** — `pretty` (ANSI-colored, tabwriter), `table` (bordered), `json` (stable snake_case). Effective default: pretty on a TTY, JSON for non-TTY stdout or when `HYPERLIQUID_AGENT=1`. Explicit `--format` overrides everything.
- **Structured exit codes** — 0 success, 1 internal, 2 usage/configuration, 10 auth, 11 rate-limit, 12 unavailable/timeout, 13 unsupported/asset-not-found, 14 stale, 15 partial. See [exit codes](../reference/exit-codes.md).
- **Decimal-correct** — all prices, sizes, and amounts use `rust_decimal::Decimal`. No floats.
- **Safe by default** — mutating commands surface `--dry-run`, prompt-gate live mainnet actions unless `-y` is passed, and treat all remote API/protocol strings as untrusted (sanitized with an `[untrusted remote data]` label).
- **Agent-first output contract** — `--select`, `--results-only`, `--max-results`, bounded streams (`--max-events`, `--max-ticks`, `--idle-timeout-ms`), and `schema` are first-class.
- **OWS-first wallets** — wallet lifecycle (create, import, list, default) flows through the encrypted OWS vault at `~/.hyperliquid`. Explicit private-key flag/env/config and Foundry keystore signing paths are still supported outside OWS.
- **Catalog-driven schemas** — the embedded `src/command_catalog.json` is the editable source for command metadata; `src/command_registry.rs` loads it and emits schemas through `src/commands/schema.rs`.

## Quick links

- [Architecture](architecture.md) — components, data flows, and Mermaid diagrams
- [Getting started](getting-started.md) — install, build, test, run
- [Glossary](glossary.md) — selector vocabulary and project terminology
- [CLI application](../applications/cli.md) — binary structure and command dispatch
- [Agent output contract](../features/agent-output-contract.md) — `--format`, `--select`, `--results-only`, schema
- [Orders subsystem](../features/orders.md) — order lifecycle and safety hardening
- [Configuration reference](../reference/configuration.md) — env vars, config files, account storage
