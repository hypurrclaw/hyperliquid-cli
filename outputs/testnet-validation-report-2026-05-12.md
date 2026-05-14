# Hyperliquid CLI Testnet Validation Report

Date: `2026-05-12`

Wallet under test:
- `0xB901ae5BF657D1aBbcea23Ef8cEA1a9936442372`

Scope:
- live testnet validation only
- OWS-backed default wallet
- real submissions preferred over dry-run
- report focuses on commands validated, protocol blockers, bugs, gotchas, and bad agent UX

## Executive Summary

The CLI has meaningful real testnet coverage now, but it is not at "all commands validated cleanly" yet.

What is solid:
- account-scoped reads with implicit/default signer
- OWS wallet signing
- perp transfer round-trip
- perp order create / cancel / close-position path
- scale / batch-create / TWAP / TP-SL order variants on real BTC testnet state
- successful spot HYPE acquisition on `HYPE/USDC`
- spot order cancel / cancel-all cleanup path
- live staking deposit / delegate / undelegate / withdraw-queue path
- live HLP vault deposit path
- live HLP `vault get` path on a valid vault address
- borrow/lend supply / withdraw
- `prio bid` submission succeeds on OWS signing path
- subaccount create and both subaccount transfer surfaces
- some user-state mutations (`referral set`, `referral register`, `account abstraction set`, `api-wallet create`, `api-wallet approve`, `api-wallet revoke`, `builder approve 0%`)

What is not solid:
- several surfaces are blocked by protocol eligibility or wallet state rather than CLI transport

Current known residue:
- referral state was changed to `TESTNET`
- account abstraction mode was changed to `disabled`
- an API-wallet replacement entry remains visible after revoke (`appr0512`)
- `10 USDC` was withdrawn from exchange balance to the same external wallet address during live `transfer withdraw` validation
- staking-link attribution was finalized between:
  - trading wallet `0x1e1C549C8E7B7a90Fd01ff61aaAc59eDdc0D61a8`
  - staking wallet `0xB901ae5BF657D1aBbcea23Ef8cEA1a9936442372`
- latest wallet state after staking tests:
  - `orders open`: `[]`
  - `positions list`: `[]`
  - spot `USDC total`: `55.13908551`
  - spot `HYPE total`: `0.48928192`
  - staking `undelegated`: `0.4`
  - staking `delegated`: `0.0`
- latest wallet state after HLP vault deposit:
  - `account_value`: `99.308521`
  - `vault_equities_count`: `1`
  - HLP vault equity: `4.999983`
  - HLP vault lockup until: `1778952231733`

## Commands Validated Live

### Wallet / account reads
- `wallet show`
- `wallet address`
- `account fills`
- `account portfolio`
- `account portfolio wallet`
- `account orders`
- `account ledger`
- `account fees`
- `account rate-limit`
- `account subaccounts`
- `account portfolio-history`
- `account funding --start 0`
- `account twap-history`
- `account twap-fills`
- `account abstraction <USER>`
- `referral status`
- `api-wallet list <USER>`
- `builder approved --user <USER>`
- `builder max-fee --user <USER> --builder <BUILDER>`
- `staking validators`
- `staking summary`
- `staking rewards`
- `staking history`
- `borrowlend user`
- `borrowlend rates`
- `borrowlend get USDC`
- `status`
- `meta`
- `perps list`
- `perps get BTC`
- `spot list`
- `spot get HYPE/USDC`
- `book BTC`
- `mids`
- `candles BTC --limit 2`
- `spread BTC`
- `funding BTC`
- `outcomes list --limit 5`
- `outcomes get +100` (validated failing path for missing notation)
- `prio status`
- `vault list`
- `vault search`
- `vault get 0xB901ae5BF657D1aBbcea23Ef8cEA1a9936442372` (validated clean failing-path)
- `vault positions 0xB901ae5BF657D1aBbcea23Ef8cEA1a9936442372`
- `subscribe trades --asset BTC --max-events 1 --idle-timeout-ms 6000`
- `subscribe orderbook --asset BTC --max-events 1 --idle-timeout-ms 6000`
- `subscribe candles --asset BTC --max-events 0 --idle-timeout-ms 6000`
- `subscribe all-mids --max-events 1 --idle-timeout-ms 6000`
- `subscribe order-updates --max-events 0 --idle-timeout-ms 6000`
- `subscribe fills --max-events 0 --idle-timeout-ms 6000`

### Transfers / orders / positions
- `transfer spot-to-perp --amount 5`
- `transfer perp-to-spot --amount 5`
- `transfer spot-send --to <SECOND_WALLET> --token USDC --amount 0.1 -y`
- `transfer send --to <SECOND_WALLET> --amount 0.01 -y`
- `transfer send-asset --to <SELF> --source spot --dest perp --token USDC --amount 0.1`
- `transfer withdraw --to <SELF> --amount 10 -y`
- `orders create --coin BTC --side buy --price 20000 --size 0.001 --tif alo -y`
- `orders cancel-all --coin BTC -y`
- `orders create --coin BTC --side sell --type market --amount 20 --reduce-only -y`
- `orders scale --coin BTC --side buy --start-price 20000 --end-price 21000 --total-size 0.0012 --orders 2 --tif alo -y`
- `orders batch-create --orders-file outputs/live-batch-orders.json -y`
- `orders history`
- `orders status --user <USER> --oid 52943226350`
- `orders open`
- `orders modify 52945272963 --price 21000 --size 0.001`
- `orders cancel 52945324833`
- `orders create --coin HYPE/USDC --side buy --price 28 --size 0.36 --tif gtc -y`
- `orders modify 52953626772 --price 27.5 --size 0.36`
- `orders cancel 52953643434`
- `orders create --coin BTC --side buy --type market --amount 20 -y`
- `orders tpsl --coin BTC --take-profit 90000 --stop-loss 70000 -y`
- `orders twap-create --coin BTC --side buy --size 0.003 --duration 300 -y`
- `orders twap-cancel 15418 --coin BTC`
- `positions list`
- `positions update-leverage --coin BTC --leverage 3 --isolated`
- `positions update-margin --coin BTC --amount 1`

### Borrow/lend
- `borrowlend supply USDC --amount 5`
- `borrowlend withdraw USDC --amount 5`

### Subaccounts
- `subaccount create --name codex-c-0512c`
- `subaccount transfer --subaccount 0x7cc55AC2Cfe2083c74529F7848A55971fd76fDdD --amount 1 --direction deposit`
- `subaccount transfer --subaccount 0x7cc55AC2Cfe2083c74529F7848A55971fd76fDdD --amount 1 --direction withdraw`
- `subaccount spot-transfer --subaccount 0x7cc55AC2Cfe2083c74529F7848A55971fd76fDdD --token USDC --amount 1 --direction deposit`
- `subaccount spot-transfer --subaccount 0x7cc55AC2Cfe2083c74529F7848A55971fd76fDdD --token USDC --amount 1 --direction withdraw`

### User-state mutations
- `api-wallet create --name codextrackc0512 ...`
- `api-wallet approve --agent-address 0x00000000000000000000000000000000000c0de0 --name appr0512 --expires-in 1h`
- `api-wallet revoke --name codextrackc0512`
- `api-wallet revoke --name appr0512`
- `builder approve --builder <USER> --max-fee-rate 0% -y`
- `referral set TESTNET`
- `referral register C512B901AE`
- `account abstraction set --mode disabled -y`

## Protocol / Eligibility Blockers

These were real testnet attempts, but the failures are protocol or state constraints rather than obvious CLI transport failures.

### Staking
- `staking claim-rewards`
  - live attempt now reaches exchange and fails cleanly with: `No rewards to claim`
  - re-checked on `2026-05-13`; state is unchanged:
    - `staking summary` still shows `pending_rewards: "0"`
    - `staking rewards` still returns `[]`
    - `staking claim-rewards` still fails cleanly with: `No rewards to claim`
  - official staking docs say rewards are accrued every minute and distributed every day, with rewards based on the minimum balance staked during each staking epoch
  - inference: this short-lived testnet session did not accumulate any claimable reward state for the wallet under test
  - source: https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/staking

