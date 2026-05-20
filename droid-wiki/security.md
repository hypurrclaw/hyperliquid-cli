# Security

`hyperliquid-cli` signs financial actions and stores private keys. This page is a working summary of the threat model and the code-level defenses. Read `SECURITY.md` at the repo root for the disclosure policy.

## Threat model summary

| Threat | Defense |
|--------|---------|
| Plaintext private key leaks via stdout/stderr/logs | Hidden prompts (`rpassword`), no echo, never logged. Exceptions are explicit: `wallet export` and `api-wallet create` print exactly once on a deliberate path. |
| Plaintext key stored on disk | Encrypted SQLite (`src/db.rs`) with AES-256-GCM, key material in the OS keychain |
| Untrusted remote text injected into terminal | `src/response_sanitization.rs` strips ANSI/control sequences, prefixes with `[untrusted remote data]` |
| Hostile JSON payload exhausts memory | `src/input_hardening.rs` clamps file size (1 MiB), depth (64), key count (4096), string length (64 KiB) |
| Path-traversal via `--payload-file` | `FilePolicy` validates path components, rejects `..` and unrelated absolute paths |
| Accidental mainnet mutation | `--dry-run`, confirmation prompts, `-y` only for deliberate automation; mainnet `schedule cancel-all` prompts even with `-y` |
| Malicious release binary | SHA-256 verification in `install.sh` and `hyperliquid update`; release assets carry their own checksum file |
| Withdrawal abuse by automation | API/agent wallets cannot withdraw by protocol design (`approveAgent` scope) |
| Secret in CI artifacts | `scripts/pre-release-check.sh` and `task release:check` scan for local-only artifacts (QA wallets, `.qa/` metadata, password files) |
| Mixed signer / acting-account contexts | Order safety hardening plumbs `--on-behalf-of` through lookups + dry-run + live submission (v0.11.0) |
| Raw payload silently bypassing validation | `RawPayloadPolicy` is fail-closed by default; live raw-payload requires explicit allowlist |

## Wallet secrets

- Secrets enter the CLI through hidden prompts (`rpassword`) or env vars. They are never echoed.
- The SQLite account DB is encrypted with AES-256-GCM. Encryption keys live in the OS keychain (`keyring` crate) by default. Tests and headless systems can supply a passphrase-derived key via `HYPERLIQUID_ACCOUNT_KEY_PASSPHRASE`.
- The OWS vault path is `~/.hyperliquid` (or `HYPERLIQUID_OWS_VAULT_PATH`). Unlock uses `OWS_PASSPHRASE` for unattended use.
- Generated API/agent wallet keys are printed exactly once at create time, in JSON or pretty form. Treat them like any hot trading key.

## Encrypted on-disk format

`ENCRYPTION_VERSION = "v1"`. Each record has a unique nonce. Keys are domain-separated with `b"hyperliquid-cli account encryption passphrase v1"` for the passphrase-derived KDF path.

## Sanitization boundary

`labelled_untrusted_text(s)` in `src/response_sanitization.rs` is the single helper that surfaces remote text safely. Every error mapper that propagates an exchange or HTTP-layer string routes through it. Tests in `tests/security_contracts.rs` enforce the label.

## Confirmation gating

| Risk | Confirmation |
|------|--------------|
| Read-only | none |
| Funds movement | required on live mainnet unless `-y` |
| Irreversible | required regardless of `-y` for select actions |
| Mainnet `schedule cancel-all` | required (even with `-y`) |

The exact policy for each command is in `src/command_catalog.json` under `confirmation`.

## CI security workflow

`.github/workflows/security.yml` runs on every PR. It is intentionally narrow — it does not replace a full audit. Use `task release:check` locally before tagging.

## Reporting

See `SECURITY.md` at the repo root for the responsible-disclosure process. Do not report security issues in public issues or PRs.

## See also

- [systems/signing-and-wallets](systems/signing-and-wallets.md)
- [systems/input-hardening](systems/input-hardening.md)
- [systems/update-and-release](systems/update-and-release.md)
- [background/design-decisions](background/design-decisions.md)
