# Lore

Data collected on 2026-05-20.

A short history of how the codebase reached its current shape. Dates come from git log timestamps and tag dates.

## Eras

### Pre-public development (before May 15, 2026)

The codebase developed on `develop`, with an initial empty `main` commit (`c075119`, 2026-05-15) and a corresponding `Initial develop import` (`594c522`). The full command surface, OWS vault, agent-first JSON contract, and registry/catalog were already in place when the repo was made public.

### Initial public release — v0.1.0 (May 15, 2026)

Tagged via PR #5 (`d283cd8`, "Release hyperliquid-cli 0.1.0"). v0.1.0 shipped:

- Agent-first JSON, schema, projection, and bounded stream output contracts.
- Open Wallet Standard local encrypted wallet support for setup, import, signing, and account selection.
- Market data, perps, spot, orders, transfers, subaccounts, staking, vaults, borrow/lend, referrals, builder fee, and feedback command coverage.
- Safe-by-default dry-run previews, confirmation gates, structured errors, and stable exit codes.
- Release packaging for Linux, macOS, and Windows with checksum verification.

In parallel: localized READMEs (`2631b3f`, "docs: add localized readmes") for `zh-CN`, `ja-JP`, `ko-KR`.

### Dependency churn era (May 14-15, 2026)

A burst of dependabot bumps merged in rapid succession:

- `rand` 0.9.4 → 0.10.1 (PR #10)
- `alloy` 1.8.3 → 2.0.4 (PR #9)
- `hypersdk` 0.2.10 → 0.2.11 (PR #8)
- `rpassword` 7.5.1 → 7.5.2 (PR #7)
- `sha2` 0.10.9 → 0.11.0 (`5ab639f`)

The Alloy 2 bump prompted the dual Alloy 1 / Alloy 2 pinning pattern: hypersdk 0.2 still re-exports Alloy 1's `PrivateKeySigner`, so the crate kept `alloy-signer-local` v1 alongside the v2 app-level helpers (`faeabd7`, "fix: keep hypersdk signer compatibility"; `1e051f1`, "fix: anchor alloy v1 signer feature"; `d0d09dc`, "docs: clarify dual alloy signer compatibility").

### Asset id ergonomics (May 15, 2026)

`feat: add asset id decode and search` (commit `626b6f8`, PR #12) added `asset decode <RAW_ID>` and `asset search <QUERY>` so agents and humans could resolve a protocol asset id without separate `info` calls. The follow-up `4eae698` ("fix: reuse asset search metadata") avoided a duplicate metadata fetch.

### Order safety hardening — v0.11.0 (May 16-17, 2026)

PR #14 (`9bf98d6`, merged via `3440982`, "fix: harden order lifecycle safety") plumbed `--on-behalf-of` through:

- order lookups (so cancel / modify / TP/SL resolve the right open-orders set),
- dry-run previews (so `vault_address` is captured), and
- live `vaultAddress` submission.

The same window added prompt-gated safety for mainnet scheduled cancel-all actions (with `--yes` bypass for intentional automation) and clarified signer-vs-acting-account selector semantics in public docs and schemas (`5b43263`, "docs: clarify signer and acting-account selectors").

v0.11.0 was tagged on 2026-05-17 (`dda4069`, "chore: prepare v0.11.0 release").

### Release packaging stabilization (May 17, 2026)

`ci: keep release packaging self-contained` (commit `da17cb8`) tightened `.github/workflows/release.yml` so the release job no longer depends on auxiliary scripts. Combined with `e2dcb24` ("ci: publish friendly release asset names") earlier in the cycle, the release artifact path is now: build → name → SHA-256 → publish, end-to-end inside one workflow.

## Longest-standing files

Files present since the earliest pre-public import and still actively used:

- `src/main.rs` — clap surface
- `src/errors.rs` — `CliError`
- `src/output/mod.rs` — three-format rendering
- `src/commands/orders.rs` — order family root
- `src/auth.rs`, `src/signing.rs` — signer abstractions
- `src/config.rs` — env / config-file plumbing

The `orders/` directory itself is younger than `orders.rs` — args, planning, queries, rendering, and validation were extracted into the sub-module to make the order family navigable.

## Major rewrites

The largest in-flight rewrite is the **command-spine registry migration**. `src/command_catalog.json` is the editable source of truth, `src/command_registry.rs` is the typed loader and parity layer, and `src/command_handlers.rs` carries the `HandlerBinding` enum. Legacy clap dispatch in `src/main.rs` remains the execution authority. The `PHASE1_AUTHORITY_DECISION` constant documents this. The rollout policy lives in `docs/registry-rollout-policy.md`.

## Growth trajectory

Between v0.1.0 (May 15) and v0.11.0 (May 17) — roughly 48 hours — the project saw:

- ~50 commits on `develop`
- 5 dependabot merges
- 1 substantive safety fix (PR #14)
- 1 new feature (`asset decode/search`)
- Self-contained release packaging
- 3 localized READMEs

The version-number cadence reflects rapid v0.x iteration, not user-impacting churn. The agent-first contract, catalog-driven schemas, and OWS-first wallets have all stayed stable across the bumps.

## Deprecated features

None yet — the project is young enough that no public command has been removed. The registry rollout policy treats legacy clap dispatch as still authoritative, so "deprecation" of internal paths is gated by `docs/registry-rollout-policy.md`.

## See also

- [by-the-numbers](by-the-numbers.md)
- [fun-facts](fun-facts.md)
- [background/design-decisions](background/design-decisions.md)