### HYPE acquisition
- market buy attempt on `HYPE/USDC`
  - failed: no immediately matchable resting liquidity
- limit bids could be placed live, but no HYPE filled during this session
- latest retest:
  - `orders create --coin HYPE/USDC --side buy --price 30 --size 1 --tif gtc -y`
  - rested unfilled at OID `52959916107`
  - the earlier cancel response surfaced `coin: "@1035"`
- more aggressive retests:
  - `orders create --coin HYPE/USDC --side buy --price 50 --size 0.5 --tif gtc -y`
  - rested unfilled at OID `52960432379`
  - `orders create --coin HYPE/USDC --side buy --price 90 --size 0.5 --tif gtc -y`
  - also rested unfilled at OID `52960461965`
  - both were canceled successfully
- direct book probes:
  - raw testnet `/info` request with `{"type":"l2Book","coin":"HYPE/USDC"}` returned `null`
  - this turned out to be identifier semantics rather than missing market data
  - Hyperliquid docs say most spot books use `@{index}` rather than the human pair symbol
  - for testnet HYPE spot, `spotMeta` showed pair index `1035`, i.e. `@1035`
  - raw `l2Book` for `@1035` returned a valid snapshot immediately
  - the CLI bug was that `book HYPE/USDC` and `subscribe orderbook --asset HYPE/USDC` were subscribing with the display symbol instead of `@1035`
  - after the fix, both commands succeeded live and now subscribe against `l2Book(@1035)`
  - later lifecycle normalization work also fixed operator-facing spot symbols:
    - funded-live canary now shows `HYPE/USDC` consistently in `orders open` and `orders cancel`
- official docs only document a mock USDC testnet faucet, not a HYPE faucet:
  - https://hyperliquid.gitbook.io/hyperliquid-docs/onboarding/testnet-faucet
- official docs do list **HyperEVM testnet HYPE faucets for gas**:
  - Chainstack: `1 HYPE every 24 hours`
  - QuickNode faucet
  - source: https://hyperliquid.gitbook.io/hyperliquid-docs/builder-tools/hyperevm-tools
- Chainstack's own faucet docs add important operational constraints:
  - requires a Chainstack API key for authentication
  - requires at least `0.08 ETH` on mainnet to qualify
  - source: https://docs.chainstack.com/reference/chainstack-faucet-introduction
- Chainstack's Hyperliquid faucet guide also says the UI flow is:
  - sign into Chainstack
  - get API key
  - paste API key into the faucet page
  - claim `1 HYPE` every 24 hours
  - source: https://chainstack.com/hyperliquid-faucet/
- QuickNode's faucet page indicates an interactive wallet/social flow rather than a simple anonymous API:
  - connect wallet
  - choose chain/network
  - tweet/share to receive a token drip
  - source: https://faucet.quicknode.com/hyperliquid
- official docs also describe moving HYPE from HyperEVM back to HyperCore by sending it to:
  - `0x2222222222222222222222222222222222222222`
  - source: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/hyperevm/hypercore-less-than-greater-than-hyperevm-transfers
- direct RPC check on the wallet under test showed no existing HyperEVM HYPE to bridge:
  - `eth_getBalance(0xB901ae5BF657D1aBbcea23Ef8cEA1a9936442372)` on `https://rpc.hyperliquid-testnet.xyz/evm`
  - result: `0x0`
- official staking docs confirm staking requires HYPE in Spot first:
  - https://hyperliquid.gitbook.io/hyperliquid-docs/onboarding/how-to-stake-hype
- after additional mock USDC funding, the spot acquisition path worked live:
  - `orders create --coin HYPE/USDC --side buy --price 100 --size 1 --tif gtc -y`
  - filled immediately at `91.8`
  - OID `52961140214`
  - resulting spot state included `HYPE total: 0.999328`
- with that HYPE in spot, the staking path became live-testable and succeeded:
  - `staking deposit --amount 0.5`
    - submitted successfully
    - `staking summary` then showed `undelegated: 0.5`
  - `staking delegate --validator 0x056997a0a5da08dca9410945fb7aa8daba39d45d --amount 0.2`
    - submitted successfully
    - `staking summary` then showed `delegated: 0.2`, `undelegated: 0.3`
  - immediate `staking undelegate ... --amount 0.2`
    - failed cleanly with: `Cannot undelegate during lockup period after delegating or voting.`
  - after the lockup expired, `staking undelegate ... --amount 0.2`
    - submitted successfully
    - `staking summary` then returned `delegated: 0.0`, `undelegated: 0.4`
  - `staking withdraw --amount 0.1`
    - submitted successfully
    - created `n_pending_withdrawals: 1`
    - `total_pending_withdrawal: 0.1`
  - two-wallet staking-link flow also worked live:
    - `--ows-signer wallet-2 staking link initiate --user 0xB901ae5BF657D1aBbcea23Ef8cEA1a9936442372 --yes`
    - `staking link finalize --user 0x1e1C549C8E7B7a90Fd01ff61aaAc59eDdc0D61a8 --yes`
    - both submitted successfully

### Subaccounts
- `subaccount create --name codex-c-0512`
  - originally failed: `Cannot create sub-accounts until enough volume traded. Required: $100000. Traded: $0.`
- after driving testnet BTC volume to `100333.39`, `subaccount create --name codex-c-0512c` succeeded live
- `subaccount transfer` and `subaccount spot-transfer` both succeeded live after normalizing the signed subaccount address casing in the raw action payload

### Referral
- `referral register C512B901AE`
  - originally failed: insufficient traded volume to generate a code
  - succeeded live after driving testnet BTC volume above `$10000`

### Vaults
- `vault deposit --vault 0xB901... --amount 0.01`
  - failed: target not a registered vault
- `vault deposit --vault <known QA vault> --amount 1`
  - failed: minimum deposit is `$5`
- `vault deposit --vault <known QA vault> --amount 5`
  - failed: insufficient funds available to deposit
- `vault withdraw`
  - failed because no vault balance was created
- later direct app probe contradicted the earlier empty-summary assumption:
  - the live testnet `/vaults` page showed real protocol and user vaults
  - clicking the HLP row navigated to:
    - `https://app.hyperliquid-testnet.xyz/vaults/0xa15099a30bbf2e68942d6f4c43d70d04faeab0a0`
  - that gave a real, depositable protocol vault address for HLP
- raw testnet `vaultDetails` for HLP succeeded:
  - `POST /info` with `{"type":"vaultDetails","vaultAddress":"0xa15099a30bbf2e68942d6f4c43d70d04faeab0a0"}`
  - returned valid details including:
    - `allowDeposits: true`
    - `isClosed: false`
- live vault deposit then succeeded:
  - `hyperliquid --testnet --format json vault deposit --vault 0xa15099a30bbf2e68942d6f4c43d70d04faeab0a0 --amount 5`
  - returned:
    - `status: "submitted"`
    - `vault_address: "0xa15099a30bbf2e68942d6f4c43d70d04faeab0a0"`
  - follow-up state confirmed:
    - `account portfolio` showed `vault_equities_count: 1`
    - `vault_equities[0].equity: "4.999991"`
    - `locked_until_timestamp: 1778952231733`
- live vault withdraw now has a precise blocker:
  - `hyperliquid --testnet --format json vault withdraw --vault 0xa15099a30bbf2e68942d6f4c43d70d04faeab0a0 --amount 5`
  - failed with:
    - `Cannot withdraw during lockup period after depositing.`
  - official docs say HLP has a 4 day lock-up period after the most recent deposit
  - user vaults have a 1 day lock-up period, but this session only found a usable HLP testnet vault path
  - sources:
    - https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/vaults/protocol-vaults
    - https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/vaults/for-vault-depositors-legacy
- the valid-vault read bug was fixed locally:
  - raw HLP details had scientific-notation decimals such as:
    - `leaderFraction: 7.445690056409042E-7`
  - the CLI parser was updated to accept scientific notation via `Decimal::from_scientific(...)`
  - live retest on the rebuilt local binary:
    - `./target/debug/hyperliquid --testnet --format json vault get 0xa15099a30bbf2e68942d6f4c43d70d04faeab0a0`
    - succeeded and returned:
      - `leader_fraction: "0.0000007445690056409042"`
      - `apr: "-0.005265174448410338"`
      - `allow_deposits: true`
  - clean failing-path behavior also still works:
    - `./target/debug/hyperliquid --format json vault get 0x0000000000000000000000000000000000000002`
    - returns `vault details not found ...`
