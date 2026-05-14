# Contributing

Thanks for helping improve `hyperliquid-cli`.

## Development Setup

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

Optional repeatable QA tasks:

```bash
task bind
task qa:matrix
task qa:registry-rollout
```

Use dry-run or mocked coverage unless a maintainer explicitly opts into funded testnet QA.

## Project Conventions

- Keep financial values as `rust_decimal::Decimal`; do not introduce floats for prices, sizes, or amounts.
- Every data command should support `--format pretty|table|json`.
- JSON keys should be stable, snake_case, and safe for agents to parse.
- Preserve structured exit codes documented in `AGENTS.md`.
- Prefer `hypersdk` when it covers the needed Hyperliquid API behavior.
- Never print, log, commit, or store plaintext private keys.

## Tests

- Add unit tests beside focused pure logic in `src/`.
- Add CLI behavior tests under `tests/` with `assert_cmd`.
- Mock Hyperliquid HTTP with `wiremock` when tests need API responses.
- Use isolated `HOME`, `XDG_CONFIG_HOME`, and `XDG_DATA_HOME` for tests touching user state.
- Verify JSON output with `serde_json`.

## Generated Artifacts

If command metadata changes, refresh the contract fixtures and review the diff:

```bash
HYPERLIQUID_UPDATE_CONTRACTS=1 task contracts
```

## Pull Requests

Before opening a PR:

1. Run the relevant Rust tests and formatting checks.
2. Update README/docs/schema/agent artifacts when command behavior changes.
3. Keep QA credentials and local artifacts out of the diff.
4. Note any live-mutating behavior and the exact dry-run or mocked evidence used.
