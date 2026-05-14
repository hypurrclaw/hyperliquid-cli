# Wallets and signing

Active contributors: Sayo

Wallet lifecycle management, API/agent wallet approval, and builder fee configuration. OWS (Open Wallet Standard) is the only wallet backend.

## Commands

| Command | Description | Implementation |
|---------|-------------|---------------|
| `setup` | Guided first-time setup wizard | `src/commands/setup.rs` |
| `wallet create` | Create and store a new OWS wallet | `src/commands/wallet.rs` |
| `wallet import [PRIVATE_KEY]` | Import a wallet via private key | `src/commands/wallet.rs` |
| `wallet import-mnemonic [MNEMONIC]` | Import via BIP-39 mnemonic | `src/commands/wallet.rs` |
| `wallet show` | Show current wallet metadata | `src/commands/wallet.rs` |
| `wallet address` | Print only the wallet address | `src/commands/wallet.rs` |
| `wallet list` | List all wallets in the OWS vault | `src/commands/wallet.rs` |
| `wallet rename <SELECTOR> --new-name <NAME>` | Rename a wallet | `src/commands/wallet.rs` |
| `wallet delete <SELECTOR>` | Delete a wallet (prompts unless `-y`) | `src/commands/wallet.rs` |
| `wallet export <SELECTOR>` | Export wallet secret (mnemonic or key) | `src/commands/wallet.rs` |
| `wallet reset` | Remove wallet configuration | `src/commands/wallet.rs` |
| `api-wallet create` | Generate or approve an API/agent wallet | `src/commands/api_wallet.rs` |
| `api-wallet approve` | Approve an existing agent address | `src/commands/api_wallet.rs` |
| `api-wallet list [ACCOUNT]` | List API wallets approved by a master | `src/commands/api_wallet.rs` |
| `api-wallet revoke` | Replace a named API wallet with a short-lived throwaway | `src/commands/api_wallet.rs` |
| `account add` | Store a signing account | `src/main.rs` (AccountCommands::Add) |
| `account ls` | List stored accounts | `src/main.rs` (AccountCommands::Ls) |
| `account set-default` | Set default account | `src/main.rs` (AccountCommands::SetDefault) |
| `account remove` | Remove a stored account | `src/main.rs` (AccountCommands::Remove) |
| `builder max-fee` | Query max builder fee | `src/commands/builder.rs` |
| `builder approved` | List approved builders | `src/commands/builder.rs` |
| `builder approve` | Approve/revoke builder fee cap | `src/commands/builder.rs` |

## Key abstractions

| Type | File | Description |
|------|------|-------------|
| `AccountRow` | `src/commands/wallet.rs` | Renderable stored account row (id, alias, address, type, default status) |
| `AccountsOutput` | `src/commands/wallet.rs` | List of stored accounts implementing `TableData` |
| `CreateArgs` | `src/commands/api_wallet.rs` | Args for API wallet creation (name, expiry, store options) |
| `ApproveArgs` | `src/commands/api_wallet.rs` | Args for approving an existing agent address |
| `ApproveBuilderFee` | `src/commands/builder.rs` | Solidity struct for builder fee approval action |

## Wallet lifecycle

```mermaid
stateDiagram-v2
    [*] --> Created: wallet create
    [*] --> Imported: wallet import / import-mnemonic
    Created --> Default: becomes default if first wallet
    Imported --> Default: becomes default if first wallet
    Default --> Renamed: wallet rename
    Default --> Exported: wallet export
    Default --> Deleted: wallet delete
    Deleted --> [*]
```

## API wallets

API wallets (agent wallets) are delegated Hyperliquid trading keys. They can trade for the approving master account but cannot withdraw. Key behaviors:

- `api-wallet create` generates a new local key, signs an `approveAgent` action, and prints the API private key once (or stores it encrypted with `--store --alias`)
- `api-wallet approve` approves an existing agent address without generating a new key
- Named API wallets replace prior agents with the same name
- Maximum agent expiration is 180 days
- Revoke replaces an API wallet with a short-lived (60s) throwaway agent

## Builder fees

Builder fee approvals use percent strings like `0.001%`. `0%` revokes by setting the max fee to zero. Perp builder fees are capped at `0.1%`, spot at `1%`. The action must be signed by the master account, not an API wallet.

## Entry points for modification

- **Add a new wallet command**: add variant to `WalletCommands` in `src/main.rs`, implement handler in `src/commands/wallet.rs`
- **Support a new wallet backend**: implement signing trait, add to signer resolution chain in `src/auth.rs`
- **Change API wallet behavior**: modify `src/commands/api_wallet.rs`