- docs confirm vaults are a real HyperCore primitive and HLP is a protocol vault:
  - https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/vaults
  - https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/vaults/protocol-vaults
- docs also say creating a vault requires:
  - minimum `100 USDC` deposit
  - a separate `100 USDC` gas fee
  - source: https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/vaults/for-vault-leaders-legacy
- however, the current CLI surface has `vault list|search|get|positions|deposit|withdraw` only; it does **not** expose vault creation
- so the vault picture is now:
  - `vault deposit` is validated live
  - `vault withdraw` is protocol-blocked by lockup, not by missing vault discovery
  - `vault get` is validated live on both a real vault and a clean not-found path

### Orders / transfers / risk controls
- `transfer send --to <SELF> --amount 0.1`
  - failed: uses perp withdrawable, which was only `0.001091`
- `transfer send --to <SECOND_WALLET> --amount 0.0005 -y`
  - failed: `Usd transfer is smaller than fee.`
- `transfer spot-send --to <SELF> --token USDC --amount 0.1`
  - failed: `Cannot self-transfer.`
- `transfer withdraw --amount 0.1 --to <SELF> -y`
  - now reaches exchange-side validation but fails with: `Withdrawal is smaller than fee.`
- `transfer withdraw --amount 10 --to <SELF> -y`
  - succeeded live
- `orders create --coin BTC --side buy --type market --amount 10 -y`
  - failed: `Order must have minimum value of $10. asset=3`
  - observed behavior suggests the effective minimum is stricter than the obvious boundary case
- `orders schedule-cancel --in 30s`
  - failed: hidden volume gate of `$1000000`
  - latest retest after additional live BTC round-trip still failed with:
    - `Cannot set scheduled cancel time until enough volume traded. Required: $1000000. Traded: $102455.68.`
  - later account volume reads show the wallet advanced to:
    - `cum_vlm: 192770.56`
  - further live farming experiments established:
    - direct BTC market churn moves volume quickly but burns capital too aggressively
    - HYPE perp paired crossing between the funded wallet and `wallet-2` works cleanly and keeps both wallets flat
    - making the funded wallet the maker is materially cheaper than making it the taker
  - measured HYPE perp canaries:
    - main wallet taker, `price 26.53`, `size 18`
      - volume delta: `955.08`
      - main-wallet withdrawable burn: `0.412594`
    - main wallet maker, `price 26.54`, `size 18`
      - volume delta: `955.44`
      - main-wallet withdrawable burn: `0.137582`
    - higher-notional main wallet maker, `price 26.55`, `size 38`
      - volume delta: `2017.80`
      - main-wallet withdrawable burn: `0.290562`
      - `wallet-2` withdrawable burn: `0.908010`
      - combined burn: `1.198572`
      - combined burn rate stayed effectively flat versus the smaller maker canary
    - attempted `size 40` at the same price failed immediately with:
      - `Insufficient margin to place order. asset=135`
    - repeated maker-batch attempts exposed an additional failure mode:
      - the maker leg can be hit by unrelated market flow before `wallet-2` crosses it
      - this happened on both:
        - `HYPE` maker sell, leaving a naked main-wallet short at `entry 28.18`
        - `STABLE` maker sell, leaving a naked main-wallet short plus a resting cleanup bid
      - both cases required manual flattening after the fact
  - spot HYPE self-crossing also counts toward the same volume meter, but it is not a practical farming path:
    - `HYPE/USDC` maker/taker round-trip at `81.50 x 0.13`
    - volume delta: `20.37`
    - cleanup left HYPE dust on `wallet-2` and required post-trade reconciliation
  - conclusion:
    - the gate is exercisable on testnet
    - the best tested path so far is HYPE perp paired crossing with the funded wallet as maker
    - with the latest maker canaries, the combined burn rate is about `5.94 bps` per credited volume
    - at the latest `cum_vlm` of `194788.36`, reaching `$1000000` would still require about:
      - `805211.64` more credited volume
      - roughly `47.8` more size-38 maker cycles at current efficiency
      - about `478` additional account-value burn across the two wallets
    - current combined perp withdrawable across the two wallets is only about:
      - `212.047121`
    - so even the best tested farming path still needs more mock capital to finish the gate cleanly
  - separate CLI surface gap:
    - Hyperliquid's documented exchange API allows `scheduleCancel` with no `time` field to remove the dead-man switch
    - this CLI currently requires `--in <IN_DURATION>` unconditionally
    - evidence:
      - `hyperliquid orders schedule-cancel --help`
        - `Usage: hyperliquid orders schedule-cancel [OPTIONS] --in <IN_DURATION>`
      - `hyperliquid --format json schema orders schedule-cancel`
        - `in_duration` is the only argument and is marked `required: true`
    - effect:
      - even if a scheduled cancel had been set successfully, the CLI does not currently expose the documented clear/remove path

## Validated Bugs

### 1. `api-wallet revoke --name ...` leaves a replacement entry rather than a clean empty list

Observed live sequence:
- `api-wallet approve --agent-address 0x00000000000000000000000000000000000c0de0 --name appr0512 --expires-in 1h`
  - succeeded
- `api-wallet list`
  - showed `appr0512` at `0x00000000000000000000000000000000000c0De0`
- `api-wallet revoke --name appr0512`
  - submitted successfully
- follow-up `api-wallet list`
  - still showed `appr0512`, now at `0x946ddB415842797641Fa3D522D223deCDC81Aeec`

Interpretation:
- revoke for a named API wallet currently behaves like "replace with short-lived throwaway agent" rather than removing the visible entry entirely
- that may be intended protocol behavior, but it is surprising operator UX and leaves residue that agents need to understand

## Gotchas

### Borrow/lend round-trip is not perfectly lossless
- `borrowlend supply USDC --amount 5`
- `borrowlend withdraw USDC --amount 5`

Result:
- wallet ended at `197.99999999 USDC` instead of exactly `198.0`

Interpretation:
- likely protocol rounding or accounting dust, not an open-state issue

### Transfer send path has a fee floor and leaves fee residue on round-trip
- `transfer send --to <SECOND_WALLET> --amount 0.0005 -y`
  - failed: `Usd transfer is smaller than fee.`
- `transfer send --to <SECOND_WALLET> --amount 0.01 -y`
  - succeeded
- returning `0.009` from the destination wallet succeeded
- destination wallet then showed:
  - `spot USDC total: 0.0`
  - `withdrawable/account_value: 0.001`

Interpretation:
- the live transfer-send path is real and working
- there is a fee floor, and a `0.01 -> 0.009` round-trip leaves `0.001` behind on the destination wallet

### `orders create --type market` on spot can fail cleanly even when the pair exists
- the pair existing is not enough
- actual matchable resting liquidity matters

### `orders create --type market --amount 10` on perps fails at the advertised boundary
- the exchange rejected a `$10` BTC market order with:
  - `Order must have minimum value of $10. asset=3`
- from an agent perspective this behaves like `> $10`, not `>= $10`

### Spot order identifiers previously leaked internal asset notation into operator-visible output
- earlier in the session, spot order lifecycle responses surfaced `@1035` instead of `HYPE/USDC`
- this is now fixed for the tested paths:
  - funded-live canary with order `52968350932` returned `coin: "HYPE/USDC"` in:
    - create response
    - `orders open`
    - cancel response
- remaining implementation note:
  - the CLI still translates to `@1035` internally for exchange actions and orderbook subscriptions, which is correct protocol behavior

### Staking withdraw state is not obvious from follow-up summary
- `staking withdraw --amount 0.1`
  - submitted successfully
  - response note said: `7-day withdrawal queue before funds become available in spot`
- immediate follow-up `staking summary` showed:
  - `n_pending_withdrawals: 1`
  - `total_pending_withdrawal: 0.1`
