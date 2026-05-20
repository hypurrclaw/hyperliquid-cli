# Dry-run

`--dry-run` validates and previews a mutating command without sending it. The envelope shape is a public CLI contract.

## Key file

`src/dry_run.rs`. The `ActionPlan` and `DryRunEnvelope` types are intentionally small and internal-facing; each command family decides what `details` to surface.

## DryRunPolicy

The command catalog records each command's dry-run policy:

```rust
pub enum DryRunPolicy {
    NotApplicable,  // read-only command
    Supported,      // dry-run is optional; live also works
    DryRunOnly,     // live execution is fail-closed until allowlisted
}
```

Agents must treat `DryRunOnly` as fail-closed for live execution until the command is explicitly tested and allowlisted.

## ActionPlan

```rust
pub struct ActionPlan {
    would_execute: String,
    kind: ActionKind,            // SignedExchangeAction | LocalStateMutation
    reversibility: ActionReversibility, // Reversible | PartiallyReversible | Irreversible
    live_submission: LiveSubmissionPolicy, // DryRunOnly | ValidateConfirmSignSubmit
}
```

## DryRunEnvelope

The envelope is the stable JSON shape produced when `--dry-run` is set:

```json
{
  "dry_run": true,
  "command": "subaccount create",
  "would_execute": "create_subaccount",
  "args": {"name": "market-maker-1"},
  "signer": null,
  "acting_as": null,
  "vault_address": null
}
```

When a `--payload-json`/`--payload-file` is supplied, the envelope also includes a `payload` field.

The signing context (`signer`, `acting_as`, `vault_address`) is attached via `with_signing_context(DryRunSigningContext::new(...))` so reviewers and agents can verify the right key is about to sign.

## Per-command details

Commands extend the envelope with action-specific fields. Orders, for example, populate `asset_id`, `resolved_asset`, `amount`, `amount_unit`, `limit_px`, `size`, `margin_mode`, `would_execute`. See [orders](orders.md).

## What dry-run does NOT do

- It does not pre-flight against the protocol — there is no `/exchange?dry=true` endpoint. The CLI validates the typed action against schema and metadata cache rules.
- It does not check the live account state (margin, balance, lockup). Those rejections only surface live.
- It does not avoid all rate-limit cost — a dry-run still hits `/info` for metadata.

## Cleanup checks

After a live action that mutates state, agents should run cleanup checks:

```bash
hyperliquid --format json orders open
hyperliquid --format json positions list
hyperliquid --format json account portfolio USER
```

See `SKILL.md` and [pitfalls](../background/pitfalls.md).

## Entry points for modification

- To add a new dry-run field to an existing command, extend the planning helper in `src/commands/<family>/planning.rs` and include the field in the envelope `details`. Update `tests/dry_run_contracts.rs`.
- To introduce a new `ActionKind` or `LiveSubmissionPolicy`, edit `src/dry_run.rs` and update characterization fixtures.

See also: [schema-discovery](schema-discovery.md), [raw-payload](raw-payload.md).
