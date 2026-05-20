# Glossary

Project vocabulary, selector semantics, and acronyms used throughout the codebase and CLI surface.

## Selector vocabulary

The CLI distinguishes several classes of "who/what is this" inputs. Mixing them up is a common bug class, so the source code and schema metadata treat them as distinct types.

| Term | Meaning | Examples |
|------|---------|----------|
| **Explicit local signer** | Non-stored signer passed for a command through `--private-key`, `HYPERLIQUID_PRIVATE_KEY`, config `private_key`, or `--keystore` | Scripted signing without using OWS storage |
| **Selected signer** | The key used to sign an authenticated action for the current command | Resolved through `SelectedSigner` in `src/signing.rs` |
| **API wallet / agent wallet** | Delegated Hyperliquid trading key approved by a master account via `approveAgent`. Can trade, cannot withdraw | Created with `api-wallet create` |
| **OWS wallet** | Wallet managed by the Open Wallet Standard backend at `~/.hyperliquid` (or `HYPERLIQUID_OWS_VAULT_PATH`) | The default backend; selected with `--ows-signer` |
| **Protocol user address** / `USER` | Public account-data lookup target. Anything readable on-chain | The argument to `account portfolio USER` |
| **`ACCOUNT_SELECTOR`** | Input that may accept an OWS wallet name, OWS wallet id, or a `0x` address | `--account alice`, `--account 0xabc...` |
| **`*_ADDRESS`** | Explicit protocol object address. Wallet names and aliases are **not** resolved for these | Transfer recipient (`--to`), vault, validator, builder |
| **Acting-account selector** | Signer is one address; the action is taken on behalf of another (subaccount or vault). Documented separately in command schemas | `orders --on-behalf-of`, `subaccount transfer --subaccount` |

When schema metadata disagrees with README prose, treat schema `input_kind`, `risk`, `dry_run`, and `confirmation` metadata as authoritative.

## Protocol terms

| Term | Meaning |
|------|---------|
| **Hyperliquid** | The DEX this CLI targets. Mainnet uses the `eip155:999` chain id. |
| **HIP-3** | Builder-deployed perpetual market on Hyperliquid. Symbols are DEX-qualified, e.g. `xyz:TSLA`. |
| **TWAP** | Time-weighted average price order, sliced over the protocol's TWAP duration. |
| **TP/SL** | Take-profit / stop-loss orders. Often attached to a position via `orders tpsl`. |
| **Cloid** | Client-side order id supplied by the caller. Distinct from the protocol's `oid`. |
| **OID** | Protocol-assigned order id. |
| **Scheduled cancel** | A `ScheduleCancel` action that asks the exchange to cancel everything if the next heartbeat is missed (dead-man's switch). |
| **Outcome market** | Hyperliquid prediction-market sides referenced as `#N` or `+N` notation. |
| **Funding** | Periodic perp funding payment, separate from fills. |
| **Builder fee** | A fee a Hyperliquid builder can charge through their UI. Requires user `approveBuilderFee`. |
| **Referral code** | A code a referrer registers and a new account can `referrerCode` to. |
| **Subaccount** | A separate Hyperliquid balance domain under a master account. |
| **Vault** | A managed-deposit position address that can accept deposits and process withdrawals subject to a lockup. |
| **Account abstraction** | Hyperliquid account-abstraction mode; toggled via `account abstraction set`. |

## Output and agent terms

| Term | Meaning |
|------|---------|
| **`--format pretty\|table\|json`** | Effective output format. Precedence: explicit flag, then `HYPERLIQUID_FORMAT`, then agent/non-TTY defaults. |
| **`HYPERLIQUID_AGENT=1`** | Forces JSON defaults and non-TTY semantics for an agent caller. |
| **`--select`** | Comma-separated field projection over JSON output. |
| **`--results-only`** | Strip envelope/metadata from JSON output. |
| **`--max-results N`** | Top-level result cap for agent context control. |
| **`--max-events`, `--max-ticks`, `--idle-timeout-ms`** | Bounds for streaming and watch commands so they always return. |
| **`schema`** | Subcommand that emits machine-readable command contracts. |
| **`raw_payload`** | Schema field documenting whether a command accepts `--payload-json` / `--payload-file`. Treated as fail-closed until an action is explicitly allowlisted. |
| **`[untrusted remote data]`** | Sanitization label applied to any string returned by the exchange or HTTP layer before display. |

## Command lifecycle terms (`CommandContract`)

These come from `src/command_registry.rs` and the embedded `command_catalog.json`.

| Field | Meaning |
|-------|---------|
| **`Lifecycle`** | Whether the command is a read, signed action, or local mutation. |
| **`Risk`** | `safe`, `funds_movement`, `irreversible`. Drives confirmation prompts. |
| **`Mutability`** | Whether the command mutates remote state. |
| **`DryRunPolicy`** | `not_applicable`, `supported`, `dry_run_only`. |
| **`RawPayloadPolicy`** | Whether `--payload-json` / `--payload-file` are accepted. |
| **`ConfirmationPolicy`** | `none`, `required`, `required_unless_yes`. |
| **`ActionPlan` / `DryRunEnvelope`** | Stable JSON shape printed by `--dry-run`. See [features/dry-run](../features/dry-run.md). |

## Wallet and signing acronyms

| Acronym | Expansion |
|---------|-----------|
| **OWS** | Open Wallet Standard. The primary wallet vault backend (`ows-lib` crate). |
| **CAIP-2** | Chain Agnostic namespace. Hyperliquid uses `eip155:999`. |
| **EIP-712** | Typed structured data signing standard used for Hyperliquid actions. |
| **BIP-39** | Mnemonic seed standard supported by `wallet import-mnemonic`. |
| **Foundry keystore** | JSON-encrypted Ethereum keystore (`--keystore`, `--keystore-password`). |
