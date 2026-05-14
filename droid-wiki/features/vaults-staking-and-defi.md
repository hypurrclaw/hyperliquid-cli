# Vaults, staking, and DeFi

Active contributors: Sayo

Staking, vault management, borrow/lend reserve actions, builder approvals, priority auctions, and referrals.

## Commands

### Staking

| Command | Description | Implementation |
|---------|-------------|---------------|
| `staking summary <ADDR>` | Staking summary for an address | `src/commands/staking.rs` |
| `staking validators` | List validators | `src/commands/staking.rs` |
| `staking rewards <ADDR>` | Staking rewards | `src/commands/staking.rs` |
| `staking history <ADDR>` | Delegation and withdrawal history | `src/commands/staking.rs` |
| `staking delegate --validator <ADDR> --amount <HYPE>` | Delegate stake | `src/commands/staking.rs` |
| `staking undelegate --validator <ADDR> --amount <HYPE>` | Undelegate stake | `src/commands/staking.rs` |
| `staking deposit --amount <HYPE>` | Deposit to staking | `src/commands/staking.rs` |
| `staking withdraw --amount <HYPE>` | Withdraw from staking | `src/commands/staking.rs` |
| `staking claim-rewards` | Claim staking rewards | `src/commands/staking.rs` |
| `staking link initiate --user <ADDR>` | Initiate staking-link for fee discount | `src/commands/staking.rs` |
| `staking link finalize --user <ADDR>` | Finalize staking-link | `src/commands/staking.rs` |

Staking uses Solidity struct definitions (`TokenDelegate`, `CDeposit`, `CWithdraw`, `LinkStakingUser`) for EIP-712 typed data signing. Amounts are in HYPE with `HYPE_WEI_SCALE = 100_000_000`. Staking link dry-runs include permanence/control warnings.

### Vaults

| Command | Description | Implementation |
|---------|-------------|---------------|
| `vault list [--kind] [--user] [--limit] [--sort]` | List vault summaries | `src/commands/vaults.rs` |
| `vault search <QUERY>` | Search vaults by name/leader/address | `src/commands/vaults.rs` |
| `vault get <ADDR>` | Get vault details | `src/commands/vaults.rs` |
| `vault positions <ADDR>` | List vault positions | `src/commands/vaults.rs` |
| `vault deposit --vault <ADDR> --amount <USDC>` | Deposit to a vault | `src/commands/vaults.rs` |
| `vault withdraw --vault <ADDR> --amount <USDC>` | Withdraw from a vault | `src/commands/vaults.rs` |

Vault transfers use `VaultTransferActionKind` for deposit/withdraw classification. Both are `PartiallyReversible`.

### Borrow/lend

| Command | Description | Implementation |
|---------|-------------|---------------|
| `borrowlend rates` | All borrow/lend rates | `src/commands/borrowlend.rs` |
| `borrowlend get <TOKEN>` | Single token reserve info | `src/commands/borrowlend.rs` |
| `borrowlend user <ADDR>` | User borrow/lend state | `src/commands/borrowlend.rs` |
| `borrowlend supply <TOKEN> --amount <AMOUNT>` | Supply to borrow/lend pool | `src/commands/borrowlend.rs` |
| `borrowlend withdraw <TOKEN> --amount <AMOUNT>\|--max` | Withdraw from pool | `src/commands/borrowlend.rs` |

Supply and withdraw use Hyperliquid's `CoreWriter` action (`action_id=15`) rather than the standard exchange action. The `--max` flag encodes `wei=0` in the CoreWriter action shape.

### Builder, priority, and referrals

| Command | Description | Implementation |
|---------|-------------|---------------|
| `builder max-fee --user <ADDR> --builder <ADDR>` | Query one approved builder fee | `src/commands/builder.rs` |
| `builder approved --user <ADDR>` | List approved builders for a user | `src/commands/builder.rs` |
| `builder approve --builder <ADDR> --max-fee-rate <PERCENT>` | Approve or update builder fee rate | `src/commands/builder.rs` |
| `prio status` | Priority auction status | `src/commands/prio.rs` |
| `prio bid --max <HYPE> --ip <IP> [--slot 0-4]` | Place a priority bid | `src/commands/prio.rs` |
| `referral set [CODE]` | Set referral code | `src/commands/referral.rs` |
| `referral register <CODE>` | Register your own referral code | `src/commands/referral.rs` |
| `referral status` | Show referral status | `src/commands/referral.rs` |
## Key abstractions

| Type | File | Description |
|------|------|-------------|
| `DelegateArgs` | `src/commands/staking.rs` | Args for delegate/undelegate (validator + amount) |
| `AmountArgs` | `src/commands/staking.rs` | Args for staking deposit/withdraw (amount only) |
| `VaultTransferArgs` | `src/commands/vaults.rs` | Args for vault deposit/withdraw (vault address + amount) |
| `ActionArgs` | `src/commands/borrowlend.rs` | Args for borrow/lend supply/withdraw (token + amount or --max) |
| `ApproveArgs` | `src/commands/builder.rs` | Args for builder fee approval (builder address + max fee rate) |
| `BidArgs` | `src/commands/prio.rs` | Args for priority bid (max HYPE, IP, slot) |
## Entry points for modification

- **Add a new staking action**: add Solidity struct, implement signing in `src/commands/actions.rs`, add handler in `src/commands/staking.rs`
- **Change default builder/referral behavior**: start with `src/commands/builder.rs` and `src/commands/referral.rs`; environment variables override the compile-time defaults.
