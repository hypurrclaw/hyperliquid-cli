# Glossary

| Term | Definition |
|------|-----------|
| **Local signing account** | An encrypted private-key record stored in the local SQLite database (`accounts.db`). Managed by `account add`, `account ls`, `account set-default`, and `account remove`. |
| **Selected signer** | The key used to sign authenticated actions. Chosen from CLI flags (`--private-key`, `--keystore`, `--account`, `--ows-signer`), environment variables, or stored defaults. |
| **Protocol user address** | A public Hyperliquid user address used for info queries (fills, portfolio, order status). |
| **Master account** | The protocol owner account that can approve API wallets and own subaccounts. |
| **API wallet / Agent wallet** | A delegated Hyperliquid trading key approved by a master account. Can trade but cannot withdraw. |
| **OWS wallet** | A wallet managed by the Open Wallet Standard backend. OWS is the only wallet lifecycle backend; wallet creation, import, and listing flow through the OWS vault at `~/.hyperliquid` (or `HYPERLIQUID_OWS_VAULT_PATH`). Signing can also use explicit private keys, Foundry keystores, or stored local signing accounts. |
| **OWS signer** | A signer selected via `--ows-signer` by wallet name, id, or `0x` address. |
| **Subaccount** | A protocol subaccount controlled by a master account. |
| **Protocol address** | A literal on-chain address for a recipient, vault, validator, builder, or similar object. Never resolved from local aliases. |
| **ACCOUNT_SELECTOR** | Input class accepting stored account alias, stored account id, or `0x` address. Used for selecting a local signer. |
| **USER** | Input class accepting `0x` user address or stored account selector for public lookups. |
| **`*_ADDRESS`** | Input class accepting only explicit `0x` protocol addresses. No alias resolution. |
| **Hypersdk** | The Rust SDK crate (`hypersdk`) providing Hyperliquid API types, HTTP/WebSocket clients, signing, and chain primitives. |
| **Tool catalog** | The JSON file at `src/command_catalog.json` that defines every command's contract: risk, lifecycle, auth requirements, dry-run policy, input schemas. |
| **Command registry** | The in-memory typed representation of the tool catalog, loaded at startup via `CommandRegistry::load()`. |
| **Dry run** | `--dry-run` mode that validates and previews supported mutating commands without submitting them to the exchange. |
| **HIP-3** | Hyperliquid Improvement Proposal 3 — the DEX qualification format (`dex:TOKEN`) for perpetual market lookups. |
| **Agent-first output contract** | JSON mode defaults when `HYPERLIQUID_AGENT=1` or stdout is not a TTY. Includes stable snake_case keys, `--select` field projection, and structured error envelopes. |
| **EIP-712 signing** | Hyperliquid uses EIP-712 typed data signing for L1 exchange actions. The CLI constructs typed data payloads via alloy's `TypedData` resolver. |
| **CoreWriter** | A Hyperliquid L1 action type (`action_id=15`) used for borrow/lend supply and withdraw operations that don't fit the standard exchange action path. |
