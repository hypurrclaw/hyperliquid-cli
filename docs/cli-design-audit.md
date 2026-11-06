# CLI design audit

Checked against [Vercel's CLI Design Guidelines](https://vercel.com/design/cli) in November 2026. Four passes ran the built binary under an isolated `HOME` and read the source: help and parsing, output streams, errors and exit codes, and consent and safety. They logged 124 findings. This file records what changed, where the repository contract overrides a guideline, and what is still open.

The guideline's own rule decides conflicts: "Public contracts change deliberately, with compatibility and evidence." Anything in `AGENTS.md` (JSON keys, exit codes, `--format`, non-TTY JSON defaults, watch bounds) stays as it is until a major version.

## Applied

| Area | Change |
| --- | --- |
| Help | Group without an action and bare `hyperliquid` print help on stdout and exit 0. `-h` equals `--help`. `--version`/`-v` works at any depth and prints `{"version":"X"}` under JSON. Global options sit under their own heading. Summaries are imperative and ordered by task. Root help lists every environment variable. Every state-changing command ends with a `Notes:` block generated from the catalog (risk, dry-run, confirmation, testnet). Options without help text get it from the catalog. A test parses every help example. |
| Parsing | `-fjson` and an explicit `--format` beat `HYPERLIQUID_FORMAT` (no hand-scanned argv). Invalid boolean `HYPERLIQUID_*` values exit 2. Usage errors show one suggestion, the real usage line, and the nearest `--help`. Hidden `ls`/`list` and singular/plural aliases. |
| Modes | One module (`src/mode.rs`) decides prompting. `--non-interactive`, `CI`, `HYPERLIQUID_AGENT`, JSON, and `--payload-file -` never prompt. Prompts go to stderr through `src/prompt.rs`, and end of input is missing input, never a default. |
| Output | Color precedence `--no-color` > `NO_COLOR` > `FORCE_COLOR` > per-stream TTY, `TERM=dumb` plain. Tables are tab-separated on a pipe. JSON is compact on a pipe. Empty results print the header on stdout and `No <items>.` on stderr. Truncation by `--max-results` is announced on stderr. The timing footer only appears on a terminal. Cells escape control, bidi, and zero-width characters. Epoch times render as ISO 8601 in human output. Update notices come after the result, once a day, never in CI, agent, or JSON runs, and never block the command. |
| Errors | JSON envelope adds `reason`, `retryable`, `retry_after`, `next` (with `when` for approval-gated steps), and `data`. Human errors use `✗` on a styled terminal and `Error:` otherwise, with recovery commands. Network errors name the host and cause. A 5xx is an API failure, not a network one. Reads retry with backoff; mutations never do, and an unknown mutation outcome says so (`unknown_outcome`). One asset suggestion at most. A panic prints a bug report line and exits 1; a closed pipe exits 0. |
| Consent | Confirmation errors give a runnable `--dry-run` command and the `--yes` rerun marked for human approval, with secret options stripped. `--yes` works on every command that has it (derived from the clap tree). Order and cancel-all prompts show network, signer, acting account, builder fee, and counts. `wallet delete` needs the typed wallet name on a terminal. A stale default wallet is an error, not a silent fallback. |
| Files and secrets | Config and account-store writes are atomic, `0600`, and locked. Broken config fails loudly. Read-only commands don't create the vault. Key-holding types redact `Debug`. `HYPERLIQUID_KEYSTORE_PASSWORD` replaces passing the password in argv. Exchange per-leg errors carry the `[untrusted remote data]` label. |
| Streams | SIGINT, SIGTERM, and SIGHUP exit 130, 143, and 129 with the terminal restored and no partial JSON line. Reconnects emit a `stream_gap` warning. |
| Partial work | Batch, scale, TP/SL, and cancel-all with failed legs print every leg in one document and exit 15. Bracket failures after the entry is live say what is resting or filled and how to protect it. |
| Schema | The catalog lists every option clap accepts, and a test fails on drift. |

## Recorded exceptions

| Guideline | This CLI | Why it stays | Pinned by |
| --- | --- | --- | --- |
| `--json` is the only format selector | `--format pretty\|table\|json`, `-f` | `AGENTS.md` contract | `tests/output_contracts.rs` |
| camelCase JSON mirroring the API | snake_case keys | `AGENTS.md` contract | contract fixtures |
| Exit codes 1 and 2 by class | 10 auth, 11 rate limit, 12 unavailable, 13 unsupported, 14 stale, 15 partial | `AGENTS.md` contract | `tests/error_exit_codes.rs` |
| JSON errors on stderr, stdout empty | JSON error document on stdout | Agents parse stdout today; moving it breaks them silently | `tests/error_exit_codes.rs` |
| Piping stdout never selects JSON or blocks prompts | Non-TTY stdout defaults to JSON, and JSON never prompts | `AGENTS.md` contract | `tests/output_formatting.rs` |
| Confirmation required is exit 2 | Exit 13 with `reason: "confirmation_required"` | Existing agents branch on 13 | `tests/security_contracts.rs` |
| Declining a confirmation exits 130 | Wallet cancels exit 0, order and transfer declines exit 2 | Changing decline codes breaks scripts | `tests/wallet_management.rs`, `tests/orders_create.rs` |
| Destructive commands need `--yes` when non-interactive | Vault, staking, borrow/lend, API wallet, leverage, margin, cancel, and modify run without confirmation | Adding a gate breaks unattended scripts; needs a major version | `command_catalog.json` |
| `-f` means `--follow`; `--watch` is banned | `-f` is `--format`; `--watch`/`-w` and `--max-ticks` | `AGENTS.md` contract | `tests/watch_subscribe.rs` |
| Unknown `--fields` lists valid ones | Unknown `--select` fields are omitted | Documented `--select` behavior | `tests/output_contracts.rs` |
| Prompts need a TTY | Piped stdin may answer prompts in human mode; EOF is missing input | Secret-on-stdin import and the interactive test suite depend on it | `tests/setup_wizard.rs` |
| Verbs nest (`orders cancel --all`, `positions leverage update`) | Fused names like `cancel-all`, `update-leverage`, `set-default` | Renames need a deprecation cycle | command inventory fixture |
| `setup --yes` covers only setup | `setup --yes` also approves the default builder | Existing behavior; separating it changes what `--yes` grants | `tests/setup_wizard.rs` |

## Still open

- `Unsupported input:` and `Configuration error:` prefixes stay in many headlines. The guideline wants the failed action and object first.
- Read retries cover requests through `http_api`. Reads through the hypersdk client (`perps list`, `status`) don't retry.
- Pretty tables ignore terminal width. There is no compact-record fallback at 40 or 80 columns.
- No `result_schema` per command and no secret-field marking in `schema` output.
- No `completion <SHELL>` command and no `help search`.
- Validation reports the first problem for most commands. Batch files list every bad field; other inputs don't.
- The clap-generated `help` row still reads "Print this message or the help of the given subcommand(s)".
- `api-wallet create` prints the generated key after submitting the approval. If stdout is closed at that moment, the key is lost while the approval stands.
- Update self-replace verifies a SHA-256 from the same release, not a signature.
