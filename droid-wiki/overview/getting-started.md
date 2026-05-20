# Getting started

This page walks through prerequisites, install paths, the standard development loop, and the first commands you should run as a human or an agent.

## Prerequisites

- **Rust 1.93+** (`rust-version = "1.93"` in `Cargo.toml`)
- A POSIX shell for the install script (macOS or Linux). Windows users build from source.
- For live trading: a funded Hyperliquid account or a master account that can approve an API/agent wallet.

## Install

### Released binary

```bash
curl -fsSLO https://raw.githubusercontent.com/hypurrclaw/hyperliquid-cli/main/install.sh
sh install.sh
hyperliquid --version
```

The installer downloads the asset for your platform, verifies a SHA-256 checksum, and copies the binary to `~/.local/bin/hyperliquid`. Override the source repo, pinned version, or install directory with `HYPERLIQUID_CLI_REPO`, `HYPERLIQUID_CLI_VERSION`, and `BIN_DIR` env vars.

### From source

```bash
cargo install --path . --bin hyperliquid
# or, during development:
cargo build --release
target/release/hyperliquid --version
```

### Self-update

```bash
hyperliquid update
```

The update path downloads the asset, verifies it against the published SHA-256, and atomically swaps the running binary. See [systems/update-and-release](../systems/update-and-release.md).

## Development loop

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

When the `Taskfile.yml` `task` runner is available:

```bash
task bind            # build release and symlink ~/.local/bin/hyperliquid
task qa:matrix       # broad dry-run sweep across the command surface
task contracts       # contract characterization tests
task ci              # fmt + clippy + test + contracts + qa
```

If macOS scratch space is constrained, pin Cargo's tmp dir into the repo:

```bash
mkdir -p .tmp
TMPDIR="$PWD/.tmp" CARGO_TARGET_DIR="$PWD/target" task qa:matrix
```

## First commands

### Read-only

```bash
hyperliquid status              # API health + rate-limit context
hyperliquid mids                # all mid prices
hyperliquid book BTC            # L2 order book for BTC
hyperliquid perps list          # all perpetual markets
hyperliquid --format json --select coin,price --max-results 5 mids
```

### Self-description

```bash
hyperliquid --format json schema                     # full command catalog
hyperliquid --format json schema orders create       # one command's contract
hyperliquid orders create --help                     # human help text
```

### Set up a wallet

```bash
hyperliquid setup           # interactive wizard
hyperliquid setup -y        # unattended; accepts packaged defaults
hyperliquid wallet create   # generate a fresh OWS-managed wallet
hyperliquid wallet import   # paste a private key at the hidden prompt
hyperliquid wallet list     # list wallets in the OWS vault
hyperliquid wallet address  # show current default address
```

Wallets live in the OWS vault at `~/.hyperliquid` by default (override with `HYPERLIQUID_OWS_VAULT_PATH`). Secrets entered at hidden prompts are never echoed, logged, or printed.

### Plan an order before sending

```bash
# Always dry-run mutating commands first
hyperliquid --dry-run orders create \
  --coin BTC --side buy --price 50000 --size 0.001 --tif alo

# Network selector: testnet is one flag away
hyperliquid --testnet orders create \
  --coin BTC --side buy --price 50000 --size 0.001 --tif alo
```

### Hand a bounded wallet to an agent

```bash
hyperliquid api-wallet create --name trading-agent
# The newly generated agent private key is printed exactly once. Store it securely.
```

API wallets (also called agent wallets) can sign trading actions for the master account but cannot withdraw. See [features/api-wallets](../features/api-wallets.md).

## Environment variables

| Variable | Purpose |
|----------|---------|
| `HYPERLIQUID_AGENT=1` | Default to JSON output and disable interactive prompts |
| `HYPERLIQUID_FORMAT=json\|table\|pretty` | Default output format |
| `HYPERLIQUID_NETWORK=mainnet\|testnet` | Default network |
| `HYPERLIQUID_PRIVATE_KEY` | Signer private key (least secure; prefer OWS) |
| `HYPERLIQUID_OWS_VAULT_PATH` | Override OWS vault location |
| `OWS_PASSPHRASE` | Vault unlock passphrase for unattended automation |
| `HYPERLIQUID_NO_UPDATE_CHECK=1` | Disable passive release notices |
| `HYPERLIQUID_WATCH_MAX_TICKS` | Cap snapshot watch loops |

The full env-var list is in [reference/configuration](../reference/configuration.md).