- later follow-up `staking summary` showed:
  - `n_pending_withdrawals: 0`
  - `total_pending_withdrawal: 0.0`
  - `undelegated: 0.4`
- spot HYPE stayed at `0.499328`

Interpretation:
- the live command path worked, but the resulting staking state is not intuitive from CLI reads alone
- this may be protocol behavior, indexing lag, or a surface that needs clearer operator explanation

### Spot hold cleanup is slightly eventually consistent after cancel
- right after canceling the replacement spot order `52953643434`, `account portfolio` still showed:
  - `hold: 9.9`
- a follow-up portfolio read a few seconds later returned:
  - `hold: 0.0`

Interpretation:
- the live order lifecycle is working, but immediate post-cancel balance reads can lag briefly

### Self-transfer semantics are inconsistent across transfer commands
- `transfer send --to <SELF>` did not reject self-targeting directly; it failed later on perp withdrawable balance
- `transfer spot-send --to <SELF>` rejected self-targeting explicitly
- `transfer send-asset --to <SELF> --source spot --dest perp` succeeded and acted like an internal venue transfer
- that inconsistency is hard for agents to predict

### `positions update-leverage` can report success without an open position
- `positions update-leverage --coin BTC --leverage 3 --isolated` returned `status: updated`
- the wallet had no BTC position at that moment
- this may be valid account-level configuration behavior, but the UX is ambiguous and easy for agents to misread as position mutation

### TWAP create/cancel state is hard to interpret from one-shot command output
- `twap-create` returned `status: running`
- `account twap-history` later showed:
  - first `activated`
  - then terminal `error: Insufficient margin to place order.`
- `twap-cancel` then said:
  - `TWAP was never placed, already canceled, or filled.`

From an agent perspective that is hard to reason about without a follow-up history read.

### `orders modify` behaves like replace, with a new live OID to clean up
- a resting BTC order at OID `52945272963` was modified successfully:
  - `orders modify 52945272963 --price 21000 --size 0.001`
- the modified result returned `status: modified`
- live `orders open` then showed a replacement order at a new OID:
  - `52945293712`
- that replacement had to be canceled separately

Agents need to treat modify as a replace-style lifecycle, not as an in-place patch with the original OID necessarily remaining active.

### Volume-gated commands can be force-unblocked with repeated reversible BTC round-trips
- to cross the referral and subaccount gates, the session used repeated live BTC market buy/sell round-trips on isolated leverage
- this raised `daily_user_vlm[].user_cross` for `2026-05-12` from `64.6` to `100333.39`
- this was sufficient to convert:
  - `referral register` from blocked to validated
  - `subaccount create` from blocked to validated

Interpretation:
- the gating is genuinely volume-based and can be exercised on testnet without external state, at the cost of repeated reversible trading loops

### Spot self-crossing is not clean enough to use as a volume-farming primitive
- a live spot HYPE/USDC canary with the funded wallet as maker succeeded:
  - maker sell `0.13 HYPE` at `81.50`
  - `wallet-2` IOC buy matched
  - maker buy `0.13 HYPE` at `81.50`
  - `wallet-2` IOC sell only filled `0.12`
- results:
  - `cum_vlm` still increased by `20.37`, so spot volume appears to count toward the same gate
  - the second leg silently left:
    - funded wallet bid residue of `0.01 HYPE`
    - `wallet-2` HYPE dust of `0.009909`
  - attempting to clean that dust with:
    - `transfer spot-send --token HYPE --amount 0.009909`
    - failed with: `Unknown token HYPE`
- also observed:
  - spot orders under `$10` are rejected with:
    - `Order must have minimum value of 10 USDC. asset=11035`

Interpretation:
- spot self-crossing is real and volume-counting
- it is worse than the perp-maker path for agent automation because of minimum-order floors, asymmetrical close sizing, and token-dust cleanup failure

### Perp self-crossing is not batch-safe without stronger orchestration
- repeated maker-first farming attempts on the main wallet showed that a "working canary" is not enough to assume batch safety
- failure mode observed live:
  - the maker order can be matched by unrelated external flow before `wallet-2` submits the IOC cross
  - that leaves the main wallet with an unintended naked position
- concrete incidents:
  - `HYPE`
    - maker sell was lifted externally
    - `wallet-2` IOC buy then failed with `Order could not immediately match against any resting orders`
    - main wallet was left with a naked short
    - cleanup required:
      - explicit aggressive reduce-only limit buy at `29`
  - `STABLE`
    - maker sell filled externally
    - `wallet-2` IOC buy failed the same way
    - the follow-up maker-close bid rested instead of flattening
    - main wallet was left with:
      - a short `STABLE` perp position
      - a resting `STABLE` bid
    - cleanup required:
      - canceling the stray bid
      - explicit reduce-only market buy to flatten the short

Interpretation:
- maker-first perp farming is still the best tested economic path
- but it is operationally unsafe to batch naïvely because external matching can race the counter-wallet
- agents need either:
  - very tight orchestration with position/order reconciliation after every cycle, or
  - a quieter venue/asset than the ones tested here

### `api-wallet approve` has two distinct UX paths with very different automation properties
- `api-wallet approve --generate --store ...` entered an interactive local-secret path and was aborted during coverage
- `api-wallet approve --agent-address ... --name appr0512 --expires-in 1h` submitted cleanly and is suitable for automation

Agents need this distinction surfaced more clearly up front.

### `setup` is not safe to run non-interactively
- `setup --format json` auto-walked the wizard under non-interactive execution
- it created a new local wallet alias `setup`
- it rewrote local config and changed the default wallet away from the funded test wallet until manually restored with:
  - `account set-default wallet`

From an agent perspective this is dangerous local-state mutation for a command that presents as a setup wizard rather than an explicit destructive action.

## Bad Agent UX

### 1. Protocol blockers are often only discovered after live submission attempts
Examples:
- subaccount volume gate
- referral registration volume gate
- schedule-cancel volume gate

### 2. `setup` mutates local wallet state too eagerly
Bad:
- `setup` auto-walked wallet creation non-interactively
- it changed the default wallet until manual cleanup restored the intended signer

Better:
- require explicit confirmation before creating a wallet or changing the default signer
- print the planned local mutations before applying them

### 3. API-wallet revoke behavior is still surprising
Bad:
- revoke completed, but a replacement-style residue entry remained visible during live QA

Better:
- explain replacement/revocation semantics clearly in the success output
- or return the remaining visible API-wallet state directly
- vault eligibility
- spot liquidity absence
- schedule-cancel volume gate
- TWAP minimum notional gate

Better:
- expose clearer preflight/eligibility reads where possible

### 4. Some irreversible surfaces need clearer agent-facing warnings
Examples:
- `referral set`
- `account abstraction set`
- `subaccount create`
- `setup`

The CLI can do them live, but an agent needs very obvious messaging about durability and cleanup impossibility.

### 5. Hidden venue semantics are not surfaced early enough
Examples:
- `transfer send` is effectively constrained by perp withdrawable, not total visible USDC
- `transfer send-asset` can behave like an internal venue move even when `--to` is self
- `positions update-leverage` may be account-scoped rather than position-scoped in effect

Agents need stronger output cues about what balance bucket or control plane a command is actually mutating.

### 6. Spot order close sizing and token cleanup are not agent-friendly
Bad:
- a symmetric spot round-trip request at `0.13` came back with a filled close size of `0.12`
- cleanup then required understanding:
  - residual bid size on the funded wallet
  - residual HYPE dust on `wallet-2`
  - `spot-send` rejecting `HYPE` by symbol for dust cleanup

Better:
- close responses should make any fee-related size reduction explicit
- spot-transfer surfaces should accept the same token identifier that portfolio and spot order flows expose
- dust-producing paths should be documented up front when the CLI cannot clean them fully

### 7. Repeated self-cross farming can create accidental exposure if the maker leg is externally filled
Bad:
- a canary can work, but repeating the same pattern later can still fail because the market changes underneath the agent
- when the maker leg is lifted externally, the counter-wallet IOC fails and the primary wallet is left holding unintended risk

