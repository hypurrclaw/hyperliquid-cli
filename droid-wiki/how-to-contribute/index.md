# How to contribute

This section covers the development workflow, testing strategy, debugging, coding conventions, and tooling for `hyperliquid-cli`.

## Before you start

1. Read the [architecture overview](../overview/architecture.md) and [glossary](../overview/glossary.md) to understand the codebase structure
2. Run `cargo build && cargo test` to confirm your environment works
3. Pick up an issue or propose a change in a focused PR

## PR expectations

- Run `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test` before pushing
- Update the [tool catalog](../../src/command_catalog.json) if you add or change a command
- Run `HYPERLIQUID_UPDATE_CONTRACTS=1 task contracts` to update characterization contracts
- Keep QA credentials and local artifacts out of the diff
- Note any live-mutating behavior and the exact dry-run or mocked evidence used

## Definition of done

- Code compiles without warnings or errors
- All tests pass (unit tests, integration tests, contract characterization tests)
- CLI interface respects existing output contracts (pretty/table/JSON, field selection, exit codes)
- New commands have entries in `src/command_catalog.json` with correct `lifecycle`, `risk`, `dry_run`, and `confirmation` metadata
