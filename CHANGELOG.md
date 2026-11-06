# Changelog

## Unreleased

Command-line behavior:

- A group without an action (`hyperliquid orders`) and bare `hyperliquid` print help on stdout and exit `0`. `-h` and `--help` print the same help at every depth, and a closed pipe (`| head`) ends quietly.
- `-v`/`--version` works at every depth. With JSON output it prints `{"version":"X"}`.
- Added global `--non-interactive` and `--no-color`. `CI`, `HYPERLIQUID_AGENT`, JSON output, and `--payload-file -` never prompt. Boolean `HYPERLIQUID_*` variables are validated, and an invalid value exits `2`.
- Usage errors show one suggestion, the real usage line, and `hyperliquid <path> --help` for the nearest valid command. `-fjson` and `-ftable` are honored, and an explicit `--format` wins over `HYPERLIQUID_FORMAT`.
- Help summaries are imperative and ordered by task, the root help lists environment variables, and every state-changing command ends with a `Notes:` block (risk, dry-run, confirmation, testnet behavior). Singular group names and `ls`/`list` work as hidden aliases.
- Non-interactive runs that need confirmation fail with `Can't run <command> without confirmation in non-interactive mode.` and name `--dry-run` and, when supported, `--yes`. `wallet export --yes` and `setup --yes` satisfy confirmation in machine contexts.
- A panic prints `Unexpected error in hyperliquid <command>. This is a bug in hyperliquid-cli.` with the version and report URL, and exits `1`.

## 0.11.1 - 2026-05-26

Highlights:

- Aligned order builder-fee handling with Hyperliquid builder-code semantics: explicit builder fees are rejected for spot buys, spot sells remain supported, and packaged default builder fees are skipped where they cannot apply.
- Clarified spot builder-fee applicability in English, Chinese, Korean, Japanese, and internal contributor docs.

## 0.11.0 - 2026-05-17

Highlights:

- Hardened order lifecycle safety for `--on-behalf-of` across cancel, cancel-all, modify, TP/SL, TWAP, and scheduled cancel flows so acting-account selectors are carried into lookups, dry-run previews, and live `vaultAddress` submissions.
- Added prompt-gated safety for mainnet scheduled cancel-all actions, including `--yes` bypass support for intentional automation.
- Clarified signer versus acting-account selector semantics in public docs and machine-readable schemas.
- Preserved Hypersdk / Alloy 1 signer compatibility while allowing app-level Alloy 2 helpers and refreshed dependency pins for `hypersdk`, `alloy`, `rand`, `rpassword`, and `sha2`.
- Refreshed QA command matrix fixtures and bounded subscription output checks for agent-safe automation.
- Aligned release packaging and pre-release health checks for Linux, macOS, and Windows artifacts, plus self-update asset lookup for the supported Linux/macOS tarballs.

## 0.1.0 - 2026-05-15

Initial public release of `hyperliquid-cli`.

Highlights:

- Agent-first JSON, schema, projection, and bounded stream output contracts.
- Open Wallet Standard local encrypted wallet support for setup, import, signing, and account selection.
- Market data, perps, spot, orders, transfers, subaccounts, staking, vaults, borrow/lend, referrals, builder fee, and feedback command coverage.
- Safe-by-default dry-run previews, confirmation gates, structured errors, and stable exit codes.
- Release packaging for Linux, macOS, and Windows with checksum verification.
