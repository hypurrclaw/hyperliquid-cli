# Subaccounts

Subaccounts split a master account's balance into separate domains. Each one has its own positions, orders, and balance. Trading on a subaccount uses the master signer with `--on-behalf-of <SUBACCOUNT_ADDRESS>` or the dedicated subaccount-transfer commands.

## Key file

`src/commands/subaccounts.rs`.

## Subcommands

| Command | Purpose |
|---------|---------|
| `subaccount list [USER]` | List subaccounts for an address |
| `subaccount create --name` | Create a new subaccount under the selected signer |
| `subaccount transfer --subaccount <ADDRESS> --amount [--from-master]` | Move USDC between master and a subaccount |
| `subaccount spot-transfer --subaccount <ADDRESS> --token --amount [--from-master]` | Move a spot token between master and a subaccount |

`subaccount transfer --subaccount` is an **acting-account selector** that resolves a subaccount context for the signed action. It does not imply local-alias resolution for any `*_ADDRESS` field (see [glossary](../overview/glossary.md)).

## Dry-run shape

```json
{
  "command": "subaccount create",
  "would_execute": "create_subaccount",
  "args": {"name": "market-maker-1"},
  "signer": "0x...",
  "acting_as": null,
  "vault_address": null
}
```

`create_subaccount` is irreversible — the action plan's reversibility is `Irreversible` and `live_submission` is `ValidateConfirmSignSubmit`.

## Read paths

`account subaccounts [USER]` (in [account-and-portfolio](account-and-portfolio.md)) is the public read; it accepts a wallet name/id/address selector.

## Entry points for modification

- To add a new subaccount-scoped action, follow the `--on-behalf-of` plumbing pattern from [orders](orders.md) so the `vaultAddress` reaches both the dry-run preview and the live submission.
- To extend the spot-transfer to new tokens, update the asset resolver in `src/commands/mod.rs` and the validation helpers in `src/commands/subaccounts.rs`.
