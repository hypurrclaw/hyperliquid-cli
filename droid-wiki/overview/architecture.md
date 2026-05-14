# Architecture

## System overview

`hyperliquid-cli` is a single Rust binary with a clap-based command parser that dispatches to modular command handlers. The binary talks to Hyperliquid's HTTP API (`/info` and `/exchange` endpoints), WebSocket streams, and GitHub release metadata for its update workflow.

```mermaid
graph TD
    User[User / Agent] -->|CLI args| Main[src/main.rs<br/>clap parse + dispatch]
    Main -->|resolve format + context| Runtime[src/cli_runtime.rs<br/>command routing]

    Runtime -->|read only| InfoAPI[Hyperliquid /info API]
    Runtime -->|signed actions| ExchangeAPI[Hyperliquid /exchange API]
    Runtime -->|streaming| Ws[WebSocket subscriptions]
    Runtime -->|release metadata| GitHub[GitHub releases API]

    subgraph Signer Resolution
        Auth[src/auth.rs]
        Signing[src/signing.rs]
        Ows[src/ows.rs]
        Db[src/db.rs<br/>encrypted SQLite]
        Auth --> Signing
        Signing --> Ows
        Signing --> Db
    end

    subgraph Output Pipeline
        Output[src/output/mod.rs]
        Output --> Pretty[Pretty<br/>ANSI colors + tabwriter]
        Output --> Table[Table<br/>tabled crate]
        Output --> Json[JSON<br/>stable snake_case]
    end

    Runtime --> Auth
    Runtime --> Output
```

## Component map

| Component | File(s) | Role |
|-----------|---------|------|
| Entry point | `src/main.rs` | Clap CLI definition, argument parsing, format resolution, error routing |
| Command dispatch | `src/cli_runtime.rs` | Per-command routing, context resolution, API client creation, dry-run gating |
| Command registry | `src/command_registry.rs` | Typed command contracts loaded from `src/command_catalog.json` |
| Command handlers | `src/commands/` | 23 domain modules implementing all ~45 commands |
| Output system | `src/output/mod.rs` | Pretty/table/JSON rendering, color theme, JSON projection |
| Error system | `src/errors.rs` | Structured `CliError` variants with exit codes 0-15 |
| Auth/signing | `src/auth.rs`, `src/signing.rs`, `src/resolvers.rs` | Signer resolution from private keys, keystores, stored accounts, or OWS |
| OWS wallet backend | `src/ows.rs` | Open Wallet Standard vault at `~/.hyperliquid` |
| Account storage | `src/db.rs` | SQLite with AES-256-GCM encrypted private keys |
| Config | `src/config.rs` | Config file, env vars, network selection |
| Dry-run | `src/dry_run.rs` | Action plan previews for mutating commands |
| Watch/streaming | `src/watch.rs` | Terminal watch mode (alternate screen) and WebSocket subscription helpers |
| Update checks | `src/update_check.rs`, `install.sh` | Best-effort release notices and self-update/install archive verification |
| Input hardening | `src/input_hardening.rs` | Path traversal prevention, JSON size/depth limits |
| Response sanitization | `src/response_sanitization.rs` | Strips ANSI/control sequences from untrusted remote text |
| HTTP helpers | `src/http_api.rs` | Shared reqwest client, POST helpers, error mapping |

## Data flow: signed action

```mermaid
sequenceDiagram
    participant User
    participant Clap as clap parse
    participant Runtime as cli_runtime
    participant Auth as auth/signing
    participant Db as account store
    participant Ows as OWS vault
    participant Signer as SelectedSigner
    participant Actions as commands/actions
    participant API as /exchange

    User->>Clap: orders create --coin BTC --side buy --price 50000 --size 0.001
    Clap->>Runtime: parsed Cli struct
    Runtime->>Auth: resolve signer (--private-key, --account, --ows-signer, etc.)
    Auth->>Db: load encrypted key
    Db-->>Auth: decrypted PrivateKeySigner
    alt OWS signer
        Auth->>Ows: resolve wallet
        Ows-->>Auth: OwsSigningConfig
    end
    Auth-->>Runtime: ResolvedSigner
    Runtime->>Runtime: check --dry-run
    Runtime->>Actions: prepare and sign action
    Actions->>Signer: sign_l1_action(action, nonce)
    Signer-->>Actions: signature
    Actions->>API: POST /exchange {action, nonce, signature}
    API-->>Actions: response
    Actions-->>Runtime: output
    Runtime->>User: rendered output (pretty/table/json)
```

## Key dependencies

- **hypersdk** — Hyperliquid API types, signing, WebSocket client
- **clap** — CLI argument parsing with derive macros
- **tokio** — async runtime
- **alloy** — Ethereum signing, EIP-712 typed data, keystore support
- **rusqlite** — encrypted local account storage
- **ows-lib** — Open Wallet Standard integration
- **rust_decimal** — financial precision for prices and amounts

## Current command surface

The command surface is catalog-driven from `src/command_catalog.json`; `src/command_registry.rs` loads that embedded catalog and emits schemas through `src/commands/schema.rs`. Builder approval, referral defaults, OWS-first wallet resolution, isolated-margin validation, and the release update path are part of the main CLI runtime.
