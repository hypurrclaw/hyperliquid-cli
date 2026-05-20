# Staking, vaults, and borrow/lend

Three DeFi command families share a similar shape: each one inspects on-chain state, supports dry-run for any mutating action, and gates live mainnet mutations behind confirmation.

## Key files

| File | Purpose |
|------|---------|
| `src/commands/staking.rs` (~1,323 lines) | Validator delegate/undelegate, staking deposit/withdraw, claim rewards |
| `src/commands/vaults.rs` (~1,102 lines) | Vault list/get/deposit/withdraw with lockup awareness |
| `src/commands/borrowlend.rs` (~870 lines) | Borrow/lend reserve list, supply, withdraw |

## Staking

| Command | Purpose |
|---------|---------|
| `staking validators` | List validators with stake/commission |
| `staking history [USER]` | Delegator history |
| `staking summary [USER]` | Aggregate stake + pending rewards |
| `staking delegate --validator <ADDRESS> --amount` | Delegate stake |
| `staking undelegate --validator <ADDRESS> --amount` | Schedule undelegation |
| `staking deposit --amount` | Move USDC from perp to staking |
| `staking withdraw --amount` | Move USDC from staking to perp |
| `staking claim-rewards` | Claim pending rewards (no-op when none) |

`--validator` is a `*_ADDRESS` field; local aliases are not resolved.

## Vaults

| Command | Purpose |
|---------|---------|
| `vault list [USER]` | Vaults the address has stake in |
| `vault get <VAULT_ADDRESS>` | Vault details (lockup, equity, leader) |
| `vault deposit --vault <ADDRESS> --amount` | Deposit USDC into a vault |
| `vault withdraw --vault <ADDRESS> --amount` | Withdraw from a vault (subject to lockup) |

Lockup may reject a withdraw — that's a normal account-state outcome and surfaces as `CliError::Unsupported` (exit 13) with a sanitized message.

## Borrow / lend

| Command | Purpose |
|---------|---------|
| `borrowlend list` | Reserve rates per asset |
| `borrowlend supply --token --amount` | Supply liquidity (CoreWriter action) |
| `borrowlend withdraw --token --amount` | Withdraw supplied liquidity |

The supply/withdraw actions are routed through `CoreWriter` action types in `hypersdk`.

## Dry-run shape

All mutating commands emit the standard `DryRunEnvelope` (see [dry-run](dry-run.md)) with command-specific args. Example for delegation:

```json
{
  "command": "staking delegate",
  "would_execute": "delegate_stake",
  "args": {
    "validator": "0x...",
    "amount": "0.001",
    "is_undelegate": false
  },
  "signer": "0x...",
  "acting_as": null
}
```

## Stateful outcomes

Agents should not treat these account-state results as bugs:

- `staking claim-rewards` returning "no rewards to claim"
- `vault withdraw` blocked by lockup
- `borrowlend supply` failing due to reserve caps

They are valid no-ops or rejections that the protocol enforces. See [background/pitfalls](../background/pitfalls.md).

## Entry points for modification

- To add a new validator-scoped read, extend `staking.rs` with a new `info` query and a renderer.
- To change vault lockup display, update `src/commands/vaults.rs` and the JSON rendering helpers.
- To support a new borrow/lend token, ensure the asset is in the metadata cache; the `--token` flag is resolved through `AssetResolver`.
