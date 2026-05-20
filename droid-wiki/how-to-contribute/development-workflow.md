# Development workflow

## Standard cycle

```bash
git checkout -b feat/short-description

cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check

# When command metadata changes:
HYPERLIQUID_UPDATE_CONTRACTS=1 task contracts

# Local pre-release check (only relevant before tagging):
task release:check
```

## Branch model

- `main` is the released branch; `develop` is the integration branch.
- Feature branches target `develop`. Release prep merges `develop` → `main` and tags.

## Editable surfaces

- `src/command_catalog.json` is the editable source for command metadata. Edit it, then refresh fixtures.
- Command handlers live in `src/commands/`. Each domain is one module (or a small sub-tree for `orders/`).
- `src/main.rs` defines the clap surface; `src/cli_runtime.rs` does per-command dispatch.

## Local install

```bash
cargo install --path . --bin hyperliquid
# or
task bind   # symlinks target/release/hyperliquid into ~/.local/bin
hyperliquid --version
```

## PR template

See `.github/PULL_REQUEST_TEMPLATE.md`. Key checkboxes:

- Rust tests + fmt/clippy pass locally
- README / schema / agent artifacts updated when command behavior changes
- QA credentials and local artifacts kept out of the diff
- Live-mutating behavior is noted with dry-run/mocked evidence

## Release process

1. Update `CHANGELOG.md` with highlights.
2. Bump version in `Cargo.toml`.
3. Open PR `develop` → `main`.
4. After merge, tag `vX.Y.Z` and let `.github/workflows/release.yml` package artifacts.
5. Confirm `scripts/pre-release-check.sh` and `task release:check` are green.

See also: [tooling](tooling.md), [systems/update-and-release](../systems/update-and-release.md).
