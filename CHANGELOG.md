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

Output:

- Color follows `--no-color`, `NO_COLOR`, `FORCE_COLOR`, then per-stream terminal detection (`TERM=dumb` is plain). Pretty output on a pipe has no ANSI codes.
- `--format table` prints a header and tab-separated rows on a pipe and keeps borders on a terminal. JSON is indented on a terminal and compact single-line otherwise; parsed content is unchanged.
- Empty results print the real header on stdout and a `No <items>.` sentence on stderr. `--max-results` truncation in pretty and table output prints `Showing N of M.` on stderr.
- `Completed in` timing prints only for human formats on a terminal, never in JSON, CI, or agent runs.
- Remote text in pretty and table cells is escaped (control, bidi, and zero-width characters show as visible escapes). Epoch-millisecond times show as ISO 8601 UTC in pretty and table output; JSON keeps the numbers.
- Update notices print after the result, at most once a day, never in CI, agent, JSON, or non-terminal runs, or with `NO_UPDATE_NOTIFIER`. The background check no longer delays commands. `hyperliquid update` refuses to replace Homebrew or Cargo installs and prints the matching upgrade command.

Errors:

- JSON errors keep `error`, `category`, and `exit_code` on stdout and add a stable `reason`, plus `retryable`, `retry_after`, `next`, and `data` when they apply. Usage errors move clap's usage block into `data.usage` and the help command into `next`.
- Human errors start with `✗` on a styled terminal and `Error:` otherwise, followed by recovery commands when one applies.
- Network failures name the host and cause (`Couldn't reach api.hyperliquid.xyz. Connection refused.`). A 5xx from the API is reported as an API failure, not a network problem. Rate limits say when to retry.
- Reads retry up to twice on connect errors, timeouts, 5xx, and 429, honoring `Retry-After`. Mutations are never retried. A mutation whose result is unknown (timeout, reset, or 5xx after sending) fails with `reason: "unknown_outcome"` (exit `12`) and points to `orders open` and `account fills`.
- Exchange rejections are no longer misclassified as network failures because their text mentions "connection" or "timeout"; they exit `1` with `reason: "exchange_rejected"`.
- Unknown assets suggest at most one close match, also in `data.suggestion`.
- Remote text is stripped of bidi and zero-width characters and capped at 300 characters.

Trading and consent:

- Per-order exchange errors in create, scale, batch, TP/SL, TWAP, cancel, and modify are labelled `[untrusted remote data]` and sanitized.
- Batch, scale, TP/SL, and cancel-all results with some failed legs print every leg in one document and exit `15`. Bracket failures after the entry is live say whether the entry is resting or filled and how to protect or cancel it.
- Mainnet order and cancel-all prompts show network, signer, acting account, builder fee, and counts before asking. Prompts and setup narration go to stderr. An empty stdin no longer accepts defaults: `setup </dev/null` exits `2` without writing anything.
- `wallet delete` and `account remove` preview the wallet and state that deletion can't be undone; on a terminal they require typing the wallet name. `wallet reset` says it deletes the config file and which defaults it clears.
- `account remove` and `account set-default` without a selector in a run that can't prompt exit `2` with `reason: "missing_input"`.
- A configured default wallet that no longer exists is an error instead of a silent fallback to another wallet.
- Config and account-store writes are atomic, owner-only (`0600`), and locked against concurrent updates. Broken config is reported instead of ignored. Read-only commands no longer create the wallet vault directory.
- `--keystore` reads `HYPERLIQUID_KEYSTORE_PASSWORD`. `--payload-file -` reads multi-line JSON from stdin.
- Dry-run envelopes add `network` and the resolved `signer` (or `signer_warning`); pretty and table dry-runs list the arguments.

Trading:

- Added `orders trailing-stop`, Hyperliquid's native perp trailing stop (live since 2026-09-21). The exchange tracks the mark price and fires a market order when it retraces from its best level by `--retracement-pct` or `--retracement-px`; `--activation-price` delays tracking and `--reduce-only` limits it to closing. It supports `--dex`, `--on-behalf-of`, `--dry-run` (`would_execute: "trailing_stop"`), and signs through every signer. Mainnet prompts before placing and `--testnet` does not; machine contexts need `--yes`. Spot and outcome markets are rejected with exit `2`. The JSON receipt carries `oid`, and `orders open` shows the stop as `TrailingStopMarket`.
- Added `orders modify --always-place`. It sends `a: true` on the modify action, which places the replacement even if the cancel fails and lets trigger orders be modified. Without it the replacement must be a non-trigger ALO order or a non-executable GTC order, and the action is unchanged. Modify dry-runs report `always_place`.
- `orders modify` on a trailing stop now points to `orders trailing-stop` instead of the Hyperliquid app.

Streams:

- `watch`, `subscribe`, and `watch risk` exit `130`, `143`, or `129` on SIGINT, SIGTERM, or SIGHUP, restore the terminal, and never leave a partial JSON line. A closed stdout ends them quietly with exit `0`.
- A dropped subscription connection prints `! Lost connection. Retrying automatically…` on a terminal and emits one `{"type":"warning","reason":"stream_gap"}` line after reconnecting.
- An explicit human format that falls back to JSON lines on a pipe says so on stderr.

Schema:

- `schema` lists every option the CLI accepts, including the previously missing `buy`, `sell`, `outcome-buy`, `outcome-sell`, `orders chase`, `orders bracket`, `orders tpsl`, `borrowlend supply`, and `watch risk` options. A test fails when the catalog and the CLI drift.

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
