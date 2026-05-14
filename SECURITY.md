# Security Policy

## Supported versions

Security updates are provided for the latest released version of `hyperliquid-cli`.

## Reporting a vulnerability

Please report suspected vulnerabilities privately by contacting the maintainers through the repository security advisory flow, or by opening a minimal GitHub issue that does not disclose exploit details or secrets.

Do not include private keys, seed phrases, wallet vault files, passphrases, API tokens, or account databases in reports. If a secret may have been exposed, rotate it before sharing diagnostic context.

## Security expectations

- Wallet material is stored locally in the encrypted Open Wallet Standard vault.
- Mainnet mutations are prompt-gated unless explicitly bypassed with documented flags.
- Use `--dry-run` to inspect side effects before submitting signed actions.
- Remote API/protocol error strings are treated as untrusted data.
