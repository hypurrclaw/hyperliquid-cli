# Tooling

## Build system

Cargo is the build system. Key commands:

```bash
cargo build                 # debug build
cargo build --release       # release build
cargo test                  # all tests
cargo clippy -- -D warnings # lint with warnings as errors
cargo fmt --check           # check formatting
```

## Taskfile

An optional [Task](https://taskfile.dev) file at `Taskfile.yml` provides convenience commands:

```bash
task build                   # release build of hyperliquid binary
task test                    # run test suite
task clippy                  # lint
task fmt                     # check formatting
task bind                    # build + link ~/.local/bin/hyperliquid
task qa:matrix               # build + run QA command matrix
task contracts               # run contract characterization tests
task ci                      # all quality gates (fmt, clippy, test, contracts, qa)
task release:check           # pre-release repository check
```

## Linting and formatting

- **rustfmt**: `cargo fmt --check` — standard Rust formatting
- **clippy**: `cargo clippy -- -D warnings` — treats all clippy warnings as errors
- No custom lint plugins or configuration beyond defaults

## CI pipeline

Three GitHub Actions workflows at `.github/workflows/`:

| Workflow | Trigger | Jobs |
|----------|---------|------|
| `ci.yml` | Push to main, PRs | Build, tests, contract tests, registry rollout gates, clippy, OWS tests, formatting |
| `release.yml` | Tag `v*`, manual dispatch | Multi-platform builds (linux x86_64, linux arm64, macos x86_64, macos arm64), archive packaging, checksum generation |
| `security.yml` | PRs, push to main, weekly Mondays | `cargo audit` for dependency vulnerabilities, Gitleaks secret scanning |

## Code generation

- **Contract fixtures**: `HYPERLIQUID_UPDATE_CONTRACTS=1 task contracts` updates JSON fixtures under `tests/fixtures/contracts/`

## QA scripts

| Script | Purpose |
|--------|---------|
| `scripts/qa-command-matrix.sh` | Sweeps the full command surface against the QA wallet (dry-run for mutating commands) |
| `scripts/pre-release-check.sh` | Verifies no local-only secrets/artifacts are in the release |
| `scripts/qa-registry-rollout-gates.sh` | Validates registry rollout policy compliance |

## Installer

`install.sh` downloads the release archive for the current platform, verifies SHA-256, and copies `hyperliquid` into `~/.local/bin`.
