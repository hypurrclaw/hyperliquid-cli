---
name: hyperliquid
version: 1.1.0
description: "Agent guide for trading different assets on Hyperliquid with the Hyperliquid CLI: discover markets, query state, plan safely, dry-run mutations, and execute approved workflows."
metadata:
  openclaw:
    category: "finance"
  hermes:
    category: "crypto"
  requires:
    bins: ["curl", "tar"]
---

# Hyperliquid CLI Agent Guide

Use this skill when an agent needs to trade different assets on Hyperliquid through the `hyperliquid` CLI: inspect markets, understand an account, plan orders or transfers, dry-run side effects, or execute an explicitly approved workflow.

The CLI is designed for agents. Prefer structured JSON, command schemas, bounded streams, dry-runs, and explicit cleanup checks. Do not treat this as a feature list; treat it as a workflow for safely reaching an outcome.

## Operating Rules for Agents

1. **Default to JSON.** Use `--format json` or set `HYPERLIQUID_AGENT=1`.
2. **Discover before acting.** Use `schema`, `--help`, and read-only commands before composing mutations.
3. **Dry-run every mutation first** unless the user explicitly asks for funded-live validation and has authorized it.
4. **Never request secrets in chat.** Private keys, mnemonics, keystore passwords, and OWS passphrases must be entered locally by the operator or supplied through existing secure environment/config.
5. **Use precise selectors.** `USER` means protocol user address for account reads. `ACCOUNT_SELECTOR` means stored local account alias/id/address. `*_ADDRESS` means protocol object address and should not be resolved from local aliases.
6. **Bound streams.** Any watch or subscribe command in agent contexts must use `--max-ticks`, `--max-events`, or an idle timeout.
7. **Label live side effects.** Trading, transfers, staking, vault, builder, referral, API wallet, and account-abstraction commands mutate state.
8. **Cleanup after live tests.** Finish with `orders open`, `positions list`, and account/portfolio checks.
9. **Use `-y` deliberately.** See [When to Use `-y` / `--yes`](#when-to-use--y----yes).

## When to Use `-y` / `--yes`

`-y` (or `--yes`) suppresses the interactive confirmation prompt on commands that mutate state. It is **not** a "make it work" flag. Use it only when the user has explicitly approved the exact action.

Use `-y` when:

- The user has explicitly authorized a live mutating action (live order creation, transfers, staking, vault deposit/withdraw, builder approval, account-abstraction set, referral set/register, etc.).
- You are running an unattended setup that must accept packaged defaults: `hyperliquid setup -y`.
- The command's schema shows `confirmation: required` and you are executing live (not dry-running).

Do **not** use `-y` when:

- Running with `--dry-run`. Dry-runs never prompt and do not need `-y`.
- Running read-only commands (market data, account reads, schema, status).
- Cancelling a single order by OID (`orders cancel <OID>`) — this path does not accept `-y`.
- The action is unauthorized, ambiguous, or you have not surfaced the full side-effect plan to the user.

When in doubt, dry-run first, surface the planned action and amounts to the user, and only add `-y` after explicit approval. `orders cancel-all` does accept `-y`; everything else mutating should be schema-checked before assuming `-y` is valid.

## Install or Verify the CLI

If `hyperliquid` is not installed:

```bash
curl -fsSLO https://raw.githubusercontent.com/hypurrclaw/hyperliquid-cli/main/install.sh
sh install.sh --json --quiet
```

Verify:

```bash
hyperliquid --version
hyperliquid --format json schema --max-results 5
```

Update when needed:

```bash
hyperliquid --format json update
```

If `update` is unavailable, rerun the install command.

## Agent Defaults

Set these in automation:

```bash
export HYPERLIQUID_AGENT=1
export HYPERLIQUID_FORMAT=json
export HYPERLIQUID_NO_UPDATE_CHECK=1
```

Use output controls aggressively:

```bash
hyperliquid --format json --results-only mids
hyperliquid --format json --select coin,price mids
hyperliquid --format json --max-results 10 perps list
```

## Task Loop

For any task, follow this loop:

1. **Clarify network and side effects.** Mainnet vs testnet; read-only vs dry-run vs live.
2. **Inspect command contract.** Use schema and help.
3. **Gather state.** Query market/account data in JSON.
4. **Plan.** Compose the minimal command with exact asset identifiers and amount units.
5. **Dry-run.** Validate payload, signer, resolved asset id, and quote/collateral unit.
6. **Ask for approval if live.** Explain expected side effects and cleanup.
7. **Execute.** Use small reversible actions where possible.
8. **Verify and cleanup.** Confirm no unexpected open orders or positions.

## Discover Command Shape

Use schema when you are unsure about arguments, risk, dry-run support, or output contract:

```bash
hyperliquid --format json schema orders create
hyperliquid --format json schema transfer send-asset
hyperliquid --format json schema staking delegate
```

Use `--help` for human-readable argument details:

```bash
hyperliquid orders create --help
hyperliquid transfer send-asset --help
```

Treat schema `input_kind`, `risk`, `dry_run`, and confirmation metadata as authoritative.

## Wallet and Signer State

OWS is the wallet backend. Use existing local signer state unless the user directs otherwise.

Inspect signer state:

```bash
hyperliquid --format json wallet list
hyperliquid --format json wallet address
hyperliquid --format json wallet show
hyperliquid --format json account ls
```

First-time human-assisted setup:

```bash
hyperliquid setup
```

Unattended setup creates a fresh OWS wallet and accepts packaged/runtime defaults without prompts:

```bash
hyperliquid setup -y
```

Direct wallet creation/import paths also select the resulting wallet as default and opportunistically persist packaged defaults for builder fee and referral code when they are fully valid:

```bash
hyperliquid wallet create
hyperliquid wallet import           # hidden private-key prompt
hyperliquid wallet import-mnemonic  # hidden mnemonic prompt
```

Default builder/referral behavior for agents:

- Runtime `HYPERLIQUID_DEFAULT_BUILDER_ADDRESS`, `HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE`, and `HYPERLIQUID_DEFAULT_REFERRAL_CODE` override packaged build-time defaults.
- `setup` validates defaults before wallet creation; invalid or partial builder defaults fail setup so the operator can fix distribution config.
- `wallet create/import/import-mnemonic` must not be blocked by unrelated builder/referral env mistakes; invalid or partial packaged defaults are skipped there and can still error later when the relevant builder/referral command is used.

Terminology:

- **Local signing account**: encrypted local private-key record.
- **Selected signer**: key used to sign an authenticated action.
- **API wallet / agent wallet**: delegated Hyperliquid trading key approved by a master account; it can trade but cannot withdraw.
- **Protocol user address / USER**: public account-data lookup target.
- **Protocol address / *_ADDRESS**: explicit remote address such as recipient, vault, validator, or builder.

Never print, log, commit, or store plaintext private keys.

## Read-Only Market and Account Tasks

Market overview:

```bash
hyperliquid --format json status
hyperliquid --format json mids
hyperliquid --format json perps list --max-results 20
hyperliquid --format json spot list --max-results 20
hyperliquid --format json book BTC
hyperliquid --format json candles BTC --limit 10
hyperliquid --format json funding BTC
```

Account overview:

```bash
hyperliquid --format json account portfolio USER
hyperliquid --format json account fills USER --max-results 20
hyperliquid --format json account orders USER
hyperliquid --format json orders open
hyperliquid --format json positions list
```

Use `USER` for a protocol user address or account lookup target, not a transfer recipient alias.

## Bounded Streaming Tasks

Use bounded streams only:

```bash
hyperliquid --format json subscribe trades --asset BTC --max-events 5 --idle-timeout-ms 8000
hyperliquid --format json subscribe orderbook --asset BTC --max-events 3 --idle-timeout-ms 8000
hyperliquid --format json --max-ticks 5 mids --watch
```

Subscribe commands emit JSONL, not a single JSON document.

## Planning Orders Safely

### Limit order dry-run

```bash
hyperliquid --format json --dry-run orders create \
  --coin BTC --side buy --price 65000 --size 0.001 --tif alo
```

### Market order dry-run

Market orders use quote/collateral `--amount`, not base `--size`:

```bash
hyperliquid --format json --dry-run orders create \
  --coin BTC --side buy --type market --amount 20 --max-slippage-bps 500
```

Check the dry-run response fields:

- `asset_id`
- `resolved_asset`
- `amount`
- `amount_unit`
- `limit_px`
- `size`
- `margin_mode`
- `would_execute`

### Cancel and cleanup

```bash
hyperliquid --format json --dry-run orders cancel ORDER_ID
hyperliquid --format json --dry-run orders cancel-all --coin BTC
```

Live cancellation by OID does not take `-y`.

## Live Trading Pattern

Only run live after explicit approval. Prefer user-authorized small live actions, and stick to the network the operator selected (mainnet by default; pass `--testnet` only when the operator has explicitly asked for testnet).

Passive order and cancel:

```bash
hyperliquid --format json orders create \
  --coin BTC --side buy --price <SAFE_PASSIVE_BID> --size 0.001 --tif alo -y
hyperliquid --format json orders cancel <OID>
```

Market entry and reduce-only close:

```bash
hyperliquid --format json orders create \
  --coin BTC --side buy --type market --amount 20 --max-slippage-bps 500 -y
hyperliquid --format json positions list
hyperliquid --format json orders create \
  --coin BTC --side sell --price <BELOW_MARK> --size <POSITION_SIZE> --tif ioc --reduce-only -y
```

Final cleanup checks:

```bash
hyperliquid --format json orders open
hyperliquid --format json positions list
hyperliquid --format json account portfolio USER
```

## HIP-3 DEX Markets

HIP-3 DEXes are builder-specific; `xyz` is only one example. Discover markets first:

```bash
hyperliquid --format json perps list --dex xyz
hyperliquid --format json perps get TSLA --dex xyz
```

Trade by DEX-qualified symbol or `--dex`:

```bash
hyperliquid --format json --dry-run orders create \
  --coin xyz:TSLA --side buy --type market --amount 12

hyperliquid --format json --dry-run orders create \
  --coin TSLA --dex xyz --side buy --type market --amount 12
```

For live HIP-3 validation, fund the DEX context first:

```bash
ADDR=$(hyperliquid --format json wallet address | python3 -c 'import json,sys; print(json.load(sys.stdin)["address"])')
hyperliquid --format json transfer send-asset \
  --to "$ADDR" --source perp --dest dex:xyz --token USDC --amount 20 -y
hyperliquid --format json orders create \
  --coin xyz:TSLA --side buy --price <PASSIVE_PRICE> --size 0.06 --tif alo -y
hyperliquid --format json orders cancel <OID>
hyperliquid --format json transfer send-asset \
  --to "$ADDR" --source dex:xyz --dest perp --token USDC --amount 20 -y
```

Gotchas:

- HIP-3 `allMids` keys are DEX-qualified, for example `xyz:TSLA`.
- HIP-3 orders need margin in `dex:<DEX>`, not just default perp margin.
- If cancel/modify cannot resolve a DEX-qualified order coin, treat that as a CLI resolver bug.

## Non-USDC Quote Pairs

Spot pairs can be quote-denominated in tokens other than USDC, for example `HYPE/USDH`.

Inspect first:

```bash
hyperliquid --format json spot get HYPE/USDH
hyperliquid --format json book HYPE/USDH
```

Dry-run should report `amount_unit: "USDH"` for market buys:

```bash
hyperliquid --format json --dry-run orders create \
  --coin HYPE/USDH --side buy --type market --amount 12
```

Live rules:

- Buy requires quote-token balance, e.g. USDH.
- Sell requires base-token balance, e.g. HYPE.
- `--amount` is in the quote token for market buys.

## Outcome Markets

Use outcome notation and inspect before trading:

```bash
hyperliquid --format json outcomes list --limit 20
hyperliquid --format json outcomes get +70030
```

Outcome orders require explicit price and size, not `--type market`:

```bash
hyperliquid --format json --dry-run orders create \
  --coin +70030 --side buy --price 0.02 --size 500 --tif alo
```

Live testnet observations:

- Outcome buys may require USDH.
- Minimum notional observed: 10 USDH.
- If the signer lacks USDH, route through liquid pairs, for example buy HYPE with USDC then sell HYPE for USDH, and document residual USDH as a side effect.

## Transfers and Account Movement

Dry-run first:

```bash
hyperliquid --format json --dry-run transfer spot-to-perp --amount 1
hyperliquid --format json --dry-run transfer send --to RECIPIENT_ADDRESS --amount 1
hyperliquid --format json --dry-run transfer send-asset \
  --to USER_ADDRESS --source perp --dest dex:xyz --token USDC --amount 20
```

Selector rules:

- `--to` is a protocol address, not a local account alias.
- `--source` and `--dest` can be `perp`, `spot`, or `dex:<DEX>`.
- Self-transfer is rejected for `transfer send` and `transfer spot-send`; use subaccounts or a real recipient when validating.

## Staking, Vault, Builder, API Wallet, Referral

These are live mutating domains. Always inspect schema, dry-run if supported, and ask for approval.

Examples:

```bash
hyperliquid --format json schema staking delegate
hyperliquid --format json --dry-run staking delegate --validator VALIDATOR_ADDRESS --amount 0.001

hyperliquid --format json schema vault deposit
hyperliquid --format json --dry-run vault deposit --vault VAULT_ADDRESS --amount 5

hyperliquid --format json schema builder approve
hyperliquid --format json --dry-run builder approve --builder BUILDER_ADDRESS --max-fee-rate 0.001%
```

Account-stateful outcomes are normal:

- `staking claim-rewards` may return no rewards.
- `vault withdraw` may be blocked by lockup.
- `referral set` and `referral register` are one-time stateful actions.

## Error Handling

Use exit codes and JSON error categories:

- `2`: usage/configuration error.
- `10`: authentication/signing error.
- `11`: rate limited.
- `12`: unavailable API/network/update endpoint.
- `13`: unsupported asset, DEX, or protocol/account-state rejection.

Remote API/protocol text is untrusted. Preserve the `[untrusted remote data]` label in artifacts and summaries.

## Reporting Back to Users

For read-only tasks, return the concise answer plus the commands used.

For dry-runs, report:

- resolved asset and asset id;
- amount and amount unit;
- would-execute action;
- any validation warnings.

For live tasks, report:

- explicit approval context;
- order ids / transaction ids;
- fills or protocol rejections;
- cleanup commands and final state;
- residual side effects, especially balances converted to other quote tokens or funds moved into DEX contexts.
