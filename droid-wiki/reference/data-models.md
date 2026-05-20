# Data models

Key types developers and reviewers encounter, grouped by topic. All file paths are relative to the repository root.

## Asset resolution (`src/commands/mod.rs`)

| Type | Purpose |
|------|---------|
| `AssetQuery` | Enum: `Perp(String)`, `Spot(String)`, `Hip3 { dex, token }`, `Outcome(String)` |
| `PerpAsset` | Resolved perpetual: name, index, optional DEX, sz_decimals, collateral |
| `SpotAsset` | Resolved spot pair |
| `ResolvedAsset` | Sum type of resolved assets |
| `AssetResolver` | Resolves an `AssetQuery` against cached metadata |
| `MetadataCache` | 60-second TTL cache of perps/spot metadata (`METADATA_TTL`) |
| `AssetMetadata` | Raw exchange metadata blob |

## Auth and signing (`src/auth.rs`, `src/signing.rs`, `src/resolvers.rs`)

| Type | Purpose |
|------|---------|
| `ResolvedSigner` | Public wrapper around `SelectedSigner` |
| `SelectedSigner` | Backend-neutral signer (`LocalPrivateKey` or `Ows`) |
| `SignerSource` | `PrivateKey`, `Keystore`, `StoredAccount { alias }`, `Ows { selector }` |
| `SignerResolverInput` | Inputs to `resolvers::resolve_selected_signer` |
| `DefaultSignerFallback` | `AllowStoredDefaultOrFirst`, `Disallow` |

## OWS (`src/ows.rs`)

| Type | Purpose |
|------|---------|
| `OwsSignerConfig` | Selector + address + optional `OwsWalletSelection` + vault path |
| `OwsSigningConfig` | Backend signing config used by `SelectedSigner` |
| `OwsWalletSelection` | Wallet id, name, chain id |
| `HYPERLIQUID_CAIP2` const | `"eip155:999"` |

## Account storage (`src/db.rs`)

| Type | Purpose |
|------|---------|
| `Account` | Stored account row (alias, address, encrypted key blob) |
| `AccountStore` | SQLite-backed store with AES-256-GCM |
| `EncryptionKeyStore` (trait) | Key-material backend (OS keychain or passphrase) |
| `AgentAccountMetadata` | Master address, agent name, expiry for API/agent wallets |

## Command registry (`src/command_registry.rs`)

| Type | Purpose |
|------|---------|
| `CommandRegistry` | Loaded from `src/command_catalog.json` |
| `CommandContract` | Per-command typed contract |
| `Lifecycle` | Lifecycle category |
| `Risk` | `safe`, `funds_movement`, `irreversible` |
| `Mutability` | Mutation flag |
| `DryRunPolicy` | `not_applicable`, `supported`, `dry_run_only` |
| `RawPayloadPolicy` | Raw-payload support level |
| `ConfirmationPolicy` | `none`, `required`, `required_unless_yes` |
| `Transport` | HTTP / WebSocket usage |
| `OwsSupport` | OWS support level |
| `OutputContract` | Success-shape descriptor |
| `HandlerBinding` | Runtime handler dispatch tag (`src/command_handlers.rs`) |
| `InputContract` | Per-arg metadata (input_kind, required, default) |

## Dry-run (`src/dry_run.rs`)

| Type | Purpose |
|------|---------|
| `ActionPlan` | `would_execute`, `kind`, `reversibility`, `live_submission` |
| `ActionKind` | `SignedExchangeAction`, `LocalStateMutation` |
| `ActionReversibility` | `Reversible`, `PartiallyReversible`, `Irreversible` |
| `LiveSubmissionPolicy` | `DryRunOnly`, `ValidateConfirmSignSubmit` |
| `DryRunSigningContext` | `signer`, `acting_as`, `vault_address` |
| `DryRunEnvelope` | Stable JSON envelope for `--dry-run` |

## Output (`src/output/mod.rs`)

| Type | Purpose |
|------|---------|
| `OutputFormat` | `Pretty`, `Table`, `Json` |
| `TableData` (trait) | Implemented by every renderable type |
| `colors::*` | ANSI color helpers |

## Errors (`src/errors.rs`)

| Type | Purpose |
|------|---------|
| `CliError` | Top-level error variants with exit-code mapping |

## Config (`src/config.rs`)

| Type | Purpose |
|------|---------|
| `Config` | On-disk config |
| `Network` | `Mainnet`, `Testnet` |

## Watch (`src/watch.rs`)

| Type | Purpose |
|------|---------|
| `SubscribeEventKind` | `Trades`, `Orderbook`, `Candles`, `AllMids`, `OrderUpdates`, `Fills` |
| `SnapshotWatchArgs` | Flattened clap struct for `--watch`-capable commands |

## See also

- [systems/command-registry](../systems/command-registry.md)
- [features/dry-run](../features/dry-run.md)
- [systems/signing-and-wallets](../systems/signing-and-wallets.md)
