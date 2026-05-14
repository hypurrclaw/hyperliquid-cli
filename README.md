# Hyperliquid CLI

[![Crates.io](https://img.shields.io/badge/crates.io-v0.1.0-orange.svg)](https://crates.io/crates/hyperliquid-cli)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.93%2B-blue.svg)](https://www.rust-lang.org)
[![Built on hypersdk](https://img.shields.io/badge/built%20on-hypersdk-blueviolet.svg)](https://github.com/infinitefield/hypersdk)

**Give your AI agent a CLI and wallet to trade Hyperliquid.**

`hyperliquid` is a single binary that hands your personal agent — OpenClaw, Hermes, PicoClaw, Claude, Codex, or any LLM that can shell out — a production-grade command line and encrypted wallet for [Hyperliquid](https://app.hyperliquid.xyz). Markets, orders, transfers, staking, vaults, borrow/lend, builder fees, WebSocket streams: every action surfaces as a clean JSON command an agent can read, reason about, and execute.

Drop it into your agent's tool belt and it can check prices, place orders, manage positions, and stream the book — all through one binary, with dry-runs, schemas, and safety gates built in.

## Why hyperliquid-cli

- **Agent-first.** Built for the agent loop. Every command speaks JSON with `--format json`, field projection (`--select`), result caps (`--max-results`), and machine-readable `schema` output. `HYPERLIQUID_AGENT=1` (or non-TTY stdout) defaults to JSON automatically. Your agent reads stable snake_case keys, structured error objects, and well-defined exit codes — no scraping, no guessing.
- **Wallet for your agent.** Create an API wallet (aka agent wallet) that can trade but never withdraw. Hand it to OpenClaw, Hermes, Claude, or any automation and it operates with bounded authority. Wallets live in the encrypted OWS vault — secrets never touch stdout, logs, or shell history.
- **One tool, broad protocol coverage.** Markets, perps, spot, HIP-3 DEXes, orders, transfers, subaccounts, vaults, staking, borrow/lend, builder fees, referrals, account abstraction, and WebSocket subscriptions — all behind one binary.
- **Safe by default.** Prompt-gated mainnet mutations require confirmation. `--dry-run` previews every side effect before it happens. Testnet is one flag away. API wallets are withdraw-proof by protocol design.
- **Decimal-correct.** Every price, size, and amount uses `rust_decimal`. No floats, no surprise rounding.
- **Single static binary.** Built in Rust on top of [`hypersdk`](https://github.com/infinitefield/hypersdk). Install in seconds, ship in containers, run anywhere.

## Install

```bash
curl -fsSLO https://raw.githubusercontent.com/hypurrclaw/hyperliquid-cli/main/install.sh
sh install.sh
hyperliquid --version
```

The installer verifies a SHA-256 checksum before copying the binary to `~/.local/bin`. Override defaults with `HYPERLIQUID_CLI_REPO`, `HYPERLIQUID_CLI_VERSION`, and `BIN_DIR`.

From source:

```bash
cargo install --path . --bin hyperliquid
```

Requires Rust 1.93+.

## Quick start

```bash
# Read the market
hyperliquid status
hyperliquid mids
hyperliquid book BTC

# Read it as JSON, projected, capped
hyperliquid --format json --select coin,price --max-results 10 mids

# Configure a signer
hyperliquid setup

# Plan an order without sending it
hyperliquid --dry-run orders create --coin BTC --side buy --price 50000 --size 0.001

# Send it on testnet
hyperliquid --testnet orders create --coin BTC --side buy --price 50000 --size 0.001

# Ask the CLI to describe its own contract
hyperliquid --format json schema orders create
```

## Wallet Setup

`hyperliquid` uses the **Open Wallet Standard (OWS)** as its only wallet backend. Wallets live in an encrypted vault on disk (`~/.hyperliquid` by default, overridable via `HYPERLIQUID_OWS_VAULT_PATH`). Secrets are entered interactively at hidden prompts — never echoed, logged, or printed.

### Guided setup

The fastest path for a new operator:

```bash
hyperliquid setup
```

The wizard walks you through creating or importing a wallet, choosing a default network, persisting any packaged builder fee / referral defaults, and optionally approving the default builder fee cap. For unattended environments, accept all defaults non-interactively:

```bash
hyperliquid setup -y
```

### Create or import a wallet directly

```bash
hyperliquid wallet create               # generate a new wallet
hyperliquid wallet import               # paste a private key (hidden prompt)
hyperliquid wallet import-mnemonic      # paste a BIP-39 mnemonic (hidden prompt)
```

The newly created or imported wallet becomes the default signer.

### Manage multiple wallets

```bash
hyperliquid wallet list                 # all wallets in the OWS vault
hyperliquid wallet show                 # current default
hyperliquid wallet address              # just the address
hyperliquid wallet rename <SELECTOR> <NEW_NAME>
hyperliquid wallet export <SELECTOR>    # reveal secret (with confirmation)
hyperliquid wallet delete <SELECTOR>
```

Select a specific wallet per command without changing the default:

```bash
hyperliquid --account alice orders open
hyperliquid --ows-signer 0xabc... positions list
```

### API / agent wallets

API wallets (aka agent wallets) are delegated Hyperliquid trading keys approved by a master account. They can trade but cannot withdraw — ideal for handing a bounded signer to an automation or AI agent:

```bash
hyperliquid api-wallet create --name trading-agent
hyperliquid api-wallet list <MASTER_ADDRESS>
hyperliquid api-wallet revoke --name trading-agent
```

### Alternative signer sources

If you prefer not to use the OWS vault for a single command, pass the signer explicitly:

```bash
hyperliquid --keystore ~/.foundry/keystores/my-wallet ...
hyperliquid --private-key 0x... ...     # avoid in shared shells / history
```

Or set environment variables:

```bash
export HYPERLIQUID_PRIVATE_KEY=0x...
export OWS_PASSPHRASE=...               # unlock encrypted OWS wallet
```

### Safety rules

- **Never** commit private keys, mnemonics, keystore files, OWS secrets, or config databases.
- Prefer OWS wallets or keystores over raw `--private-key` flags in shared environments.
- Use API wallets when delegating trading to scripts or agents — they can't withdraw funds.
- `--testnet` is one flag away whenever you want to rehearse a flow before going live.

## Output formats

Every data command exposes the same automation surface:

| Flag | Purpose |
| --- | --- |
| `--format pretty\|table\|json` | Human or machine output. |
| `--select <FIELDS>` | Project JSON to comma-separated fields. |
| `--results-only` | Strip envelopes, return only data. |
| `--max-results <N>` | Cap top-level list/map size client-side. |
| `--dry-run` | Validate and preview supported mutations. |
| `--payload-json` / `--payload-file` | Feed raw JSON into dry-runs. |

Set `HYPERLIQUID_AGENT=1` (or run non-TTY) and one-shot commands default to JSON automatically. Errors are stable objects:

```json
{"error": "Authentication required. Run `hyperliquid setup` to configure your wallet."}
```

Every mutating command ships a `schema` description an agent can read before acting — including risk class, confirmation requirements, and dry-run support.

See [`SKILL.md`](SKILL.md) for the agent operating guide.

## Terminology and address selectors

| Domain | Examples |
| --- | --- |
| Local signing account | An OWS wallet managed by `account add`, `account ls`, `account set-default`, and related commands. |
| Selected signer | The key used to sign authenticated actions, chosen from flags, environment/config, global `--account`, or the OWS selector. |
| Protocol user address | A public Hyperliquid user address used for info queries such as fills, portfolio, fees, or order status. |
| Master account | The protocol owner account that can approve API wallets and own subaccounts. |
| API wallet / agent wallet | A delegated Hyperliquid trading key approved by a master account. It can trade for the master account but cannot withdraw. |
| OWS signer | An Open Wallet Standard signer source selected with `--ows-signer`. |
| Subaccount | A protocol subaccount controlled by a master account. |
| Protocol address | A literal on-chain/protocol address for a recipient, vault, validator, builder, or similar object. |

Address-like command inputs fall into three safety classes:

| Class | Accepted values | Used for |
| --- | --- | --- |
| `ACCOUNT_SELECTOR` | Stored account alias, stored account id, or `0x` address | Selecting a signer with `--account` or managing OWS wallet records. |
| `USER` | `0x` user address, or a documented safe stored-account selector | Public lookups such as `account portfolio`, `orders status --user`, or fee queries. Stored API/agent wallet selectors resolve to their approving master account for these reads. |
| `*_ADDRESS` | Explicit `0x` protocol address only | Transfer recipients, vaults, validators, builders, and other protocol objects. Local aliases are not substituted for these fields. |

For agents, `hyperliquid --format json schema ...` tool schemas are the authoritative source for input semantics when they conflict with examples or prose.

Canonical top-level aliases accepted by the CLI:

- `api-wallets` -> `api-wallet`
- `subaccounts` -> `subaccount`
- `transfers` -> `transfer`
- `vaults` -> `vault`

## Command reference

### Global options

| Option | Description |
| --- | --- |
| `-f, --format pretty\|table\|json` | Output format. Defaults to `pretty`. |
| `--private-key <PRIVATE_KEY>` | Sign with a raw private key. Overrides environment and config. |
| `--keystore <PATH>` | Sign with a Foundry-compatible keystore file. |
| `--keystore-password <PASSWORD>` | Keystore password. Prefer safer secret sources for humans. |
| `--account <SELECTOR>` | Stored wallet alias, id, or address to use as the signer. Conflicts with other signer flags. |
| `--ows-signer <SELECTOR>` | OWS wallet selector (name or id). Accepts `0x` addresses for identity/dry-run plumbing. Alias: `--wallet`. Conflicts with local signer flags. |
| `--testnet` | Route API calls and signed actions to Hyperliquid testnet. |
| `--select <FIELDS>` | Project JSON output to comma-separated fields. |
| `--results-only` | Strip common JSON envelopes and return only data. |
| `--max-results <N>` | Limit top-level list/map results client-side. |
| `--dry-run` | Validate and preview mutating commands without side effects. |
| `--payload-json <JSON>` / `--payload-file <PATH\|->` | Provide raw JSON payload context for mutating dry-runs. |

### Market data

| Command | Description |
| --- | --- |
| `perps list [--dex <DEX>]` | List perpetual markets. |
| `perps get <COIN> [--dex <DEX>]` | Show one perpetual market. |
| `spot list` | List spot markets. |
| `spot get <PAIR>` | Show one spot pair, for example `PURR/USDC`. |
| `outcomes list [--limit <N>]` | List active outcome market sides from `outcomeMeta`. |
| `outcomes get #<ENCODING>` / `outcomes get +<ENCODING>` | Show outcome side metadata and derived asset ID. |
| `book <COIN_OR_PAIR> [-w]` | Show L2 order book snapshot or watch updates. |
| `mids [-w]` | Show all mid prices. |
| `candles <COIN> [--interval <INTERVAL>] [--limit <N>] [-w]` | Show candle history. |
| `spread <COIN>` | Show bid, ask, and spread. |
| `funding <COIN>` | Show current and predicted funding. |
| `meta` | Show raw exchange metadata. |
| `status` | Show API health and rate-limit context. |

### Account, wallet, and setup

| Command | Description |
| --- | --- |
| `setup` | Run the guided first-time setup wizard. |
| `wallet create` | Create and store a new wallet. |
| `wallet import [PRIVATE_KEY]` | Import a wallet. Omit the key to use a hidden prompt. |
| `wallet show` | Show current wallet metadata. |
| `wallet address` | Print only the configured wallet address. |
| `wallet import-mnemonic [MNEMONIC]` | Import a wallet from a BIP-39 mnemonic phrase. |
| `wallet list` | List all wallets in the OWS vault. |
| `wallet rename <SELECTOR> <NEW_NAME>` | Rename a wallet. |
| `wallet delete <SELECTOR>` | Delete a wallet. Prompts unless `-y`. |
| `wallet export <SELECTOR>` | Export wallet secret (mnemonic or private key). |
| `wallet reset` | Remove wallet configuration after confirmation. |
| `account fees <ADDRESS>` | Query fee schedule and volume context. |
| `account fills <ADDRESS> [--start <TIME>] [--end <TIME>] [--aggregate-by-time]` | Query public fill history, optionally by time. |
| `account ledger <ADDRESS> --start <TIME> [--end <TIME>]` | Query deposits, withdrawals, transfers, and other non-funding ledger updates. |
| `account funding <ADDRESS> --start <TIME> [--end <TIME>]` | Query user funding payment history. |
| `account orders <ADDRESS>` | Query public open orders. |
| `account portfolio <ADDRESS>` | Query public portfolio summary. |
| `account portfolio-history <ADDRESS>` | Query frontend portfolio graph/history data. |
| `account rate-limit <ADDRESS>` | Query user rate-limit context. |
| `account subaccounts <ADDRESS>` | Query public subaccounts. |
| `account twap-history <ADDRESS>` | Query user TWAP order history. |
| `account twap-fills <ADDRESS> [--start <TIME>] [--end <TIME>]` | Query user TWAP slice fills. |
| `account abstraction [ADDRESS]` | Read account abstraction mode for an address, or the selected account when `ADDRESS` is omitted. |
| `account abstraction set --mode disabled\|unified-account\|portfolio-margin` | Set account abstraction for the configured signer; prompts unless `-y`. |
| `subaccount list <ADDRESS>` | Query public subaccounts for a master address. |
| `subaccount create --name <NAME>` | Create a subaccount signed by the master account. |
| `account add` / `account ls` / `account set-default` / `account remove` | Manage stored wallets. |
| `api-wallet create --name <NAME> [--expires-in <DURATION>]` | Generate and approve a Hyperliquid API/agent wallet. |
| `api-wallet approve --agent-address <ADDRESS>` | Approve an existing or generated agent wallet address. |
| `api-wallet list [ACCOUNT]` | List API wallets approved by a master account. |
| `api-wallet revoke --name <NAME>` | Replace a named API wallet with a short-lived throwaway agent. |

API wallets can sign trading actions for the approving master account, but they cannot withdraw. Use the master or subaccount address for info queries; the CLI stores generated API wallets as `agent-wallet` records with their master address so stored-agent reads resolve to the master account. When `api-wallet create` generates a local agent keypair, it prints the private key once before submitting `approveAgent` for that address.

### Trading and transfers

| Command | Description |
| --- | --- |
| `orders open [-w]` | List open orders. |
| `orders history` | List order history. |
| `orders status --user <ADDRESS> --oid <OID>` | Query public order status. |
| `orders create --coin <COIN> --side buy\|sell ... [--reduce-only] [--on-behalf-of <ACCOUNT_SELECTOR>]` | Create limit, market, stop-loss, take-profit, stop-limit, or take-limit orders. `--on-behalf-of` is an acting-account selector used as `vaultAddress`. |
| `orders scale --coin <COIN> --side buy\|sell --start-price <PX> --end-price <PX> --total-size <SIZE> --orders <N>` | Create an evenly spaced batch of limit orders. |
| `orders batch-create --orders-file <PATH>` | Create a batch of limit orders from JSON. |
| `orders create --coin <COIN> --side buy\|sell --take-profit <PX> [--stop-loss <PX>] --grouping normal-tpsl ...` | Create a parent order with fixed-size TP/SL children. |
| `orders tpsl --coin <COIN> --take-profit <PX> [--stop-loss <PX>] --grouping position-tpsl` | Create TP/SL orders attached to the current position. |
| `orders cancel <OID>` / `orders cancel --cloid <CLOID>` | Cancel by order ID or client order ID. |
| `orders cancel-all [--coin <COIN>] [-y]` | Cancel all open orders, optionally filtered by coin. |
| `orders modify <OID> [--price <PRICE>] [--size <SIZE>]` | Modify an existing order. |
| `orders twap-create --coin <COIN> --side buy\|sell --size <SIZE> --duration <SECONDS>` | Create a TWAP order. |
| `orders twap-cancel <TWAP_ID> --coin <COIN>` | Cancel a TWAP order. |
| `orders schedule-cancel --in <DURATION>` | Configure a dead man's switch. |
| `positions list [-w]` | List open positions. |
| `positions update-leverage --coin <COIN> --leverage <N>` | Update leverage. |
| `positions update-margin --coin <COIN> --amount <AMOUNT>` | Add or remove isolated margin. |
| `transfer spot-to-perp --amount <USDC>` | Move USDC from spot to perp. |
| `transfer perp-to-spot --amount <USDC>` | Move USDC from perp to spot. |
| `transfer send --to <ADDRESS> --amount <USDC>` | Send USDC to another address. |
| `transfer spot-send --to <ADDRESS> --token <TOKEN> --amount <AMOUNT>` | Send a spot token to another address. |
| `transfer send-asset --to <ADDRESS> --source perp\|spot\|dex:<DEX> --dest perp\|spot\|dex:<DEX> --token <TOKEN> --amount <AMOUNT>` | Send an asset between accounts, spot, perp, or DEX contexts. |
| `transfer withdraw --to <ADDRESS> --amount <USDC>` | Withdraw USDC to Arbitrum. |
| `subaccount transfer --subaccount <ACCOUNT_SELECTOR> --amount <USDC> --direction deposit\|withdraw` | Move USDC to or from a subaccount. The subaccount field is an acting-account selector, not a generic transfer recipient. |
| `subaccount spot-transfer --subaccount <ACCOUNT_SELECTOR> --token <TOKEN> --amount <AMOUNT> --direction deposit\|withdraw` | Move a spot token to or from a subaccount. The subaccount field is an acting-account selector, not a generic transfer recipient. |

`api-wallets` is accepted as an alias for `api-wallet`.
`subaccounts` is accepted as an alias for `subaccount`.
`transfers` is accepted as an alias for `transfer`.

Time-bounded account history commands accept RFC3339 timestamps and epoch milliseconds. The CLI sends milliseconds to Hyperliquid, for example:

```bash
hyperliquid --format json account fills 0x0000000000000000000000000000000000000000 --start 2026-05-01T00:00:00Z --end 2026-05-02T00:00:00Z --aggregate-by-time
hyperliquid --format json --select time,delta account ledger 0x0000000000000000000000000000000000000000 --start 1777593600000 --end 1777680000000
hyperliquid --format json account portfolio-history 0x0000000000000000000000000000000000000000
hyperliquid --format json orders status --user 0x0000000000000000000000000000000000000000 --oid 123
hyperliquid --format json --dry-run orders scale --coin BTC --side buy --start-price 80000 --end-price 90000 --total-size 0.005 --orders 5
hyperliquid --format json --dry-run orders batch-create --orders-file tests/fixtures/orders_batch_create.json
hyperliquid --format json --dry-run account abstraction set --mode disabled
hyperliquid --format json outcomes get '#10'
```

Hyperliquid's documented history endpoints return bounded windows, commonly capped around 500 or 2000 rows depending on endpoint. For exports, query adjacent non-overlapping windows and keep `--format json`; use global `--max-results` only to trim local CLI output for inspection.

Outcome market notation (`#N` spot coin and `+N` token name) is available for discovery through `outcomes list` and `outcomes get`. `orders create --coin '#N' --dry-run` emits a stable outcome order preview with the encoded asset id, and live limit-order submission supports verified outcome notation when a signer is configured. Use `--dry-run` first to inspect the encoded asset id and signed-action preview before placing a live outcome order.

### Advanced commands

| Command | Description |
| --- | --- |
| `staking summary <ADDRESS>` / `staking validators` / `staking rewards <ADDRESS>` / `staking history <ADDRESS>` | Read staking state and history. |
| `staking delegate` / `staking undelegate` / `staking deposit` / `staking withdraw` / `staking claim-rewards` | Submit staking actions. |
| `staking link initiate --user <ADDRESS>` / `staking link finalize --user <ADDRESS>` | Link trading and staking accounts for fee discount attribution. Dry-runs include permanence/control warnings; live commands require confirmation or `--yes`. |
| `vault list [--kind protocol|user|normal|child|parent] [--user <ADDRESS>]` / `vault search <QUERY> [--user <ADDRESS>]` / `vault get <ADDRESS>` / `vault positions <ADDRESS>` | Discover and query vault state. `--user` includes user deposit context when the API returns it. |
| `vault deposit` / `vault withdraw` | Submit vault transfers. |
| `borrowlend rates` / `borrowlend get <TOKEN>` / `borrowlend user <ADDRESS>` | Query borrow/lend markets. |
| `borrowlend supply <TOKEN> --amount <AMOUNT>` / `borrowlend withdraw <TOKEN> --amount <AMOUNT|--max>` | Submit verified wallet-signed exchange `borrowLend` supply/withdraw actions; use `--dry-run` to inspect the action first. |
| `builder max-fee --user <ADDRESS> --builder <ADDRESS>` | Query a user's approved max builder fee. |
| `builder approved --user <ADDRESS>` | List all builders approved by a user with fee caps. |
| `builder approve --builder <ADDRESS> --max-fee-rate <PERCENT>` | Approve or revoke a builder fee cap for the configured master signer. |
| `prio status` / `prio bid` | Query or bid in the gossip priority auction. |
| `referral register <CODE>` / `referral set [CODE]` / `referral status` | Register your own referral code, set a referrer, or inspect referral state. |
| `feedback --scenario-json <JSON>` / `feedback --scenario-file <PATH\|->` | Send structured CLI feedback as a scenario JSON object to the configured feedback endpoint; include `agent_address`, `signer_address`, or `wallet_address` in the scenario for rate-limit attribution, and use `--url` to override defaults. |
| `schema [COMMAND...]` | Show machine-readable command schemas for agents. |
| `subscribe trades\|orderbook\|candles\|all-mids\|order-updates\|fills` | Stream WebSocket events. |

`vaults` is accepted as an alias for `vault`.

Builder approvals use percent strings such as `0.001%`; `0%` revokes by setting the approved max fee to zero. The approval action must be signed by the master account, not an API wallet. `orders create` accepts paired `--builder <ADDRESS> --builder-fee-rate <PERCENT>` flags and includes the official `builder: { b, f }` wire object in the signed order action; perp builder fees are capped at `0.1%`, spot builder fees at `1%`. Forks/distributions can bake in default order builder parameters at build time:

```bash
HYPERLIQUID_DEFAULT_BUILDER_ADDRESS=0x... \
HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE=0.001% \
cargo build --release --bin hyperliquid
```

Runtime env vars with the same names override the build-time defaults. During `hyperliquid setup`, build-time defaults are shown as the suggested builder/fee and pressing Enter accepts them; runtime env vars become the suggestion when present. `HYPERLIQUID_DEFAULT_REFERRAL_CODE` follows the same build-time/runtime pattern for setup and `referral set` defaults. The release workflow passes these values from GitHub Actions repository variables of the same names. If neither env nor config supplies defaults, users can still pass `--builder` and `--builder-fee-rate` per order.

Vault discovery can feed vault detail and transfer dry-runs without address rewriting:

```bash
hyperliquid --format json vault list --kind protocol --limit 5 --sort tvl
hyperliquid --format json vault get 0x...
hyperliquid --format json --dry-run vault deposit --vault 0x... --amount 5
```

Full reference: `hyperliquid --help` and `hyperliquid <command> --help`, plus the guides in [`docs/`](docs/).

## Testnet

Use `--testnet` to rehearse reads, dry-runs, and approved live testnet flows before touching mainnet. Testnet uses the same command surface with the testnet API endpoint and separate account state.

## Safety Model

The CLI is designed to make side effects visible:

- Read-only commands never touch a private key.
- Signing only happens through explicit `--account`, `--ows-signer`, `--keystore`, `--private-key`, or stored OWS wallets.
- `--testnet` cleanly routes API calls and signed actions to Hyperliquid testnet.
- `--dry-run` validates and previews any supported mutation without sending it.
- Prompt-gated live mainnet mutations and destructive local secret operations require confirmation unless `-y` / `--yes` is supplied where supported.
- Transfer recipients and protocol object addresses must be explicit `0x` addresses — local aliases are never silently substituted.

## Exit Codes

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `1` | Internal error |
| `2` | Usage, validation, or configuration error |
| `10` | Missing or invalid authentication |
| `11` | Rate limited |
| `12` | API or network unavailable |
| `13` | Unsupported input, invalid asset, or unknown DEX |
| `14` | Stale data |
| `15` | Partial results |

## Configuration

Resolution order: CLI flags → environment variables → `~/.config/hyperliquid/config.json`.

| Variable | Purpose |
| --- | --- |
| `HYPERLIQUID_PRIVATE_KEY` | Private key for signing (prefer OWS or keystore). |
| `HYPERLIQUID_NETWORK` | `mainnet` or `testnet`. |
| `HYPERLIQUID_FORMAT` | Explicit default output format (`pretty`, `table`, or `json`) before agent/non-TTY fallback. |
| `HYPERLIQUID_AGENT` | Set to `1` to force agent defaults. |
| `HYPERLIQUID_WATCH_MAX_TICKS` | Default tick limit for snapshot watch mode. |
| `HYPERLIQUID_SUBSCRIBE_MAX_EVENTS` | Default event limit for WebSocket subscribe commands in agent contexts. |
| `OWS_PASSPHRASE` | Passphrase to unlock an encrypted OWS wallet. |
| `HYPERLIQUID_OWS_VAULT_PATH` | Override the OWS vault path (default `~/.hyperliquid`). |

## Develop

To embed a default endpoint for `hyperliquid feedback`, set `HYPERLIQUID_FEEDBACK_URL` in the build environment. At runtime, `--url` takes precedence, then runtime `HYPERLIQUID_FEEDBACK_URL`, then the embedded build-time default.

```bash
HYPERLIQUID_FEEDBACK_URL="https://<worker-subdomain>/feedback" cargo build --release
```

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

Optional repeatable QA via Taskfile:

```bash
task bind
task qa:matrix
```

Conventions, testing rules, and the agent-first output contract live in [`AGENTS.md`](AGENTS.md) and [`CONTRIBUTING.md`](CONTRIBUTING.md).

## When You Want Something Else

- Building a full application or trading system? Use [`hypersdk`](https://github.com/infinitefield/hypersdk) directly.
- Need long-running strategy execution, backtesting, or hosted bots? Reach for a dedicated bot framework.
- Looking for deep historical tick data or cross-exchange research? Use a market-data platform.

Use `hyperliquid` when you want one operational interface to Hyperliquid that works the same way for humans, scripts, and agents.

## Acknowledgments

Built on [`hypersdk`](https://github.com/infinitefield/hypersdk) by Infinite Field for Hyperliquid HTTP, WebSocket, and EIP-712 signing. `hypersdk` is licensed under the [Mozilla Public License 2.0](https://www.mozilla.org/en-US/MPL/2.0/).

## License

MIT — see [`LICENSE`](LICENSE).

### Third-Party Licenses

`hyperliquid-cli` is MIT, but it depends on open-source crates with their own licenses:

| Dependency | License | Notes |
| --- | --- | --- |
| [`hypersdk`](https://github.com/infinitefield/hypersdk) | MPL-2.0 | Used unmodified as a Cargo dependency. |
| [`alloy`](https://github.com/alloy-rs/alloy) family | MIT OR Apache-2.0 | EVM primitives and signers. |
| [`tokio`](https://github.com/tokio-rs/tokio) | MIT | Async runtime. |
| [`clap`](https://github.com/clap-rs/clap) | MIT OR Apache-2.0 | CLI framework. |
| [`reqwest`](https://github.com/seanmonstar/reqwest) | MIT OR Apache-2.0 | HTTP client. |
| [`rust_decimal`](https://github.com/paupino/rust-decimal) | MIT | Fixed-point decimal math. |
| [`ows-lib`](https://crates.io/crates/ows-lib) | See crate metadata | OWS wallet backend. |

Because `hypersdk` is consumed as an unmodified upstream Cargo dependency, MPL-2.0's file-level copyleft is satisfied by the public upstream repository. If you fork this CLI and modify `hypersdk` source files in-tree, those files must remain MPL-2.0 and their modified source must be made available. The remainder of `hyperliquid-cli` stays MIT.

Generate the full transitive license report with:

```bash
cargo install cargo-license
cargo license
```

## Disclaimer

This software is provided "as is", without warranty of any kind. Trading on decentralized exchanges involves substantial risk of loss. You are solely responsible for your keys, signed actions, and trading decisions. This project is not officially affiliated with Hyperliquid.
