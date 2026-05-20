# `install.sh`

The installer is a POSIX shell script that downloads a release artifact, verifies its SHA-256 checksum, and places the binary in `~/.local/bin/hyperliquid` (or `$BIN_DIR`).

## Usage

```bash
curl -fsSLO https://raw.githubusercontent.com/hypurrclaw/hyperliquid-cli/main/install.sh
sh install.sh
hyperliquid --version
```

For unattended environments:

```bash
sh install.sh --json --quiet
```

## What it does

1. Detect platform via `uname -ms` and map to a release asset name.
2. Resolve the latest release tag from `https://api.github.com/repos/hypurrclaw/hyperliquid-cli/releases/latest` (or `HYPERLIQUID_CLI_VERSION` if pinned).
3. Download the asset tarball and its `.sha256` companion.
4. Verify the checksum.
5. Extract the binary and copy it to `BIN_DIR/hyperliquid` (default `~/.local/bin`).
6. Make the binary executable.

## Environment overrides

| Variable | Purpose |
|----------|---------|
| `HYPERLIQUID_CLI_REPO=OWNER/REPO` | Alternate source repo |
| `HYPERLIQUID_CLI_VERSION=v0.11.0` | Pinned version |
| `BIN_DIR=/path/to/bin` | Alternate install location |

## Flags

| Flag | Effect |
|------|--------|
| `--json` | Emit progress as JSON lines for programmatic consumption |
| `--quiet` | Suppress non-error output |

## Failure modes

- Mismatched checksum aborts the install before touching `BIN_DIR`.
- Missing asset for the platform exits non-zero with a clear message.
- A pre-existing `BIN_DIR/hyperliquid` is overwritten; back up first if you need to roll back without redownloading.

## See also

- [systems/update-and-release](../systems/update-and-release.md) — the in-binary `hyperliquid update` path mirrors this logic.
- [security](../security.md) — checksum verification and supply-chain notes.
- The release artifacts are produced by `.github/workflows/release.yml`.
