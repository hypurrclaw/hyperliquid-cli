# Tooling

## Taskfile

`Taskfile.yml` defines the canonical targets. Run with the `task` runner if available; fall back to the raw cargo commands otherwise.

| Task | Equivalent |
|------|------------|
| `task fmt` | `cargo fmt --check` |
| `task clippy` | `cargo clippy -- -D warnings` |
| `task test` | `cargo test` |
| `task contracts` | Schema/registry/dry-run/output characterization tests |
| `task build` | `cargo build --release --bin hyperliquid` |
| `task bind` | Build release + symlink `~/.local/bin/hyperliquid` |
| `task qa:matrix` | `scripts/qa-command-matrix.sh` against the bound binary |
| `task qa:matrix:strict` | QA matrix with `HL_QA_STRICT_SKIPS=1` |
| `task qa:registry-rollout` | Registry rollout gate checks |
| `task qa:registry-canary-plan` | Manual canary plan under `.qa/` |
| `task release:check` | Pre-release secret/artifact gate |
| `task ci` | All of the above |

## QA scripts

| Script | Purpose |
|--------|---------|
| `scripts/qa-command-matrix.sh` | Broad dry-run sweep of the command surface |
| `scripts/qa-registry-rollout-gates.sh` | Gate checks for registry rollout |
| `scripts/pre-release-check.sh` | Local-only secret/artifact gate |

QA sweeps default to dry-run for unsafe commands. Funded-live QA requires `HL_ENABLE_FUNDED_LIVE_QA=1` and out-of-repo credentials.

## CI workflows

| Workflow | File | Purpose |
|----------|------|---------|
| CI | `.github/workflows/ci.yml` | fmt + clippy + tests on PRs |
| Release | `.github/workflows/release.yml` | Build and publish artifacts on tag |
| Security | `.github/workflows/security.yml` | Security scan |

## Dependabot

`.github/dependabot.yml` enables Cargo updates. Bumps for `hypersdk`, `alloy`, `rand`, `rpassword`, and `sha2` have all landed via this path.

## Build script

`build.rs` embeds packaged defaults at compile time:

- `DEFAULT_BUILDER_ADDRESS`
- `DEFAULT_BUILDER_FEE_RATE`
- `DEFAULT_REFERRAL_CODE`

These are overridable at runtime via the corresponding `HYPERLIQUID_DEFAULT_*` env vars.

## Tmp space (Cursor / sandboxes)

When macOS scratch space is constrained:

```bash
mkdir -p .tmp
TMPDIR="$PWD/.tmp" CARGO_TARGET_DIR="$PWD/target" task qa:matrix
```

`cargo clean` is safe between attempts.

## See also

- [development-workflow](development-workflow.md)
- [testing](testing.md)
- [systems/update-and-release](../systems/update-and-release.md)
