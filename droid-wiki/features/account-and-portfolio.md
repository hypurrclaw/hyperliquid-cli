# Account and portfolio

`account` exposes the public read paths for any protocol user address plus the OWS-managed wallet lifecycle (`account add / ls / set-default / remove`). Read paths accept a `USER` selector (`0x` address, stored wallet alias, or wallet id).

## Key file

`src/commands/account.rs` (~1,904 lines — the second largest source file).

## Read paths

| Command | Purpose |
|---------|---------|
| `account fills [USER] [--start --end --max-results]` | Fill history |
| `account fees [USER]` | Fee schedule and volume context |
| `account rate-limit [USER]` | User rate-limit context |
| `account orders [USER]` | Open orders for a user |
| `account portfolio [USER]` | Portfolio summary |
| `account subaccounts [USER]` | Subaccount list |
| `account portfolio-history [USER]` | Frontend graph/history data |
| `account ledger [USER --start --end]` | Non-funding ledger updates (deposits, withdrawals, transfers) |
| `account funding [USER --start --end]` | User funding payment history |
| `account twap-history [USER]` | TWAP order history |
| `account twap-fills [USER --start --end]` | TWAP slice fills |
| `account abstraction [USER]` | Account-abstraction mode |
| `account abstraction set <MODE>` | Set account-abstraction mode (signed action) |

When `[USER]` is omitted, the command falls back to the selected/default signer's address.

## OWS wallet lifecycle

| Command | Purpose |
|---------|---------|
| `account add [PRIVATE_KEY] [--alias --type --default]` | Add a wallet. The recommended human flow omits PRIVATE_KEY and pastes it at the hidden prompt. Passing it as an argument can leak it to OS process listings and shell history. |
| `account ls` | List wallets in the OWS vault |
| `account set-default <SELECTOR>` | Set the default wallet |
| `account remove <SELECTOR> [-y]` | Remove a wallet (confirmation gated unless `-y`) |

These commands are the same primitives as `wallet add / list / set-default / remove`; `account` keeps them grouped near the rest of the account read paths for discoverability.

## USER selector rules

- `USER` is a **protocol user address** lookup target. It accepts a `0x` address, a stored wallet name, or a wallet id.
- `USER` is **not** the same as a transfer recipient (`*_ADDRESS`) — aliases are intentionally resolved here because the user is looking up their own (or a public) state, not pointing at a recipient.

## Time-range arguments

`TimeRangeArgs` accepts `--start <RFC3339_OR_MS>` and `--end <RFC3339_OR_MS>`. Both are optional; omit them for the protocol's default window. The CLI converts to milliseconds before sending to `/info`.

## Dry-run / mutating

Only `account abstraction set` mutates. Its dry-run envelope captures the requested mode and the signer.

## Entry points for modification

- To add a new public read, route through `http_api::post_info_json` in `src/http_api.rs`, define a renderer in `src/commands/account.rs`, and update the schema fixture via `task contracts`.
- To extend wallet lifecycle commands, prefer modifying the OWS vault path in `src/ows.rs` so the changes apply to both `account *` and `wallet *` entry points.

See also: [signing-and-wallets](../systems/signing-and-wallets.md), [agent-output-contract](agent-output-contract.md).
