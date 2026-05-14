# Accounts and transfers

Active contributors: Sayo

Public account data queries and authenticated fund transfer commands.

## Commands

| Command | Description | Implementation |
|---------|-------------|---------------|
| `account fills <ADDR> [--start] [--end] [--aggregate-by-time]` | Public fill history | `src/commands/account.rs` |
| `account fees <ADDR>` | Fee schedule and volume context | `src/commands/account.rs` |
| `account rate-limit <ADDR>` | User rate-limit context | `src/commands/account.rs` |
| `account orders <ADDR>` | Public open orders | `src/commands/account.rs` |
| `account portfolio <ADDR>` | Public portfolio summary | `src/commands/account.rs` |
| `account portfolio-history <ADDR>` | Portfolio graph/history data | `src/commands/account.rs` |
| `account ledger <ADDR> --start <TIME> [--end <TIME>]` | Non-funding ledger updates | `src/commands/account.rs` |
| `account funding <ADDR> --start <TIME> [--end <TIME>]` | Funding payment history | `src/commands/account.rs` |
| `account subaccounts <ADDR>` | Public subaccounts | `src/commands/account.rs` |
| `account twap-history <ADDR>` | TWAP order history | `src/commands/account.rs` |
| `account twap-fills <ADDR> [--start] [--end]` | TWAP slice fills | `src/commands/account.rs` |
| `account abstraction [ADDR]` | Read account abstraction mode | `src/commands/account.rs` |
| `account abstraction set --mode <MODE>` | Set abstraction mode (signed) | `src/commands/account.rs` |
| `transfer spot-to-perp --amount <USDC>` | Move USDC from spot to perp | `src/commands/transfers.rs` |
| `transfer perp-to-spot --amount <USDC>` | Move USDC from perp to spot | `src/commands/transfers.rs` |
| `transfer send --to <ADDR> --amount <USDC>` | Send USDC to another address | `src/commands/transfers.rs` |
| `transfer spot-send --to <ADDR> --token <TOKEN> --amount <AMOUNT>` | Send a spot token | `src/commands/transfers.rs` |
| `transfer send-asset` | Send asset between spot/perp/DEX contexts | `src/commands/transfers.rs` |
| `transfer withdraw --to <ADDR> --amount <USDC>` | Withdraw USDC to Arbitrum | `src/commands/transfers.rs` |
| `subaccount list <ADDR>` | List subaccounts for a master | `src/commands/subaccounts.rs` |
| `subaccount create --name <NAME>` | Create a new subaccount | `src/commands/subaccounts.rs` |
| `subaccount transfer --subaccount <SEL> --amount <USDC> --direction deposit|withdraw` | USDC transfer to/from subaccount | `src/commands/subaccounts.rs` |
| `subaccount spot-transfer --subaccount <SEL> --token <TOKEN> --amount <AMOUNT> --direction deposit|withdraw` | Spot token transfer to/from subaccount | `src/commands/subaccounts.rs` |

## Key abstractions

| Type | File | Description |
|------|------|-------------|
| `FillsArgs` | `src/commands/account.rs` | Args for time-bounded fill queries |
| `TimeRangeArgs` | `src/commands/account.rs` | Start/end time args for ledger and funding queries |
| `ClassTransferArgs` | `src/commands/transfers.rs` | Args for spot-to-perp and perp-to-spot transfers |
| `SendArgs` | `src/commands/transfers.rs` | Args for USDC send with destination address |
| `TransferArgs` | `src/commands/subaccounts.rs` | Args for subaccount USDC transfer with direction |
| `SpotTransferArgs` | `src/commands/subaccounts.rs` | Args for subaccount spot token transfer |

## Address selectors for transfers

Transfer recipients and protocol object addresses must be explicit `0x` addresses. Local account aliases are not substituted for:

- `--to` in `transfer send` and `transfer spot-send`
- `--destination` in `transfer withdraw`
- `--vault` in vault operations
- `--validator` in staking
- `--builder` in builder operations

The `--subaccount` field on subaccount transfers is an acting-account selector (accepts aliases), not a generic transfer recipient.

## Time format

Time-bounded account history commands accept RFC3339 timestamps and epoch milliseconds:

```bash
hyperliquid account fills 0x... --start 2026-05-01T00:00:00Z --end 2026-05-02T00:00:00Z
hyperliquid account ledger 0x... --start 1777593600000 --end 1777680000000
```

## Entry points for modification

- **Add a new account query**: implement in `src/commands/account.rs`, add clap variant in `src/main.rs`
- **Add a new transfer type**: implement in `src/commands/transfers.rs`, add EIP-712 typed data struct, wire signing
- **Change time parsing**: modify `parse_time_millis` in `src/commands/account.rs`