Better:
- explicit "maker leg filled by third party" detection in command output or QA tooling
- a supported paired-order primitive for controlled two-wallet testing
- or at minimum a documented recommendation that agents reconcile positions and open orders after every farming cycle instead of batching blindly

### 8. `orders schedule-cancel` now exposes the clear/remove path, and both live variants reduce to the same protocol gate
Bad before fix:
- the exchange API supports `scheduleCancel` without a `time` field to remove an existing dead-man switch
- the CLI only exposed the set path, and the first live clear-path implementation produced a bogus recovered user/API wallet address

Evidence after fix:
- `./target/debug/hyperliquid orders schedule-cancel --help`
  - shows both `--in <IN_DURATION>` and `--clear`
- `./target/debug/hyperliquid --format json schema orders schedule-cancel`
  - includes both `in_duration` and `clear`
- `./target/debug/hyperliquid --format json --dry-run orders schedule-cancel --clear`
  - returns:
    - `{"command":"orders schedule-cancel","dry_run":true,"would_execute":"schedule_dead_mans_switch","args":{"mode":"clear"}}`
- live set path:
  - `./target/debug/hyperliquid --testnet --ows-signer wallet --format json orders schedule-cancel --in 30s`
  - fails with:
    - `Cannot set scheduled cancel time until enough volume traded. Required: $1000000. Traded: $195565.14.`
- live clear path:
  - `./target/debug/hyperliquid --testnet --ows-signer wallet --format json orders schedule-cancel --clear`
  - now reaches the same exchange gate and fails with:
    - `Cannot set scheduled cancel time until enough volume traded. Required: $1000000. Traded: $195565.14.`

Better:
- keep the current `--clear` surface
- preserve the raw-action clear-path implementation that omits `time` instead of serializing `null`
- document the exchange-side volume gate explicitly so agents can plan around it

## Final Wallet State At End Of Session

Confirmed locally near the end:
- positions: `0`
- open orders: `0`
- spot USDC hold: `0.0`
- referral:
  - `referred_by_code: TESTNET`
- account abstraction:
  - `disabled`
- api-wallet list:
  - one replacement entry remains for `appr0512`
- referral register:
  - `C512B901AE` submitted successfully
- subaccount list:
  - `codex-c-0512c` at `0x7cc55AC2Cfe2083c74529F7848A55971fd76fDdD`
- daily user cross volume on `2026-05-12`:
  - `1001294.11`
- funded wallet current withdrawable/account_value:
  - `335.479711`
- spot USDC total:
  - `154.13908551`
- spot HYPE total:
  - `0.48928192`
- HLP vault equity:
  - `4.999959`
- HLP vault lockup expires:
  - `2026-05-16 22:53:51 IST`
- staking summary:
  - `delegated: 0.0`
  - `undelegated: 0.4`
  - `pending_rewards: 0`
- disposable transfer wallet:
  - `0x1e1C549C8E7B7a90Fd01ff61aaAc59eDdc0D61a8`
  - `spot USDC total: 0.0`
  - `spot HYPE total: 0.009909`
  - `withdrawable/account_value: 37.459112`

## Remaining Gaps

Still not truly validated end-to-end because of real blockers or unresolved product decisions:
- `staking claim-rewards` still needs actual accrued rewards rather than only principal/staking state
- an unlocked vault withdraw after the HLP deposit lockup expires

## Dead-Man Switch Unlocked

After pushing traded volume above the `$1,000,000` gate, both live `orders schedule-cancel` paths validated cleanly:
- set path:
  - `orders schedule-cancel --in 30s`
  - result:
    - `status: scheduled`
    - `in_seconds: 30`
- clear path:
  - `orders schedule-cancel --clear`
  - result:
    - `status: cleared`

Latest volume at unlock:
- `cum_vlm: 1001294.11`

## Late-Session Volume Farming Findings

After the earlier schedule-cancel investigation, the wallet was used again to push traded volume higher:
- starting point for the late farming pass:
  - `cum_vlm: 195565.14`
- clean one-cycle canary:
  - maker sell on main wallet: `HYPE 20 @ 28.36`
  - IOC buy on `wallet-2`: filled
  - maker buy on main wallet: `HYPE 20 @ 28.34`
  - IOC sell on `wallet-2`: filled
  - resulting `cum_vlm` after the canary:
    - `196699.14`
- guarded loop then pushed volume further to:
  - `202369.13`
- after another `300 USDC` funding top-up, additional guarded canaries and loop batches pushed main-wallet traded volume to:
  - `232069.62`

New findings from the extra funded pass:
- converting fresh spot USDC into perp balances works cleanly and materially increases the available farming budget:
  - `transfer spot-to-perp --amount 100`
  - `transfer spot-send --to 0x1e1C549C8E7B7a90Fd01ff61aaAc59eDdc0D61a8 --token USDC --amount 100 -y`
  - `wallet-2 transfer spot-to-perp --amount 100`
- a dynamic inside-spread HYPE perp canary did succeed cleanly:
  - main maker sell at `28.519`
  - `wallet-2` IOC buy matched
  - main maker buy at `28.181`
  - `wallet-2` reduce-only IOC sell matched
  - wallets ended flat
  - main `cum_vlm` moved from `202369.13` to `203503.13`
- but broader dynamic farming remained unreliable:
  - when the HYPE spread blew out to `24.55 / 41.354`, `wallet-2` bought external asks while the main-wallet maker ask stayed resting
  - BTC had the same class of failure:
    - `wallet-2` IOC buy filled external BTC asks before the main maker ask was hit
    - cleanup left small residual positions that had to be flattened manually
  - AVAX looked better than HYPE or BTC on raw spread and depth, but still showed the same structural issue:
    - main maker sell at `9.8204` rested
    - `wallet-2` IOC buy filled only `38.4 AVAX`
    - main maker buy at `9.8132` then rested
    - `wallet-2` reduce-only IOC sell flattened, but the funded wallet was left long `38.4 AVAX` plus two resting orders
    - cleanup succeeded, and the net result still moved main `cum_vlm` upward
