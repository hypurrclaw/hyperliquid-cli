# Hyperliquid CLI

Active contributors: Sayo, wtfsayo

Hyperliquid CLI (`hyperliquid`) is a Rust command-line tool for interacting with the [Hyperliquid DEX](https://hyperliquid.xyz). It covers market data queries, account inspection, order management, fund transfers, staking, vault operations, borrow/lend reserve actions, builder fee approvals, referral workflows, WebSocket subscriptions, and JSON output for agent/script consumption.

The binary is a single-purpose operational interface — not a trading bot, not an SDK replacement. It produces structured output (pretty, table, or JSON) for humans, scripts, and AI agents alike.

## What it does

- **Market data**: perpetual and spot market listings, order books, candles, funding rates, spreads, mid prices, outcome markets
- **Trading**: limit, market, stop-loss, take-profit, stop-limit, and take-limit orders; TWAP; order scaling and batch creation; cancel and modify
- **Position management**: list positions, update leverage, adjust isolated margin
- **Wallet management**: create/import/list/show wallets via OWS (Open Wallet Standard) vault, encrypted local account storage, API/agent wallet approval
- **Funds movement**: spot↔perp transfers, USDC sends, withdrawals, subaccount transfers
- **Staking and DeFi**: validator delegation, staking rewards, vault deposits/withdrawals, borrow/lend reserve rates, and CoreWriter supply/withdraw actions
- **Real-time streaming**: WebSocket subscriptions for trades, order books, candles, fills, order updates, and all mid prices; terminal watch mode
- **Operations tooling**: release update checks, install checksum verification, and repeatable QA sweeps

## Key design principles

- **Three output formats**: pretty (colored terminal), table (bordered), and JSON (stable snake_case keys for agents)
- **Structured exit codes**: 0 success, 1 internal, 2 usage, 10 auth, 11 rate-limit, 12 unavailable, 13 unsupported, 14 stale, 15 partial
- **Signed action safety**: testnet support, `--dry-run` for supported previews, confirmation prompts for prompt-gated mainnet actions, encrypted local key storage
- **Agent-first output**: `--select` for field projection, `--results-only` to strip envelopes, `--max-results` for context control, `schema` command for machine-readable contract metadata
- **Financial precision**: all prices and amounts use `rust_decimal::Decimal`, never floats

## Quick links

- [Architecture](architecture.md) — system components and data flow
- [Getting started](getting-started.md) — install, build, test, run
- [Glossary](glossary.md) — project terminology
- [CLI application](../applications/cli.md) — binary structure and command dispatch
- [Command registry](../systems/command-registry.md) — typed command contracts
- [Configuration](../reference/configuration.md) — env vars, config files, account storage
