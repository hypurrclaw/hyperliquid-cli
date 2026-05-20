# Patterns and conventions

This page is a working summary of the conventions in `AGENTS.md` and `CONTRIBUTING.md`. Read both for the authoritative versions.

## Coding conventions

- **Financial values are `rust_decimal::Decimal`.** Never introduce floats for prices, sizes, or amounts. `serde-str` is enabled so JSON serialization is a string, avoiding float drift.
- **Stable exit codes.** Clap usage exits `2`, auth exits `10`, rate limits exit `11`, unavailable API/network exits `12`, unsupported assets/DEXes exit `13`. See [reference/exit-codes](../reference/exit-codes.md).
- **All data commands support `--format pretty|table|json`.** When adding a new command, register a renderer for all three.
- **Snake_case JSON keys.** Keys are stable for agent consumption.
- **Pretty may color; table and JSON must not.** ANSI never leaks into JSON or table output.
- **Never log, print, commit, or store plaintext private keys.** The single exception is `api-wallet create` and `wallet export`, which print exactly once on a deliberate path.
- **Schema metadata is authoritative** when README prose disagrees with `input_kind`, `risk`, `dry_run`, or `confirmation`.

## Selector semantics

`AGENTS.md` documents the selector classes; here is the working summary:

| Class | Where used | Resolves aliases? |
|-------|------------|-------------------|
| OWS wallet account | `--account`, `account add/ls/set-default/remove` | yes |
| Selected signer | `--ows-signer`, `--private-key`, `--keystore`, `--account` | yes |
| API/agent wallet | `api-wallet *`, signing as the agent | yes |
| OWS wallet | `--ows-signer`, `wallet *` | yes |
| Protocol user (`USER`) | `account *` reads | yes (for ergonomics) |
| `*_ADDRESS` | Transfer recipient, vault, validator, builder | **no** — explicit address only |
| Acting-account selector | `orders --on-behalf-of`, `subaccount transfer --subaccount` | resolves a subaccount/vault context only for that signed action |

Mixing these up is a common bug class. When in doubt, check the schema's `input_kind` field for the argument.

## Use hypersdk

Use the `hypersdk` library when it covers the Hyperliquid behavior you need. Avoid reimplementing actions or signing on top of raw HTTP unless the SDK does not cover the case (and document why in the PR).

## Use the registry rollout policy

For any signed or fund-moving command-family migration, follow `docs/registry-rollout-policy.md`. Declare a rollback mode (`legacy-child`, `legacy-dispatch`, or `fail-closed`) and gate the migration through testnet canary → mainnet dry-run comparison → opt-in → default.

## Untrusted remote data

Wrap exchange and HTTP-layer strings in `labelled_untrusted_text` (`src/response_sanitization.rs`) before display. The `[untrusted remote data]` label must persist into artifacts. Never trust remote text for control flow.

## Test layers

Unit tests own pure logic. Integration tests own CLI process behavior, prompts, auth boundaries, stdout/stderr routing, and mocked API behavior. The QA matrix owns broad installed-binary compatibility. Keep tests in their layer. See [testing](testing.md).

## Pull request hygiene

- Run `cargo fmt --check` and `cargo clippy -- -D warnings` locally.
- Refresh contract fixtures (`task contracts`) when command metadata changes.
- Keep QA credentials and local-only artifacts out of the diff (`task release:check`).
- Note any live-mutating behavior and the exact dry-run or mocked evidence used.
