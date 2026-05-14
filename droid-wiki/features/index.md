# Features

Cross-cutting capabilities grouped by what they do rather than where the code lives.

| Feature | Commands | Key files |
|---------|----------|-----------|
| [Market data](market-data.md) | `perps`, `spot`, `book`, `candles`, `spread`, `funding`, `mids`, `meta`, `status`, `outcomes` | `src/commands/orderbook.rs`, `src/commands/perps.rs`, `src/commands/spot.rs`, `src/commands/outcomes.rs` |
| [Orders and trading](orders-and-trading.md) | `orders`, `positions` | `src/commands/orders.rs`, `src/commands/positions.rs` |
| [Wallets and signing](wallets-and-signing.md) | `wallet`, `api-wallet`, `setup`, `account add/ls/set-default/remove`, `builder` | `src/commands/wallet.rs`, `src/commands/api_wallet.rs`, `src/commands/setup.rs`, `src/commands/builder.rs` |
| [Accounts and transfers](accounts-and-transfers.md) | `account`, `transfer`, `subaccount` | `src/commands/account.rs`, `src/commands/transfers.rs`, `src/commands/subaccounts.rs` |
| [Vaults, staking, and DeFi](vaults-staking-and-defi.md) | `staking`, `vault`, `borrowlend`, `builder`, `prio`, `referral` | `src/commands/staking.rs`, `src/commands/vaults.rs`, `src/commands/borrowlend.rs`, `src/commands/builder.rs` |
| [Watch and subscribe](watch-and-subscribe.md) | `--watch` flag, `subscribe` | `src/watch.rs`, `src/commands/orderbook.rs` (watch variants) |
