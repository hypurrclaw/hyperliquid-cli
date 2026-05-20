# Deployment

`hyperliquid-cli` ships as a single binary in three distribution forms:

1. Prebuilt release archives (Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64)
2. `cargo install --path . --bin hyperliquid`
3. `install.sh` (curl + SHA-256 verify + copy to `BIN_DIR`)

## Release packaging

`.github/workflows/release.yml` triggers on tags matching `v*.*.*`. For each platform it:

1. Builds the release binary with `cargo build --release --bin hyperliquid`.
2. Archives the binary into a platform-named tarball (`hyperliquid-vX.Y.Z-<target>.tar.gz`) or zip on Windows.
3. Computes SHA-256 into a `.sha256` companion file.
4. Uploads the artifact and checksum to the GitHub release.

v0.11.0 specifically tightened the workflow so release packaging is self-contained (commit `da17cb8`, "ci: keep release packaging self-contained"), and v0.1.0 dropped the inline pre-release check from the release job to simplify the artifact pipeline.

## Pre-release gate

Locally:

```bash
task release:check
# → scripts/pre-release-check.sh
```

The script scans the working tree for accidental local-only artifacts (QA wallets, `.qa/` metadata, password files, OWS vaults) and fails if any are present. Run it before tagging.

## Self-update path

`hyperliquid update` resolves the latest release, downloads the matching asset, verifies SHA-256, and atomically swaps the running binary. The current implementation supports the Linux/macOS tarball assets used by `install.sh`. See [systems/update-and-release](systems/update-and-release.md).

## Install locations

| Path | Source |
|------|--------|
| `~/.local/bin/hyperliquid` | Default `install.sh` and `task bind` |
| `target/release/hyperliquid` | `cargo build --release` output |
| Anywhere in `$PATH` | `cargo install` honors `CARGO_INSTALL_ROOT` |

## Cross-compilation

The release workflow uses GitHub-hosted runners (Linux, macOS Intel, macOS Apple Silicon, Windows). Locally, `cargo build --release --target <triple>` works for cross-compilation as long as the target toolchain is installed.

## Reproducibility

The release artifacts pin all dependency versions through `Cargo.lock` (committed). Reproducing a tagged build requires the matching Rust toolchain (1.93+) and the lockfile.

## See also

- [systems/update-and-release](systems/update-and-release.md)
- [applications/install-script](applications/install-script.md)
- [security](security.md)
