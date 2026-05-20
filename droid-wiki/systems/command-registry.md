# Command registry

`hyperliquid-cli` keeps a typed inventory of every command — what it does, what risk it carries, whether it supports dry-run, whether it accepts a raw payload, and what confirmation gating applies. That inventory drives the `hyperliquid schema` agent surface and is the parity layer for a gradual migration away from the legacy clap-only dispatch in `src/main.rs`.

## Purpose

- Source of truth for command metadata that agents read (`schema`).
- Parity check for the in-flight registry rollout: command behavior on the new path must match the legacy dispatch before any signed or fund-moving family is migrated.
- Guard rails on raw payload submission, confirmation policy, and dry-run policy.

## Key files

| File | Purpose |
|------|---------|
| `src/command_catalog.json` | Editable JSON catalog (~3,491 lines). The source of truth in Phase 1. |
| `src/command_registry.rs` | `CommandRegistry::from_embedded_catalog()` parses the catalog into typed `CommandContract` records. |
| `src/command_metadata.rs` | `CatalogCommandMetadata` and `CatalogArgMetadata` shape; argument normalization. |
| `src/command_handlers.rs` | `HandlerBinding` enum that points each catalog entry at its runtime handler. |
| `src/commands/schema.rs` | Renders `CommandContract` records for `hyperliquid schema`. |
| `docs/registry-rollout-policy.md` | Stage gates for registry-routed live execution. |

`CommandRegistry::load()` is the single entry point; it embeds the catalog with `include_str!` so the binary ships with the catalog baked in.

## CommandContract shape

```text
CommandContract {
    command:        String           // "orders create"
    command_path:   Vec<String>      // ["orders", "create"]
    aliases:        Vec<String>
    group:          String           // "orders"
    description:    String
    auth_required:  bool
    dangerous:      bool
    lifecycle:      Lifecycle
    risk:           Risk             // safe | funds_movement | irreversible
    mutability:     Mutability
    dry_run:        DryRunPolicy     // not_applicable | supported | dry_run_only
    raw_payload:    RawPayloadPolicy
    confirmation:   ConfirmationPolicy
    transport:      Vec<Transport>
    ows_signer:     OwsSupport
    output_contract:OutputContract
    handler:        HandlerBinding
    one_of_required:Vec<Vec<String>>
    inputs:         Vec<InputContract>
}
```

Agents that build live execution paths should consult schema metadata over README prose. Per `AGENTS.md`, "when schema metadata disagrees with README prose or examples, agents should treat schema `input_kind`, risk, dry-run, and confirmation metadata as authoritative."

## Phase 1 authority decision

`src/command_registry.rs` exports a constant that documents the current rollout phase:

```rust
pub const PHASE1_AUTHORITY_DECISION: &str =
    "Phase 1 keeps src/command_catalog.json as the editable catalog and emits CLI schemas from CommandRegistry until the registry becomes the source file.";
```

This is intentionally inert at runtime. It exists so that PR reviewers, schema characterization tests, and the registry rollout gates have a single string to assert on.

## Rollout policy

`docs/registry-rollout-policy.md` defines seven stages for migrating a command family from legacy dispatch to registry-routed execution:

1. hidden/internal registry
2. read-only default
3. testnet mutating canary
4. mainnet dry-run comparison
5. mainnet opt-in
6. mainnet default
7. legacy removal

Each migration must declare a rollback mode (`legacy-child`, `legacy-dispatch`, or `fail-closed`) and is rolled back when the registry and legacy paths disagree on signer, query address, network, asset, amount, destination, OIDs, or action type.

`scripts/qa-registry-rollout-gates.sh` enforces the CI-side gate.

## Schema discovery

```bash
hyperliquid --format json schema
hyperliquid --format json schema orders create
hyperliquid --format json --select command,description schema orders
```

Schema output is JSON with the snake_case fields above. Agents typically use the schema first, then plan a command, dry-run it, and only then execute live. See [features/schema-discovery](../features/schema-discovery.md).

## Entry points for modification

- To add a new command: add an entry to `src/command_catalog.json`, register a `HandlerBinding` in `src/command_handlers.rs`, and wire the clap subcommand in `src/main.rs`/`src/cli_runtime.rs`. Update characterization tests via `HYPERLIQUID_UPDATE_CONTRACTS=1 task contracts`.
- To change a command's risk or dry-run policy: edit `command_catalog.json` and rerun `task contracts`.
- To migrate a family to registry-routed execution: follow `docs/registry-rollout-policy.md` step-by-step and add the rollback declaration in the child issue.
