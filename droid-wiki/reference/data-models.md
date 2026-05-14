# Data models

## Core types

### Network

```rust
enum Network {
    Mainnet,
    Testnet,
}
```

Serialized as `"mainnet"` or `"testnet"` (case-insensitive on deserialization).

### OutputFormat

```rust
enum OutputFormat {
    Pretty,
    Table,
    Json,
}
```

### CliError

Structured error variants with exit codes 0-15. See the [debugging guide](../how-to-contribute/debugging.md) for common errors and exit codes.

### AssetQuery

Parsed asset input:

```rust
enum AssetQuery {
    Perp(String),          // "BTC"
    Spot(String),          // "PURR/USDC"
    Hip3 { dex, token },   // "dex:TOKEN"
    Outcome(String),       // "#10" or "+10"
}
```

## Command contracts

### Lifecycle

```rust
enum Lifecycle {
    ReadOnly,
    Streaming,
    InteractiveLocal,
    LiveMutating,
    BlockedUnsupported,
}
```

### Risk

```rust
enum Risk {
    None,
    LocalState,
    LocalSecret,
    AccountState,
    FundsMovement,
}
```

### DryRunPolicy

```rust
enum DryRunPolicy {
    NotSupported,
    Optional,
}
```

### ConfirmationPolicy

```rust
enum ConfirmationPolicy {
    None,
    Prompt,
}
```

## Financial values

All prices, sizes, and amounts use `rust_decimal::Decimal`. In JSON, they are serialized as strings:

```json
{
  "price": "50000.5",
  "size": "0.001"
}
```

## Action signing

### Action types (subset)

| Action | Description |
|--------|-------------|
| `OrderRequest` | Limit, market, stop-loss, take-profit, stop-limit, take-limit |
| `Cancel` | Cancel by order ID |
| `CancelByCloid` | Cancel by client order ID |
| `BatchCancel` | Cancel multiple orders |
| `Modify` | Modify an existing order |
| `ScheduleCancel` | Dead man's switch |
| `UsdClassTransfer` | Spot↔perp USDC transfer |
| `UsdSend` | USDC send to address |
| `SpotSend` | Spot token send |
| `Withdraw` | Withdraw to Arbitrum |
| `UpdateLeverage` | Update position leverage |
| `UpdateIsolatedMargin` | Adjust isolated margin |
| `ApproveAgent` | Approve API/agent wallet |
| `VaultTransfer` | Vault deposit/withdraw |
| `TokenDelegate` | Staking delegate/undelegate |
| `CDeposit` / `CWithdraw` | Staking deposit/withdraw |
| `ApproveBuilderFee` | Builder fee approval |
| `CoreWriter` action id 15 | Borrow/lend supply and withdraw |

All actions are signed via EIP-712 typed data using the `HyperliquidTransaction:` domain prefix.

## Stored account schema

```sql
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    alias TEXT NOT NULL UNIQUE,
    address TEXT NOT NULL,
    encrypted_private_key TEXT NOT NULL,
    type TEXT NOT NULL,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    master_address TEXT,
    agent_name TEXT,
    expires_at INTEGER
);
```
