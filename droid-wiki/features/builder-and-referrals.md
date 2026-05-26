# Builder fees and referrals

Builder fees let a Hyperliquid builder (a third-party UI/app) collect a fee on actions a user routes through them. The user must approve a maximum fee rate. Referrals let an existing account refer new accounts to share fee credit.

## Key files

| File | Purpose |
|------|---------|
| `src/commands/builder.rs` (~834 lines) | `builder max-fee`, `builder approved`, `builder approve` |
| `src/commands/referral.rs` (~569 lines) | `referral state`, `referral set`, `referral register` |

## Builder

| Command | Purpose |
|---------|---------|
| `builder max-fee <USER>` | Show the max approved builder fee for a user |
| `builder approved <USER>` | List all builders approved by a user |
| `builder approve --builder <BUILDER_ADDRESS> --max-fee-rate <RATE>` | Approve or update a max fee for the selected signer |

`--builder` is a `*_ADDRESS`; aliases are not resolved.

`--max-fee-rate` accepts a decimal percentage (`0.001%`) or basis points; the planning helper normalizes it. The signed action is `approveBuilderFee`.

Per-order builder fees apply to both sides of perpetual orders and to spot sells. Hyperliquid does not apply builder fees to spot buys, so `orders create` rejects explicit builder-fee flags on spot buy orders and skips packaged default builder fees for that path.

## Referrals

| Command | Purpose |
|---------|---------|
| `referral state [USER]` | Inspect referral state (code, referrer, totals) |
| `referral set --code <CODE>` | Register a referral code for the signer (one-time) |
| `referral register --code <CODE>` | Set the referrer code for the signer (one-time) |

`referral set` and `referral register` are one-time stateful actions; subsequent calls fail.

## Packaged defaults

At build time, the binary can embed three values via `build.rs`:

| Env (runtime override) | Build-time value |
|------------------------|------------------|
| `HYPERLIQUID_DEFAULT_BUILDER_ADDRESS` | `DEFAULT_BUILDER_ADDRESS` |
| `HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE` | `DEFAULT_BUILDER_FEE_RATE` |
| `HYPERLIQUID_DEFAULT_REFERRAL_CODE` | `DEFAULT_REFERRAL_CODE` |

`hyperliquid setup` validates these defaults before wallet creation. Invalid or partial builder defaults fail setup so the operator can fix distribution config. The `wallet create / import / import-mnemonic` commands tolerate invalid defaults during wallet provisioning; the error will resurface the next time the relevant builder/referral command is invoked. See [setup-wizard](setup-wizard.md).

## Dry-run shape

```json
{
  "command": "builder approve",
  "would_execute": "approve_builder_fee",
  "args": {
    "builder": "0x...",
    "max_fee_rate": "0.001"
  },
  "signer": "0x..."
}
```

## Entry points for modification

- To change the default builder fee cap shipped with the binary, update `build.rs` (compile-time) or document the `HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE` env override.
- To extend referral commands with more lifecycle states (e.g., partial revoke), add the action to `src/commands/referral.rs` and update the schema fixture.
