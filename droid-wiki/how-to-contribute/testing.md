# Testing

The test suite is layered. Unit tests live next to focused logic in `src/`. Integration tests in `tests/` drive the compiled `hyperliquid` binary via `assert_cmd` and mock the Hyperliquid HTTP API with `wiremock`. A separate QA matrix script sweeps the full command surface against a fixture wallet.

## Run everything

```bash
cargo test
task ci    # fmt + clippy + test + contracts + qa
```

## Test layers

| Layer | Owner | Files |
|-------|-------|-------|
| Unit tests | Pure helpers, private logic | Inline `#[cfg(test)] mod tests` in `src/` |
| Integration | CLI process behavior, prompts, stdout/stderr routing, mocked API | `tests/*.rs` (39 files, ~19,541 LOC) |
| Contract characterization | Schema, registry, dry-run, output | `tests/schema_contracts.rs`, `tests/registry_contracts.rs`, `tests/dry_run_contracts.rs`, `tests/output_contracts.rs` |
| Security contracts | Sanitization, exit-code routing, untrusted text | `tests/security_contracts.rs`, `tests/error_exit_codes.rs` |
| QA matrix | Broad installed-binary compatibility | `scripts/qa-command-matrix.sh` (`task qa:matrix`) |

## Shared helpers

`tests/support/mod.rs` exposes helpers for:

- Isolated `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME` directories per test.
- `env_guard` for safely scoping environment-variable mutations.
- Common WireMock fixtures for `/info` and `/exchange`.
- Account and passphrase setup that opts in to the encrypted DB.

Use these helpers whenever a test touches user state; rolling your own ad-hoc env mutation is a common flake source.

## Mocking the API

```rust
use wiremock::{MockServer, Mock, ResponseTemplate};

let server = MockServer::start().await;
Mock::given(...)
    .respond_with(ResponseTemplate::new(200).set_body_json(...))
    .mount(&server)
    .await;
```

Set `HYPERLIQUID_MAINNET_API_BASE_URL` (or `HYPERLIQUID_TESTNET_API_BASE_URL`) to `server.uri()` in the test environment so the binary hits the mock.

## Contract refresh

Some test files are characterization fixtures generated from the runtime. After changing command metadata or dry-run shapes:

```bash
HYPERLIQUID_UPDATE_CONTRACTS=1 task contracts
```

Review the diff carefully — the fixtures are the agent contract.

## QA matrix

`scripts/qa-command-matrix.sh` runs a broad dry-run sweep of the command surface against a fixture wallet. Use it before cutting a release.

```bash
task bind
task qa:matrix
HL_QA_STRICT_SKIPS=1 task qa:matrix:strict   # fail on intentional skips
```

For funded testnet QA, the operator must explicitly set `HL_ENABLE_FUNDED_LIVE_QA=1` and supply credentials from outside the repo path (the script does not auto-discover repo-local keystores).

## Where to add a test

- New command argument logic → integration test in `tests/<domain>_*.rs`
- New error path → `tests/error_exit_codes.rs`
- New schema field → `tests/schema_contracts.rs` (regenerate fixtures)
- New dry-run shape → `tests/dry_run_contracts.rs`
- Pure helper logic → unit test next to the helper in `src/`

See also: [debugging](debugging.md), [tooling](tooling.md).