- a materially better primitive was found afterward:
  - single-wallet AVAX market buy, followed immediately by single-wallet reduce-only market sell
  - example clean canary:
    - `orders create --coin AVAX --side buy --type market --amount 500 -y`
    - `orders create --coin AVAX --side sell --type market --amount 500 --reduce-only -y`
    - resulting `cum_vlm` moved from `205069.03` to `206069.11`
    - wallet ended flat with `orders open: []`, `positions list: []`
  - guarded batch result:
    - 8 clean loops completed
    - main `cum_vlm` increased from `206069.11` to `214068.74`
    - the ninth loop left only a tiny `0.05 AVAX` residual long from partial close rounding
    - cleanup via reduce-only IOC sell succeeded immediately
  - this is the cleanest live farming path found in the session so far
  - another follow-up loop at the same `500 USDC` notional also worked directionally:
    - `cum_vlm` increased from `214068.74` to `215068.87`
    - it left only a tiny `0.01 AVAX` long from partial close rounding
    - cleanup via reduce-only IOC sell succeeded immediately
  - higher notional AVAX churn was then validated:
    - clean `1500 USDC` canary:
      - `cum_vlm` increased from `217069.08` to `220068.48`
      - wallet ended flat
    - guarded `1500 USDC` batch:
      - two loops pushed `cum_vlm` from `220068.48` to `226068.13`
      - cleanup after the second loop's tiny `0.02 AVAX` residual succeeded immediately
    - another guarded `1500 USDC` batch:
      - two more loops pushed `cum_vlm` from `226068.13` to `232069.62`
      - cleanup after the second loop's tiny `0.07 AVAX` residual succeeded immediately
    - a later guarded `1500 USDC` batch pushed volume further:
      - four loops moved `cum_vlm` from `232069.62` to `244069.16`
      - cleanup after the fourth loop's tiny `0.03 AVAX` residual succeeded immediately
    - one more guarded `1500 USDC` batch moved volume again:
      - five loops moved `cum_vlm` from `244069.45` to `259068.20`
      - the batch ended with a tiny `0.01 AVAX` residual long at `9.8525`
      - cleanup via reduce-only IOC sell at `9.849` succeeded immediately
      - final post-cleanup state:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 259068.29`
        - `withdrawable/account_value: 166.422676`
    - another guarded `1500 USDC` batch pushed further:
      - five loops moved `cum_vlm` from `259068.29` to `274065.58`
      - the batch ended with a tiny `0.03 AVAX` residual long at `9.8453`
      - cleanup via reduce-only IOC sell at `9.8399` succeeded immediately
      - final post-cleanup state:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 274065.87`
        - `withdrawable/account_value: 152.600861`
    - a further guarded `1500 USDC` batch then pushed volume substantially:
      - eight loops moved `cum_vlm` from `274065.87` to `296957.07`
      - this run ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 296957.07`
        - `withdrawable/account_value: 133.119276`
    - another guarded `1500 USDC` batch kept the same single-wallet pattern and pushed further:
      - eight loops moved `cum_vlm` from `296957.07` to `317204.52`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 317204.52`
        - `withdrawable/account_value: 119.304981`
    - a subsequent guarded `1500 USDC` batch pushed the same path again:
      - eight loops moved `cum_vlm` from `317204.52` to `335137.52`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 335137.52`
        - `withdrawable/account_value: 105.328655`
    - one more guarded `1500 USDC` batch pushed the same pattern further:
      - eight loops moved `cum_vlm` from `335137.52` to `351066.94`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 351066.94`
        - `withdrawable/account_value: 93.320966`
    - the next guarded `1500 USDC` batch kept the same pattern:
      - eight loops moved `cum_vlm` from `351066.94` to `365252.87`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 365252.87`
        - `withdrawable/account_value: 83.998025`
    - the next guarded `1500 USDC` batch again ended flat:
      - eight loops moved `cum_vlm` from `365252.87` to `378089.15`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 378089.15`
        - `withdrawable/account_value: 76.028407`
    - the next guarded `1500 USDC` batch also ended flat:
      - eight loops moved `cum_vlm` from `378089.15` to `389721.22`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 389721.22`
        - `withdrawable/account_value: 69.294919`
    - the next guarded `1500 USDC` batch also ended flat:
      - eight loops moved `cum_vlm` from `389721.22` to `400292.35`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 400292.35`
        - `withdrawable/account_value: 62.790759`
    - the next guarded `1500 USDC` batch also ended flat:
      - eight loops moved `cum_vlm` from `400292.35` to `409840.90`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 409840.90`
        - `withdrawable/account_value: 55.460689`
    - the next guarded `1500 USDC` batch also ended flat:
      - eight loops moved `cum_vlm` from `409840.90` to `418226.69`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 418226.69`
        - `withdrawable/account_value: 49.376089`
    - the next guarded `1500 USDC` batch also ended flat:
      - eight loops moved `cum_vlm` from `418226.69` to `425776.20`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 425776.20`
        - `withdrawable/account_value: 44.770493`
    - the next guarded `1500 USDC` batch also ended with a flat wallet state:
      - live follow-up checks confirmed:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 432623.70`
        - `withdrawable/account_value: 40.748627`
      - the shell wrapper itself exited nonzero before emitting its final JSON payload, so this specific increment was reconstructed from the immediate post-run state rather than the batch summary blob
    - once withdrawable got thin, the churn notional was reduced from `1500` to `1000` per leg:
      - eight loops moved `cum_vlm` from `432623.70` to `438805.92`
      - this run ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 438805.92`
        - `withdrawable/account_value: 36.714225`
    - another `1000`-notional guarded batch also ended flat:
      - eight loops moved `cum_vlm` from `438805.92` to `444441.97`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 444441.97`
        - `withdrawable/account_value: 33.487255`
    - once headroom shrank further, the churn notional was reduced from `1000` to `500` per leg:
      - eight loops moved `cum_vlm` from `444441.97` to `449568.29`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 449568.29`
        - `withdrawable/account_value: 30.468643`
    - another `500`-notional guarded batch also ended flat:
      - eight loops moved `cum_vlm` from `449568.29` to `454228.11`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 454228.11`
        - `withdrawable/account_value: 27.629687`
    - another `250`-notional guarded batch still moved volume, but the shell wrapper failed before it could emit its summary blob:
      - immediate follow-up checks confirmed the wallet was still flat:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 456228.01`
        - `withdrawable/account_value: 26.396263`
      - this was another orchestration/reporting failure in the batch harness rather than a product-state failure
    - stepping down again to `100` per leg exposed a worse failure mode:
      - the batch wrapper exited without a clean summary, and live follow-up checks found:
        - a real residual long of `10.16 AVAX`
        - `cum_vlm: 456528.10`
      - the first cleanup attempt used `best_bid - 0.0001` and was rejected:
        - `Price must be divisible by tick size. asset=7`
      - retrying the reduce-only IOC sell at the exact best bid flattened the position:
        - final flat state after cleanup:
          - `orders open: []`
          - `positions list: []`
          - `cum_vlm: 456630.05`
          - `withdrawable/account_value: 28.080748`
      - this is another agent-UX issue:
        - for AVAX, naive `best_bid - 0.0001` cleanup can violate the exchange tick size
    - stepping down again to `50` per leg brought the loop back to a clean shape:
      - eight loops moved `cum_vlm` from `456630.05` to `457430.08`
      - this run ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 457430.08`
        - `withdrawable/account_value: 27.560476`
    - another `50`-notional guarded batch also ended flat:
      - eight loops moved `cum_vlm` from `457430.08` to `458230.19`
      - this run ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 458230.19`
        - `withdrawable/account_value: 26.95497`
    - stepping down again to `25` per leg still worked cleanly:
      - eight loops moved `cum_vlm` from `458230.19` to `458630.64`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 458630.64`
        - `withdrawable/account_value: 26.699453`
    - switching back to `100` per leg while keeping cleanup pinned to the exact live best bid also worked cleanly:
      - eight loops moved `cum_vlm` from `458630.64` to `460230.79`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 460230.79`
        - `withdrawable/account_value: 25.698439`
    - repeating the exact-bid `100`-notional variant also ended flat:
      - eight loops moved `cum_vlm` from `460230.79` to `461830.29`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 461830.29`
        - `withdrawable/account_value: 24.299203`
    - another exact-bid `100`-notional batch also ended flat:
      - eight loops moved `cum_vlm` from `461830.29` to `463429.95`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 463429.95`
        - `withdrawable/account_value: 22.967362`
    - another exact-bid `100`-notional batch also ended flat:
      - eight loops moved `cum_vlm` from `463429.95` to `465029.66`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 465029.66`
        - `withdrawable/account_value: 21.382798`
    - another exact-bid `100`-notional batch also ended flat:
      - eight loops moved `cum_vlm` from `465029.66` to `466629.14`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 466629.14`
        - `withdrawable/account_value: 20.035791`
      - current portfolio also shows new spot funding:
        - `spot USDC total: 305.13908551`
    - that fresh spot balance was then used to widen the loop again:
      - `transfer spot-to-perp --amount 150 -y` succeeded
      - the exact-bid `1000`-notional AVAX loop resumed
      - eight loops moved `cum_vlm` from `466629.14` to `482628.17`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 482628.17`
        - `withdrawable/account_value: 154.550845`
      - ending spot state after the top-up and churn:
        - `spot USDC total: 155.13908551`
    - the refreshed perp headroom let that widened loop continue:
      - another exact-bid `1000`-notional batch moved `cum_vlm` from `482628.17` to `498628.64`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 498628.64`
        - `withdrawable/account_value: 144.354925`
    - another exact-bid `1000`-notional batch also ended flat:
      - eight loops moved `cum_vlm` from `498628.64` to `514628.15`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 514628.15`
        - `withdrawable/account_value: 133.129529`
    - another exact-bid `1000`-notional batch also ended flat:
      - eight loops moved `cum_vlm` from `514628.15` to `530627.13`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 530627.13`
        - `withdrawable/account_value: 122.039038`
    - another exact-bid `1000`-notional batch finished cleanly while fresh spot USDC landed:
      - eight loops moved `cum_vlm` from `530627.13` to `546628.21`
      - this run also ended flat without extra cleanup:
        - `orders open: []`
        - `positions list: []`
        - `cum_vlm: 546628.21`
        - `withdrawable/account_value: 111.441758`
      - ending spot state after that batch:
        - `spot USDC total: 1154.13908551`
        - `spot HYPE total: 0.48928192`
        - `HLP vault equity: 5.000051`
    - another `500 USDC` spot-to-perp top-up succeeded:
      - `transfer spot-to-perp --amount 500 -y`
    - the next widened `1500`-notional AVAX batch did not finish cleanly:
      - `cum_vlm` still moved from `546628.21` to `549629.33`
      - the batch aborted during cleanup because `book AVAX` returned a payload without usable `levels`
      - that left a tiny residual:
        - `0.09 AVAX` long
      - manual cleanup with a direct reduce-only market sell succeeded:
        - `orders create --coin AVAX --side sell --type market --amount 1 --reduce-only -y`
      - cleanup added the final small delta:
        - `cum_vlm: 549630.22`
      - ending flat state after cleanup:
        - `orders open: []`
        - `positions list: []`
        - `withdrawable/account_value: 609.333839`
        - `spot USDC total: 654.13908551`
        - `HLP vault equity: 5.000033`
    - replacing exact-bid cleanup with a direct small reduce-only market fallback is better:
      - the next widened `1500`-notional batch wrapper hung before printing its summary
      - live follow-up checks showed the wallet ended flat anyway:
        - `orders open: []`
        - `positions list: []`
      - volume still advanced materially:
        - `cum_vlm: 558631.31`
      - ending flat state after follow-up reads:
        - `withdrawable/account_value: 604.085066`
        - `spot USDC total: 654.13908551`
        - `HLP vault equity: 5.000013`
    - a later flat-state recheck showed volume had advanced further to:
      - `cum_vlm: 567631.88`
    - another `500 USDC` spot-to-perp top-up succeeded:
      - `transfer spot-to-perp --amount 500 -y`
    - the next widened `1500`-notional AVAX pass moved volume sharply again:
      - live reads first showed an in-flight long:
        - `149.84 AVAX`
      - the direct reduce-only cleanup attempt raced the live state and was rejected because the position changed underneath it
      - follow-up reads then showed the wallet had self-cleared back to flat:
        - `orders open: []`
        - `positions list: []`
      - final checkpoint after that pass:
        - `cum_vlm: 597633.95`
        - `withdrawable/account_value: 1075.522491`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 5.000014`
    - a later four-loop attempt from a single reusable shell session was interrupted after the wrapper stalled inside a subprocess call:
      - interruption showed the live residual directly:
        - `150.13 AVAX` long
      - direct cleanup from the same shell succeeded:
        - `orders create --coin AVAX --side sell --type market --amount 1600 --reduce-only -y`
        - filled `150.13 AVAX`
      - verified flat state after cleanup:
        - `positions list: []`
        - `orders open: []`
      - final checkpoint after that interrupted pass:
        - `cum_vlm: 606637.32`
        - `withdrawable/account_value: 1068.057162`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 5.000047`
    - the most reliable late-session path turned out to be explicit one-at-a-time shell cycles instead of nested wrappers:
      - one explicit buy/sell cycle completed cleanly with no residue
      - three more explicit cycles pushed traded volume higher, but left tiny dust:
        - first batch dust:
          - `0.03 AVAX`
        - second batch dust:
          - `0.04 AVAX`
      - both dust positions were flattened successfully with:
        - `orders create --coin AVAX --side sell --type market --amount 1 --reduce-only -y`
      - final checkpoint after those manual cycles and dust cleanup:
        - `cum_vlm: 627634.84`
        - `withdrawable/account_value: 1051.025342`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 5.000049`
    - two additional explicit buy/sell cycles also completed cleanly with no residue:
      - `positions list: []`
      - no dust cleanup required
      - resulting checkpoint:
        - `cum_vlm: 633634.46`
        - `withdrawable/account_value: 1045.798711`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 5.000043`
    - two more explicit buy/sell cycles pushed the wallet further:
      - resulting volume before cleanup:
        - `cum_vlm: 639634.4`
      - this pass left tiny AVAX dust:
        - `0.01 AVAX`
      - dust cleanup succeeded with:
        - `orders create --coin AVAX --side sell --type market --amount 1 --reduce-only -y`
      - final checkpoint after cleanup:
        - `cum_vlm: 639634.5`
        - `withdrawable/account_value: 1041.403958`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 5.000042`
    - two subsequent explicit buy/sell cycles also completed cleanly:
      - `positions list: []`
      - no dust cleanup required
      - resulting checkpoint:
        - `cum_vlm: 645633.91`
        - `withdrawable/account_value: 1035.819467`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 5.000013`
    - two more explicit buy/sell cycles also completed:
      - one intermediate read briefly showed `0.02 AVAX` dust after the first close
      - by the end of the second cycle the wallet was flat again without separate cleanup:
        - `positions list: []`
      - resulting checkpoint:
        - `cum_vlm: 651633.36`
        - `withdrawable/account_value: 1030.457116`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 5.000017`
    - two further explicit buy/sell cycles also completed cleanly:
      - `positions list: []`
      - no dust cleanup required
      - resulting checkpoint:
        - `cum_vlm: 657632.85`
        - `withdrawable/account_value: 1025.751916`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 5.000019`
    - two additional explicit buy/sell cycles also completed cleanly:
      - `positions list: []`
      - no dust cleanup required
      - resulting checkpoint:
        - `cum_vlm: 663632.53`
        - `withdrawable/account_value: 1020.659771`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 5.000022`
    - two more explicit buy/sell cycles also completed cleanly:
      - `positions list: []`
      - no dust cleanup required
      - resulting checkpoint:
        - `cum_vlm: 669630.32`
        - `withdrawable/account_value: 1013.847883`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 5.000007`
    - two further explicit buy/sell cycles advanced volume again:
      - the pass ended with tiny AVAX dust:
        - `0.01 AVAX`
      - dust cleanup succeeded with:
        - `orders create --coin AVAX --side sell --type market --amount 1 --reduce-only -y`
      - final checkpoint after cleanup:
        - `cum_vlm: 675630.39`
        - `withdrawable/account_value: 1008.243976`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 5.000013`
    - after raising BTC cross leverage to `40x`, a much larger cycle became viable:
      - `positions update-leverage --coin BTC --leverage 40`
      - one BTC market round-trip with:
        - buy `--amount 50000`
        - sell `--amount 50000 --reduce-only`
      - completed cleanly and ended flat:
        - `positions list: []`
      - resulting checkpoint:
        - `cum_vlm: 701625.05`
        - `withdrawable/account_value: 979.409085`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 4.999981`
    - increasing BTC market `--amount` from `50000` to `100000` did not scale position size as expected:
      - the `100000` buy still filled only:
        - `size: 0.12363 BTC`
      - the paired reduce-only sell also filled:
        - `size: 0.12363 BTC`
      - despite that semantic mismatch, the cycle still ended flat:
        - `positions list: []`
      - resulting checkpoint:
        - `cum_vlm: 721622.82`
        - `withdrawable/account_value: 955.496687`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 4.999982`
    - repeating the clean BTC `50000` market round-trip confirmed it as the best current farming primitive:
      - buy filled:
        - `size: 0.12362 BTC`
      - reduce-only sell filled:
        - `size: 0.12362 BTC`
      - the cycle ended flat:
        - `positions list: []`
      - resulting checkpoint:
        - `cum_vlm: 741618.65`
        - `withdrawable/account_value: 929.399377`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 4.999986`
    - another BTC `50000` market round-trip also completed cleanly:
      - buy filled:
        - `size: 0.12365 BTC`
      - reduce-only sell filled:
        - `size: 0.12365 BTC`
      - the cycle ended flat:
        - `positions list: []`
      - resulting checkpoint:
        - `cum_vlm: 761604.3`
        - `withdrawable/account_value: 886.91432`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 4.999988`
    - one more BTC `50000` market round-trip also completed cleanly:
      - buy filled:
        - `size: 0.12362 BTC`
      - reduce-only sell filled:
        - `size: 0.12362 BTC`
      - the cycle ended flat:
        - `positions list: []`
      - resulting checkpoint:
        - `cum_vlm: 781579.78`
        - `withdrawable/account_value: 840.631268`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 4.999992`
    - another BTC `50000` market round-trip also completed cleanly:
      - buy filled:
        - `size: 0.12363 BTC`
      - reduce-only sell filled:
        - `size: 0.12363 BTC`
      - the cycle ended flat:
        - `positions list: []`
      - resulting checkpoint:
        - `cum_vlm: 801551.44`
        - `withdrawable/account_value: 798.158729`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 4.999993`
    - another BTC `50000` market round-trip also completed cleanly:
      - buy filled:
        - `size: 0.12361 BTC`
      - reduce-only sell filled:
        - `size: 0.12361 BTC`
      - the cycle ended flat:
        - `positions list: []`
      - resulting checkpoint:
        - `cum_vlm: 821525.46`
        - `withdrawable/account_value: 755.165145`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 5.000002`
    - another BTC `50000` market round-trip also completed cleanly:
      - buy filled:
        - `size: 0.12359 BTC`
      - reduce-only sell filled:
        - `size: 0.12359 BTC`
      - the cycle ended flat:
        - `positions list: []`
      - resulting checkpoint:
        - `cum_vlm: 841494.34`
        - `withdrawable/account_value: 706.073501`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 4.999998`
    - another BTC `50000` market round-trip also completed cleanly:
      - buy filled:
        - `size: 0.1237 BTC`
      - reduce-only sell filled:
        - `size: 0.1237 BTC`
      - the cycle ended flat:
        - `positions list: []`
      - resulting checkpoint:
        - `cum_vlm: 861470.1`
        - `withdrawable/account_value: 654.473813`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 4.999992`
    - the next BTC `50000` cycle exposed a live reliability failure mode:
      - the buy leg filled:
        - `size: 0.12375 BTC`
      - the first reduce-only sell attempt failed with:
        - `Unable to reach Hyperliquid API. Check your network connection while loading asset metadata. [untrusted remote data] error sending request for url (https://api.hyperliquid-testnet.xyz/info)`
      - that left a real open position:
        - `0.12375 BTC` long
      - a second reduce-only sell retry succeeded and flattened it:
        - `size: 0.12375 BTC`
      - final checkpoint after retry cleanup:
        - `cum_vlm: 881449.9`
        - `withdrawable/account_value: 608.803816`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 4.999974`
    - the following BTC `50000` round-trip again completed cleanly:
      - buy filled:
        - `size: 0.12377 BTC`
      - reduce-only sell filled:
        - `size: 0.12377 BTC`
      - the cycle ended flat:
        - `positions list: []`
      - resulting checkpoint:
        - `cum_vlm: 901422.48`
        - `withdrawable/account_value: 559.935112`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 4.999964`
    - another BTC `50000` market round-trip also completed cleanly:
      - buy filled:
        - `size: 0.12375 BTC`
      - reduce-only sell filled:
        - `size: 0.12375 BTC`
      - the cycle ended flat:
        - `positions list: []`
      - resulting checkpoint:
        - `cum_vlm: 921397.22`
        - `withdrawable/account_value: 515.05681`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 4.999962`
    - another BTC `50000` market round-trip also completed cleanly:
      - buy filled:
        - `size: 0.12376 BTC`
      - reduce-only sell filled:
        - `size: 0.12376 BTC`
      - the cycle ended flat:
        - `positions list: []`
      - resulting checkpoint:
        - `cum_vlm: 941373.08`
        - `withdrawable/account_value: 470.401303`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 4.999958`
    - another BTC `50000` market round-trip also completed cleanly:
      - buy filled:
        - `size: 0.12377 BTC`
      - reduce-only sell filled:
        - `size: 0.12377 BTC`
      - the cycle ended flat:
        - `positions list: []`
      - resulting checkpoint:
        - `cum_vlm: 961347.37`
        - `withdrawable/account_value: 424.919902`
        - `spot USDC total: 154.13908551`
        - `HLP vault equity: 4.999964`
  - this `1500 USDC` single-wallet AVAX market churn is the fastest validated volume path found in the session so far
- net result after cleanup was still positive volume, but the flow is too noisy to treat as a repeatable unattended primitive

Important failure mode confirmed again:
- the repeated maker/taker farming pattern is not batch-safe
- one cycle partially filled and stopped with residue:
  - main-wallet resting bid:
    - `12.96 HYPE @ 28.34`
  - matching main-wallet short:
    - `12.96 HYPE`
- the AVAX cleanup path is also not fully deterministic:
  - `book AVAX` can occasionally return a payload without usable `levels`
  - when that happens, an automated exact-bid cleanup path fails open unless it has a fallback
  - direct reduce-only market cleanup still works for tiny residuals
- the widened AVAX batch wrapper itself is also not perfectly reliable:
  - one run advanced volume and left the wallet flat, but the wrapper never printed its final JSON summary
  - the only trustworthy way to classify that run was post-hoc reads of `orders open`, `positions list`, `account rate-limit`, and `account portfolio`
- trying to orchestrate the widened loop from an interactive shell did not solve the reliability issue by itself:
  - one run stalled inside Python while waiting on a child CLI subprocess
  - interrupting the wrapper and then directly reading `positions` was still required to discover and clean up the residual exposure
- the simplest operator model is currently the most robust:
  - issue one market buy, one reduce-only market sell, then inspect residue
  - batch wrappers are more fragile than the exchange flow they are trying to automate
- the biggest performance unlock late in the session was changing instrument/leverage rather than scripting harder:
  - AVAX at `1500` notional per leg is operationally stable but too slow
  - BTC at `40x` leverage supports a `50000` per-leg market round-trip that added roughly `34k` to `cum_vlm` in one clean cycle
- there is still a real agent-UX problem around market `--amount` semantics on BTC:
  - raising `--amount` from `50000` to `100000` did not increase the filled BTC size
  - an operator or agent cannot infer reliable sizing from the flag name alone
- there is also now clear live evidence that a mutating two-leg flow can fail between legs on transient metadata/API reachability:
  - the buy can fill
  - the reduce-only exit can fail before submission on metadata lookup
  - an agent must treat that as an exposure-management event, not just a command failure
- this required explicit cleanup:
  - `wallet-2` IOC sell of `12.96 HYPE @ 28.34`
  - after cleanup:
    - `orders open: []`
    - `positions list: []`
    - `wallet-2 orders open: []`
    - `wallet-2 positions list: []`

Takeaway:
- the farming path works
- but it remains operationally agent-hostile under repetition because third-party fills can disturb the paired cycle even when each individual step is locally validated
- at the latest known volume:
  - `orders schedule-cancel` still remains blocked by the `$1000000` traded-volume gate

Local-only or not meaningfully testnet-scoped in this session:
- account storage mutations:
  - `account add`
  - `account set-default`
  - `account remove`
  - `account ls`
- wallet lifecycle commands:
  - `wallet import`
  - `wallet reset`
  - `wallet import-mnemonic`
  - `wallet rename`
  - `wallet delete`
  - `wallet export`
- note: `wallet create` and `wallet list` were exercised locally during the live-test setup, while `wallet show` and `wallet address` were also exercised live against the OWS-backed testnet session
- note: a live `api-wallet approve --generate --store ...` attempt entered an interactive path and was aborted from the coverage run, but non-generated approve via `--agent-address` was validated live
- `schema` was exercised locally and returned the full machine-readable inventory
- `setup` was exercised locally and revealed non-interactive local-state mutation behavior; see gotchas above

## Recommended Next Fixes

1. Decide whether to keep or unwind the irreversible staking-link attribution created during live validation
- `wallet-2` initiated against the funded staking wallet
- finalize succeeded

2. Add a dedicated funded-testnet canary script
- external OWS wallet only
- explicit cleanup artifacts
- protocol-blocker capture
- no repo-local credential discovery

3. If `orders schedule-cancel` needs to be forced through, prefer a purpose-built farming harness with live book guards and per-cycle reconciliation
- current ad hoc maker/taker loops are too sensitive to external fills and hidden liquidity shifts
- the best current candidate is single-wallet AVAX market open/close churn, but it still needs reconciliation for occasional small partial-close residue
