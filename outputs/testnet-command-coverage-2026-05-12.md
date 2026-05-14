# Hyperliquid CLI Testnet Command Coverage Matrix

Date: `2026-05-12`

Purpose:
- explicit command-by-command audit against [src/command_catalog.json](/Users/studio/Documents/GitHub/hyperliquid-cli/src/command_catalog.json)
- classify each command as one of:
  - `validated_live`
  - `blocked_protocol_or_state`
  - `bugged`
  - `local_only_or_not_testnet_scoped`

Primary narrative report:
- [outputs/testnet-validation-report-2026-05-12.md](/Users/studio/Documents/GitHub/hyperliquid-cli/outputs/testnet-validation-report-2026-05-12.md)

## Matrix

| Command | Status | Evidence / Notes |
| --- | --- | --- |
| `status` | `validated_live` | Live read succeeded |
| `meta` | `validated_live` | Live read succeeded |
| `perps list` | `validated_live` | Live read succeeded |
| `perps get <COIN>` | `validated_live` | Live `BTC` read succeeded |
| `spot list` | `validated_live` | Live read succeeded |
| `spot get <PAIR>` | `validated_live` | Live `HYPE/USDC` read succeeded |
| `book <COIN>` | `validated_live` | Live `BTC` book read succeeded |
| `mids` | `validated_live` | Live read succeeded |
| `candles <COIN>` | `validated_live` | Live `BTC --limit 2` read succeeded |
| `spread <COIN>` | `validated_live` | Live `BTC` read succeeded |
| `funding <COIN>` | `validated_live` | Live `BTC` read succeeded |
| `outcomes list` | `validated_live` | Live read succeeded |
| `outcomes get <NOTATION>` | `validated_live` | Live failing-path check on `+100` succeeded |
| `builder max-fee` | `validated_live` | Live `--user ... --builder ...` read succeeded |
| `builder approved` | `validated_live` | Live read succeeded |
| `builder approve` | `validated_live` | Live `0%` approval succeeded |
| `account fills <ADDRESS>` | `validated_live` | Live read succeeded |
| `account fees <ADDRESS>` | `validated_live` | Live read succeeded |
| `account rate-limit <ADDRESS>` | `validated_live` | Live read succeeded |
| `account orders <ADDRESS>` | `validated_live` | Live read succeeded |
| `account portfolio <ADDRESS>` | `validated_live` | Live read succeeded, including implicit default signer path |
| `account subaccounts <ADDRESS>` | `validated_live` | Live read succeeded |
| `account portfolio-history <ADDRESS>` | `validated_live` | Live read succeeded |
| `account ledger <ADDRESS>` | `validated_live` | Live read succeeded |
| `account funding <ADDRESS>` | `validated_live` | Live read succeeded |
| `account twap-history <ADDRESS>` | `validated_live` | Live read succeeded |
| `account twap-fills <ADDRESS>` | `validated_live` | Live read succeeded |
| `account abstraction <ADDRESS>` | `validated_live` | Live read succeeded |
| `account abstraction set` | `validated_live` | Live mutation succeeded |
| `subaccount list <ADDRESS>` | `validated_live` | Live read succeeded |
| `subaccount create` | `validated_live` | Live mutation succeeded after driving traded volume above the protocol gate |
| `subaccount transfer` | `validated_live` | Live deposit and withdraw both succeeded against a real created subaccount |
| `subaccount spot-transfer` | `validated_live` | Live spot deposit and withdraw both succeeded against a real created subaccount |
| `account ls` | `local_only_or_not_testnet_scoped` | Local account registry only |
| `account add <PRIVATE_KEY>` | `local_only_or_not_testnet_scoped` | Local secret management |
| `account set-default <SELECTOR>` | `local_only_or_not_testnet_scoped` | Local config only |
| `account remove <SELECTOR>` | `local_only_or_not_testnet_scoped` | Local config only |
| `api-wallet create` | `validated_live` | Live mutation succeeded |
| `api-wallet approve` | `validated_live` | Live non-generated approve succeeded |
| `api-wallet list` | `validated_live` | Live read succeeded |
| `api-wallet revoke` | `validated_live` | Live mutation succeeded; see replacement-entry gotcha |
| `wallet create` | `local_only_or_not_testnet_scoped` | Exercised locally; not protocol validation |
| `wallet import <PRIVATE_KEY>` | `local_only_or_not_testnet_scoped` | Local secret management |
| `wallet show` | `validated_live` | Live wallet selector / OWS context read succeeded |
| `wallet address` | `validated_live` | Live wallet address read succeeded |
| `wallet reset` | `local_only_or_not_testnet_scoped` | Local config mutation |
| `wallet import-mnemonic <MNEMONIC>` | `local_only_or_not_testnet_scoped` | Local secret management |
| `wallet list` | `local_only_or_not_testnet_scoped` | Exercised locally; not protocol validation |
| `wallet rename <SELECTOR> <NEW_NAME>` | `local_only_or_not_testnet_scoped` | Local config mutation |
| `wallet delete <SELECTOR>` | `local_only_or_not_testnet_scoped` | Local config mutation |
| `wallet export <SELECTOR>` | `local_only_or_not_testnet_scoped` | Local secret export |
| `orders open` | `validated_live` | Live read succeeded; spot `HYPE/USDC` normalization is now verified in both test and funded-live canaries |
| `orders history` | `validated_live` | Live read succeeded |
| `orders status` | `validated_live` | Live read succeeded |
| `positions list` | `validated_live` | Live read succeeded |
| `staking validators` | `validated_live` | Live read succeeded |
| `staking summary <ADDRESS>` | `validated_live` | Live read succeeded |
| `staking rewards <ADDRESS>` | `validated_live` | Live read succeeded |
| `staking history <ADDRESS>` | `validated_live` | Live read succeeded |
| `vault list` | `validated_live` | Live read succeeded |
| `vault search <QUERY>` | `validated_live` | Live read succeeded |
| `vault get <ADDRESS>` | `validated_live` | Clean failing-path works for non-vault addresses, and live `vault get 0xa15099a30bbf2e68942d6f4c43d70d04faeab0a0` now succeeds after fixing scientific-notation decimal decoding |
| `vault positions <ADDRESS>` | `validated_live` | Live read succeeded |
| `borrowlend rates` | `validated_live` | Live read succeeded |
| `borrowlend get <TOKEN>` | `validated_live` | Live `USDC` read succeeded |
| `borrowlend user <ADDRESS>` | `validated_live` | Live read succeeded |
| `borrowlend supply <TOKEN> --amount <AMOUNT>` | `validated_live` | Live mutation succeeded |
| `borrowlend withdraw <TOKEN> --amount <AMOUNT>` | `validated_live` | Live mutation succeeded |
| `prio status` | `validated_live` | Live read succeeded |
| `referral status` | `validated_live` | Live read succeeded |
| `orders create` | `validated_live` | Live perp order create succeeded; spot create also exercised |
| `orders scale` | `validated_live` | Live BTC scale order succeeded and rested two orders; cleanup via `cancel-all` succeeded |
| `orders batch-create` | `validated_live` | Live BTC batch create succeeded from a relative JSON file; cleanup via `cancel-all` succeeded |
| `orders tpsl` | `validated_live` | Live TP/SL orders succeeded against a real BTC position; cleanup via `cancel-all` and reduce-only close succeeded |
| `orders cancel <ORDER_ID>` | `validated_live` | Live perp single-order cancel succeeded; live spot replacement-order cancel also succeeded |
| `orders cancel-all` | `validated_live` | Live perp cancel-all and live spot `HYPE/USDC` cancel-all both succeeded |
| `orders modify <ORDER_ID>` | `validated_live` | Live perp modify succeeded; live spot modify also succeeded and confirmed replace-style OID semantics |
| `orders twap-create` | `validated_live` | Live BTC TWAP create succeeded with `size 0.003` and `duration 300` |
| `orders twap-cancel <TWAP_ID>` | `validated_live` | Live failing-path status/cleanup check succeeded |
| `positions update-leverage` | `validated_live` | Live mutation returned success |
| `positions update-margin` | `validated_live` | Live isolated BTC position accepted `--amount 1`; cleanup via reduce-only close succeeded |
| `orders schedule-cancel` | `validated_live` | Live set-path succeeded after volume crossed the protocol gate (`status: scheduled`, `in_seconds: 30`); live clear-path also succeeded (`status: cleared`) at wallet volume `1001294.11` |
| `transfer spot-to-perp` | `validated_live` | Live mutation succeeded |
| `transfer perp-to-spot` | `validated_live` | Live mutation succeeded |
| `transfer send` | `validated_live` | Live send to second wallet succeeded; fee floor documented |
| `transfer spot-send` | `validated_live` | Live send to second wallet succeeded |
| `transfer send-asset` | `validated_live` | Live mutation succeeded |
| `transfer withdraw` | `validated_live` | Live `withdraw --amount 10 --to <SELF>` succeeded after clearing the fee floor |
| `staking delegate` | `validated_live` | Live delegate of `0.2 HYPE` to active validator `0x056997a0a5da08dca9410945fb7aa8daba39d45d` succeeded |
| `staking undelegate` | `validated_live` | Live undelegate of `0.2 HYPE` succeeded after the protocol lockup expired; immediate undelegate attempt was correctly rejected during lockup |
| `staking deposit` | `validated_live` | Live deposit of `0.5 HYPE` succeeded after acquiring HYPE on spot |
| `staking withdraw` | `validated_live` | Live withdraw queue of `0.1 HYPE` succeeded and created a pending withdrawal |
| `staking claim-rewards` | `blocked_protocol_or_state` | No rewards |
| `staking link initiate` | `validated_live` | Live initiate succeeded from `wallet-2` to staking user `0xB901ae5BF657D1aBbcea23Ef8cEA1a9936442372` |
| `staking link finalize` | `validated_live` | Live finalize succeeded from the staking wallet targeting trading user `0x1e1C549C8E7B7a90Fd01ff61aaAc59eDdc0D61a8` |
| `vault deposit` | `validated_live` | Live `vault deposit --vault 0xa15099a30bbf2e68942d6f4c43d70d04faeab0a0 --amount 5` submitted successfully; resulting vault equity is `4.999991` |
| `vault withdraw` | `blocked_protocol_or_state` | Live withdraw attempt against the same HLP deposit failed cleanly: `Cannot withdraw during lockup period after depositing.` |
| `prio bid` | `validated_live` | Live bid submitted successfully with `bid_hype 0.10000001` for `slot 0` on `127.0.0.1` |
| `referral set <CODE>` | `validated_live` | Live mutation succeeded |
| `referral register <CODE>` | `validated_live` | Live mutation succeeded after driving traded volume above the protocol gate |
| `subscribe trades` | `validated_live` | Live stream succeeded |
| `subscribe orderbook` | `validated_live` | Live stream succeeded |
| `subscribe candles` | `validated_live` | Live stream succeeded |
| `subscribe all-mids` | `validated_live` | Live stream succeeded |
| `subscribe order-updates` | `validated_live` | Live stream succeeded |
| `subscribe fills` | `validated_live` | Live stream succeeded |
| `schema` | `local_only_or_not_testnet_scoped` | Exercised locally; machine-readable inventory only |
| `setup` | `local_only_or_not_testnet_scoped` | Exercised locally; revealed unsafe non-interactive local-state mutation |

## Summary

- `validated_live`: 92
- `blocked_protocol_or_state`: 2
- `bugged`: 0
- `local_only_or_not_testnet_scoped`: 14

This matrix is the concrete prompt-to-artifact checklist for the current session. The objective is not yet complete because the remaining `blocked_protocol_or_state` commands still prevent a truthful claim that all commands have been validated against testnet.
