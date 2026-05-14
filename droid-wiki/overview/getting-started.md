# Getting started

## Prerequisites

- Rust 1.93+ (the project uses edition 2024)
- Cargo

## Install

### From source

```bash
cargo install --path . --bin hyperliquid
hyperliquid --help
```

### From GitHub Releases

```bash
curl -fsSLO https://raw.githubusercontent.com/wtfsayo/hyperliquid-cli/main/install.sh
sh install.sh
hyperliquid --version
```

The installer downloads the release archive, verifies its SHA-256 checksum, and copies `hyperliquid` into `~/.local/bin`.

## Build and verify

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## First commands

Read-only commands work without authentication:

```bash
hyperliquid status
hyperliquid perps list
hyperliquid book BTC
hyperliquid mids
```

JSON output for scripts and agents:

```bash
hyperliquid --format json mids
hyperliquid --format json --select coin,price mids
hyperliquid --format json --results-only perps list
```

## Configure a signer

Run the setup wizard to create or import a wallet:

```bash
hyperliquid setup
```

This creates an OWS wallet at `~/.hyperliquid`, saves local config at the platform-appropriate config directory, and verifies the API connection.

## Testnet

Use `--testnet` to route all API calls and signed actions to Hyperliquid testnet:

```bash
hyperliquid --testnet status
hyperliquid --testnet orders open
```

## Dry run

Preview mutating commands without side effects:

```bash
hyperliquid --dry-run orders create --coin BTC --side buy --price 50000 --size 0.001
hyperliquid --format json --dry-run vault deposit --vault 0x... --amount 5
```

## Agent usage

Set `HYPERLIQUID_AGENT=1` or pipe to a non-TTY stdout to default to JSON output:

```bash
HYPERLIQUID_AGENT=1 hyperliquid perps list
hyperliquid --format json schema orders create | jq .
```

## Taskfile workflow

An optional Taskfile provides convenience commands:

```bash
task build        # release build
task test         # test suite
task clippy       # lint with warnings denied
task fmt          # check formatting
task bind         # link ~/.local/bin/hyperliquid to release binary
task qa:matrix    # broad QA command matrix
task ci           # all quality gates
```
