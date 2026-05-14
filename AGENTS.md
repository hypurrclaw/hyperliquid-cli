# Contributor Guide for Agents

## Project Overview

`hyperliquid-cli` is a Rust command-line interface for Hyperliquid market data, trading, wallet management, watch modes, and agent-first automation. The binary is `hyperliquid`.

## Rules

- Use `hypersdk` library when you can
- To orchestrate work use `compound engineering plugin/skills` when you can
- think of yourself as orchestrator of sub-agents

## Architecture

- `src/main.rs` — clap entry point, global flags, command dispatch, network/client setup.
- `src/errors.rs` — structured `CliError` variants and exit-code mapping.
- `src/output/` — pretty/table/JSON rendering, color theme, JSON projection.
- `src/commands/` — one module per command domain.
- `src/auth.rs`, `src/config.rs`, `src/ows.rs` — wallet/config/account storage.
- `src/watch.rs` — streaming/watch support.
- `tests/` — integration tests with `assert_cmd` and `wiremock`.

## Development Setup

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

For repeatable command-surface QA, prefer:

```bash
task bind
task qa:matrix
```

If Task is not installed, run `./scripts/qa-command-matrix.sh` directly after building and binding the desired `hyperliquid` binary. The matrix should use the QA wallet and dry-run mutating commands unless a dedicated funded-live test explicitly opts into side effects.

When the user asks for a "live test", "actual testnet run", "test against testnet", or similar wording, clarify whether they want the dry-run QA matrix or explicitly authorized funded testnet execution. Only run funded-live `--testnet` actions when the operator has explicitly opted in and `HL_ENABLE_FUNDED_LIVE_QA=1` is set for the command environment. Credentials must come from outside the repository path; do not auto-discover repository-local keystores, password files, account databases, OWS vaults, or `.qa/` metadata. Use small, reversible funded actions where possible, record redacted artifacts under an ignored local artifact directory, and finish with cleanup checks for `orders open`, `positions list`, and portfolio/vault state. Use dry-run for commands that are unsafe, irreversible, not explicitly authorized for funded-live execution, or explicitly requested as dry-run, and label those cases clearly as dry-run or live-gated.
When running QA from Cursor or another sandboxed environment, pin build scratch space to the repo if macOS temp space is constrained:

```bash
mkdir -p .tmp
TMPDIR="$PWD/.tmp" CARGO_TARGET_DIR="$PWD/target" task qa:matrix
```

If a build fails with `No space left on device`, check `df -h` and `du -sh target`; `cargo clean` is safe for clearing reproducible Cargo artifacts before rerunning the same command.

Use the mission `services.yaml` commands when working inside a Factory mission.

## Coding Conventions

- Keep financial values as `rust_decimal::Decimal`; do not introduce floats for prices, sizes, or amounts.
- Preserve structured exit codes: clap usage exits `2`, auth exits `10`, rate limits exit `11`, unavailable API/network exits `12`, unsupported assets/DEXes exit `13`.
- Every data command should support `--format pretty|table|json`.
- JSON keys must remain stable, snake_case, and safe for agents to parse.
- Pretty output may use ANSI color; table and JSON output must remain uncolored.
- Never log, print, commit, or store plaintext private keys.

## Terminology and Selector Semantics

- Use "local signing account" for encrypted local private-key records, and "selected signer" for the key used to sign an authenticated action.
- Use "API wallet" and "agent wallet" only for delegated Hyperliquid trading keys approved by a master account. These can trade for the master account but cannot withdraw.
- Use "OWS wallet" for wallets managed by the Open Wallet Standard backend. OWS is the only managed wallet lifecycle backend for `hyperliquid-cli` — wallet creation, import, and listing flow through the OWS vault (default `~/.hyperliquid` or `HYPERLIQUID_OWS_VAULT_PATH` when set). The `--ows-signer` flag selects a specific OWS wallet by name or id for signing; explicit private-key, Foundry keystore, and stored local-account signing paths still exist outside OWS. When no explicit signer is specified, commands auto-detect the first OWS wallet with a Hyperliquid account. Direct `0x` addresses passed via `--ows-signer` require a resolved wallet for live signing; without a wallet, only identity previews are possible. Priority bids and headless OWS approval remain local-only/unsupported until separately designed.
- Use "protocol user address" or `USER` for public account-data lookup targets.
- Use `ACCOUNT_SELECTOR` for inputs that may accept a stored account alias, stored account id, or `0x` address.
- Use `*_ADDRESS` for explicit protocol object addresses such as transfer recipients, vaults, validators, and builders. Do not resolve local account aliases for these fields.
- Treat acting-account selectors such as `orders --on-behalf-of` and `subaccount transfer --subaccount` as their own documented selector class. They may resolve a subaccount/vault context for that signed action, but they do not imply aliases are safe for transfer recipients or other `*_ADDRESS` fields.
- When schema metadata disagrees with README prose or examples, agents should treat schema `input_kind`, risk, dry-run, and confirmation metadata as authoritative.

## Testing Instructions

- Add unit tests beside focused logic in `src/`.
- Add CLI behavior tests in `tests/` using `assert_cmd`.
- Mock Hyperliquid HTTP with `wiremock` when tests need API responses.
- Use isolated `HOME`, `XDG_CONFIG_HOME`, and `XDG_DATA_HOME` in tests that touch user state.
- Verify JSON output with `serde_json` or another Rust-native JSON parser.
- Keep test layers distinct: unit tests own pure logic and private helpers; integration tests own CLI process behavior, prompts, auth boundaries, stdout/stderr routing, and mocked API behavior; QA matrix covers broad installed-binary compatibility.
- Prefer shared helpers in `tests/support/mod.rs` for isolated command envs, opt-in account/passphrase setup, and common WireMock fixtures, but keep domain-specific mock behavior local when it makes a test's API contract clearer.

## Agent-First Output Contract

Agents should prefer:

```bash
hyperliquid --format json --select coin,price mids
hyperliquid --format json --results-only perps list
hyperliquid --format json --max-results 5 perps list
hyperliquid --format json schema orders create
```

When adding or changing commands, ensure `--select` filters JSON output and `--results-only` remains harmless for commands that already return bare arrays or objects.
Use `--dry-run` for mutating-command validation when side effects are not intended.
Effective output format precedence is: explicit `--format pretty|table|json`, then `HYPERLIQUID_FORMAT=pretty|table|json`, then agent/non-TTY defaults. `HYPERLIQUID_AGENT=1` and non-TTY stdout default one-shot commands to JSON so agents can parse output even when `--format json` is omitted.
Watch output in JSON/agent contexts must be bounded with `--max-ticks` or `HYPERLIQUID_WATCH_MAX_TICKS`; subscribe streams should use `--max-events` and/or `--idle-timeout-ms`.
Raw payload metadata is exposed as `raw_payload` in schema `x-hyperliquid` metadata. Treat `dry_run_only` as fail-closed for live execution until a command is explicitly allowlisted and tested.
Remote API/protocol strings surfaced in errors are untrusted data. Keep the `[untrusted remote data]` label and sanitization boundary intact when adding new exchange or HTTP error paths.
