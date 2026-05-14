# Deployment

## CI/CD pipeline

Three GitHub Actions workflows handle continuous integration, releases, and security scanning.

### CI (`ci.yml`)

Runs on every push to `main` and every pull request:

- Build (`cargo build`)
- All tests (`cargo test`)
- Contract characterization tests (5 test suites)
- Registry rollout policy gate (`scripts/qa-registry-rollout-gates.sh`)
- Clippy (`cargo clippy -- -D warnings`)
- OWS tests (`cargo test --lib --bins`)
- OWS clippy (`cargo clippy --lib --bins -- -D warnings`)
- Formatting (`cargo fmt --check`)

### Release (`release.yml`)

Triggered by version tags (`v*`) or manual dispatch. Builds for 5 targets:

| Target | Runner |
|--------|--------|
| `x86_64-unknown-linux-gnu` | ubuntu-latest |
| `aarch64-unknown-linux-gnu` | ubuntu-24.04-arm |
| `x86_64-apple-darwin` | macos-13 |
| `aarch64-apple-darwin` | macos-14 |
| `x86_64-pc-windows-msvc` | windows-latest |

Each build:
1. Runs `scripts/pre-release-check.sh` to verify no local-only secrets/artifacts
2. Builds the release binary with `--locked`
3. Packages the binary with README and LICENSE into a `.tar.gz` on Unix targets or `.zip` on Windows
4. Generates a SHA-256 checksum
5. Smoke-tests the archive by running `hyperliquid --version`
6. Uploads artifacts

### Security (`security.yml`)

Runs on PRs, pushes to main, and weekly on Mondays:

- **cargo-audit**: checks dependencies for known vulnerabilities
- **Gitleaks**: scans the full git history for secrets

## Pre-release check

`scripts/pre-release-check.sh` validates the repository before release:

- No local-only secret or artifact paths in the working tree
- No QA credentials in committed files
- Required files present (LICENSE, README.md)

## Installer

`install.sh` is a shell script that:
1. Detects the current OS and architecture
2. Downloads the appropriate release archive from GitHub Releases
3. Verifies the SHA-256 checksum
4. Extracts and copies `hyperliquid` to `~/.local/bin`

Override defaults with environment variables:

```bash
HYPERLIQUID_CLI_REPO=OWNER/REPO HYPERLIQUID_CLI_VERSION=v0.1.0 BIN_DIR=/usr/local/bin sh install.sh
```

## Environments

The CLI itself has no server-side deployment. It connects to:

| Environment | API base URL |
|-------------|-------------|
| Mainnet | `https://api.hyperliquid.xyz` |
| Testnet | `https://api.hyperliquid-testnet.xyz` |

Override all networks with `HYPERLIQUID_API_BASE_URL`, or override individual networks with `HYPERLIQUID_MAINNET_API_BASE_URL` / `HYPERLIQUID_TESTNET_API_BASE_URL`.
