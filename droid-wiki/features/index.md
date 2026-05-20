# Features

Each feature page describes a user-facing capability — what commands exist, what data they accept, what dry-run shape they produce, and what safety gates apply.

## Trading and account

| Feature | Page | Top-level commands |
|---------|------|--------------------|
| Orders | [orders](orders.md) | `orders create / scale / batch-create / tpsl / cancel / cancel-all / modify / twap-create / twap-cancel / schedule-cancel / open / status / history` |
| Transfers | [transfers](transfers.md) | `transfer spot-to-perp / perp-to-spot / send / send-asset / withdraw / spot-send` |
| Subaccounts | [subaccounts](subaccounts.md) | `subaccount list / create / transfer / spot-transfer` |
| Staking, vaults, borrow/lend | [staking-vaults-borrowlend](staking-vaults-borrowlend.md) | `staking *`, `vault *`, `borrowlend *` |
| Builder fees and referrals | [builder-and-referrals](builder-and-referrals.md) | `builder *`, `referral *` |
| API / agent wallets | [api-wallets](api-wallets.md) | `api-wallet create / approve / list / revoke` |
| Account and portfolio | [account-and-portfolio](account-and-portfolio.md) | `account *` |
| Market data | [market-data](market-data.md) | `mids / book / candles / funding / spread / status / meta / perps / spot / asset / outcomes` |

## Agent surface

| Feature | Page |
|---------|------|
| JSON output contract | [agent-output-contract](agent-output-contract.md) |
| Dry-run previews | [dry-run](dry-run.md) |
| Schema discovery | [schema-discovery](schema-discovery.md) |
| Raw payload submission | [raw-payload](raw-payload.md) |
| Setup wizard | [setup-wizard](setup-wizard.md) |
| Feedback submission | [feedback](feedback.md) |
