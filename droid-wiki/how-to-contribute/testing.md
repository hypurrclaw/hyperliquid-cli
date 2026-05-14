# Testing

## Test layers

| Layer | Location | Framework | Purpose |
|-------|----------|-----------|---------|
| Unit tests | `src/**/*.rs` (inline `#[cfg(test)]`) | Rust built-in | Pure logic, private helpers, error mapping |
| Integration tests | `tests/*.rs` | `assert_cmd` | CLI process behavior, prompts, auth, stdout/stderr routing |
| Contract tests | `tests/command_contracts.rs`, `tests/schema_contracts.rs`, `tests/registry_contracts.rs`, `tests/dry_run_contracts.rs`, `tests/output_contracts.rs` | `assert_cmd` | Command contract parity, schema stability, dry-run output, registry consistency |
| QA matrix | `scripts/qa-command-matrix.sh` | Shell script | Broad installed-binary command surface sweep |

## Running tests

```bash
# All tests
cargo test

# Contract characterization tests only
cargo test --test command_contracts --test schema_contracts --test registry_contracts --test dry_run_contracts --test output_contracts

# Specific test file
cargo test --test cli_integration

# With output
cargo test -- --nocapture
```

## Mocking Hyperliquid HTTP

Integration tests use `wiremock` to simulate API responses. Mock servers are set up per-test with expected request/response pairs. See `tests/cli_integration.rs` and `tests/orders_create.rs` for patterns.

## Isolated test state

Tests that touch user state (config, accounts, wallets) use isolated `HOME`, `XDG_CONFIG_HOME`, and `XDG_DATA_HOME` via `tempfile`. See `tests/support/mod.rs` for shared helpers like `create_isolated_command_env()`.

## QA matrix

```bash
task qa:matrix
```

This builds the release binary, binds it to `~/.local/bin/hyperliquid`, and runs `scripts/qa-command-matrix.sh` which sweeps the full command surface against the QA wallet. Mutating commands are dry-run only unless `HL_ENABLE_FUNDED_LIVE_QA=1` is explicitly set.

## Updating contract tests

When command behavior changes (new args, changed output schema, modified lifecycle/risk metadata):

```bash
HYPERLIQUID_UPDATE_CONTRACTS=1 task contracts
```

This regenerates the JSON contract fixtures under `tests/fixtures/contracts/`. Review the diff carefully — these fixtures are the ground truth for CI parity checks.

## Key test files

| File | Description |
|------|-------------|
| `tests/cli_integration.rs` | Broad CLI behavior and output format tests |
| `tests/wallet_management.rs` | Wallet create, import, list, show, delete tests |
| `tests/orders_create.rs` | Order creation, validation, dry-run tests |
| `tests/config_resolution.rs` | Config file, env var, CLI flag priority tests |
| `tests/error_exit_codes.rs` | Structured exit code verification |
| `tests/security_contracts.rs` | Security boundary tests (input hardening, response sanitization) |
| `tests/support/mod.rs` | Shared test utilities |
