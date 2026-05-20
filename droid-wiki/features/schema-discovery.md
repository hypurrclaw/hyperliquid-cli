# Schema discovery

`hyperliquid schema` emits machine-readable command contracts so agents can plan against authoritative metadata rather than scraping README prose.

## Usage

```bash
hyperliquid --format json schema                        # entire surface
hyperliquid --format json schema orders create          # one command
hyperliquid --format json --select command,description schema orders
hyperliquid --format json --max-results 5 schema
```

## Key files

| File | Purpose |
|------|---------|
| `src/commands/schema.rs` | Renders `CommandContract` records |
| `src/command_catalog.json` | Editable source of truth |
| `src/command_registry.rs` | Typed loader |

## Fields agents care about

For each command the schema exposes:

| Field | Meaning |
|-------|---------|
| `command`, `command_path`, `aliases`, `group` | Identity |
| `description` | One-line summary |
| `auth_required`, `dangerous` | Whether a signer is required and whether the action is high-risk |
| `lifecycle`, `risk`, `mutability` | Lifecycle category, risk class, mutation flag |
| `dry_run` | `not_applicable` \| `supported` \| `dry_run_only` |
| `raw_payload` | `not_supported` \| `supported` \| `dry_run_only` |
| `confirmation` | `none` \| `required` \| `required_unless_yes` |
| `transport` | HTTP / WebSocket usage |
| `ows_signer` | OWS support level |
| `output_contract` | Shape of the success payload |
| `inputs` | Per-arg metadata, including `input_kind` (USER, ACCOUNT_SELECTOR, *_ADDRESS, etc.) |
| `one_of_required` | Mutually exclusive arg groups |

## Authority

Treat schema metadata as authoritative when it disagrees with README prose or examples. Specifically, `input_kind`, `risk`, `dry_run`, and `confirmation` are the contract — README prose may lag.

## Catalog-driven

`src/command_catalog.json` (~3,491 lines) is the editable source. `CommandRegistry::from_embedded_catalog()` deserializes it; `src/commands/schema.rs` renders the result. To add or change a command's metadata, edit the catalog and run:

```bash
HYPERLIQUID_UPDATE_CONTRACTS=1 task contracts
```

This refreshes the characterization test fixtures so reviewers can see the schema diff.

## Entry points for modification

- To add a new schema field, extend `CommandContract` and the catalog deserialization, then update characterization tests.
- To filter the schema output programmatically, use `--select` and `--max-results` instead of post-processing.

See also: [systems/command-registry](../systems/command-registry.md), [agent-output-contract](agent-output-contract.md).
