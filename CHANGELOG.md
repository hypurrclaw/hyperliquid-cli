# Changelog

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
