# Design decisions

## Agent-first output contract

The CLI is built around the agent loop. Every data command speaks stable JSON with snake_case keys, supports `--select`, `--results-only`, `--max-results`, and exposes its own contract via `schema`. The decision to make JSON a first-class default (with non-TTY stdout auto-flipping to JSON) is what lets an LLM tool a single binary without scraping.

## Catalog-driven schemas

`src/command_catalog.json` is the editable source of command metadata, and `src/command_registry.rs` deserializes it into typed `CommandContract` records. The catalog approach decouples metadata edits from Rust changes — a metadata-only PR is a JSON diff plus a fixture regen. Schemas are emitted from the typed registry, so agents always see the same shape regardless of which path emits it. See `PHASE1_AUTHORITY_DECISION` in [systems/command-registry](../systems/command-registry.md).

## Three output formats

Pretty, table, and JSON share a single `OutputFormat` enum (`src/output/mod.rs`) and a uniform projection pipeline. Pretty is for humans on a TTY; table is for human-readable bordered output without colors; JSON is for agents and pipelines. The same data goes through the same renderer so adding a new command does not require duplicating output logic three times.

## OWS-first wallet backend

Open Wallet Standard (via `ows-lib`) is the only **managed lifecycle** backend. Creation, import, listing, and default selection flow through the OWS vault at `~/.hyperliquid`. Explicit private-key flag/env/config and Foundry keystore signing paths exist outside OWS for power users and unattended automation, but the human-facing `wallet *` commands talk to OWS exclusively. This avoids two parallel notions of "default wallet" and consolidates lifecycle bugs in one place.

## Dual Alloy 1 / Alloy 2 pinning

`hypersdk` 0.2 re-exports Alloy 1's `PrivateKeySigner`. The CLI uses Alloy 2 for app-level EIP-712 helpers and `TypedData`. Both versions share the same `alloy-dyn-abi` / `alloy-core` typed-data representation, so they coexist in one binary. `src/lib.rs` ends with `extern crate alloy_signer_local_v1 as _;` to keep the v1 `keystore` feature anchored. Both v1 entries can be removed when hypersdk moves to Alloy 2. The choice trades a little dependency-tree complexity for keystore parity today rather than a deferred upgrade. See [reference/dependencies](../reference/dependencies.md).

## `rust_decimal` everywhere

Every price, size, and amount uses `rust_decimal::Decimal` with the `serde-str` feature. No floats. The cost is occasional ergonomic friction (e.g., explicit `Decimal::from_str`); the win is no rounding surprises and JSON values that serialize as strings.

## Fail-closed raw payload policy

The default `RawPayloadPolicy` is fail-closed: a command must be explicitly allowlisted before raw-payload submission is honored for live execution. Until then, raw payloads work for dry-run only. This converts a "raw payload silently bypasses validation" footgun into a documented allowlist.

## Untrusted remote data sanitization

Any string returned by the protocol or HTTP layer passes through `labelled_untrusted_text` in `src/response_sanitization.rs`, which strips ANSI/control sequences and prefixes the text with `[untrusted remote data]`. The label persists into artifacts so reviewers know which strings came from untrusted sources. Errors that surface remote text without this label are treated as security defects.

## Structured exit codes

`CliError` maps every error variant to a code in 0–15. The mapping is part of the agent contract and is asserted by `tests/error_exit_codes.rs`. Adding a new variant requires choosing a code, not inventing one. See [reference/exit-codes](../reference/exit-codes.md).

## Order safety hardening (v0.11.0)

The `--on-behalf-of` selector is now carried through cancel, cancel-all, modify, TP/SL, TWAP, and scheduled cancel flows. Before v0.11.0, a master-signed cancel on a subaccount could find no matching order or hit the wrong account. The fix plumbs the acting-account context into lookups, dry-run previews, and live `vaultAddress` submission. The companion change is the prompt gate on mainnet `schedule cancel-all` — even with `-y`, that flavor of dead-man's switch is treated as deliberate enough to warrant a stop. See [features/orders](../features/orders.md).

## Registry rollout policy

`docs/registry-rollout-policy.md` defines seven explicit stages between "hidden internal registry" and "legacy removal." Signed and fund-moving migrations cannot start until a stage and rollback mode are named in the migration's child issue. This is intentionally stricter than usual for a CLI; the CLI signs financial actions, so the migration cost-of-error is high.

## See also

- [systems/command-registry](../systems/command-registry.md)
- [overview/architecture](../overview/architecture.md)
- [pitfalls](pitfalls.md)
