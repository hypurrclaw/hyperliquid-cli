# Command registry

`hyperliquid-cli` keeps a typed inventory of every command — what it does, what risk it carries, whether it supports dry-run, whether it accepts a raw payload, and what confirmation gating applies. That inventory drives the `hyperliquid schema` agent surface, the `Notes:` block in help, and the policy pipeline every command runs through. It is generated from the clap tree plus each command's `CommandSpec`; nothing is written by hand per command.

## Purpose

- Source of truth for command metadata that agents read (`schema`).
- Parity check for the in-flight registry rollout: command behavior on the new path must match the legacy dispatch before any signed or fund-moving family is migrated.
- Guard rails on raw payload submission, confirmation policy, and dry-run policy.

## Key files

| File | Purpose |
|------|---------|
| `src/command_spec.rs` | `CliCommand` (clap `Args`, `CommandSpec`, `run`), `ArgSpec`, `GroupSpec`, `Entry`, and the clap tree builder. |
| `src/commands/mod.rs` | `entries()` registers every command (help order) with one line; `groups()` names the groups. |
| `src/command_registry.rs` | `CommandRegistry::get()` builds the typed `CommandContract` records from the clap tree and each `CommandSpec`. |
| `src/cli_runtime.rs` | The one pipeline: validate, stream bounds, payload, dry-run, confirmation, API override, run. |
| `src/commands/schema.rs` | Renders `CommandContract` records for `hyperliquid schema`. |
| `docs/registry-rollout-policy.md` | Stage gates for registry-routed live execution. |

Each domain module owns its commands: the `Args` struct, `const SPEC: CommandSpec` (risk, dry-run, confirmation, testnet behavior, stream bound, input kinds, schema descriptions) and `run` live together. `CommandRegistry::get()` builds the registry once.

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
    one_of_required:Vec<Vec<String>>
    inputs:         Vec<InputContract>
}
```

Agents that build live execution paths should consult schema metadata over README prose. Per `AGENTS.md`, "when schema metadata disagrees with README prose or examples, agents should treat schema `input_kind`, risk, dry-run, and confirmation metadata as authoritative."

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

- To add a new command: implement `CliCommand` in its domain module (`Args`, `SPEC`, `run`) and add one `Entry::of::<..>()` line to `commands::entries` in `src/commands/mod.rs`. Update characterization fixtures via `HYPERLIQUID_UPDATE_CONTRACTS=1 cargo test --test contract_fixtures`.
- To change a command's risk or dry-run policy: edit its `SPEC` and regenerate the fixtures the same way.
- To migrate a family to registry-routed execution: follow `docs/registry-rollout-policy.md` step-by-step and add the rollback declaration in the child issue.
