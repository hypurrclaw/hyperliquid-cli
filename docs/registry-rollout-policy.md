# Registry Rollout Policy

This policy gates command-spine registry migration work after the MVP contract plateau. It is intentionally stricter for signed and fund-moving commands than for read-only commands.

## Authority

No signed or fund-moving command-family migration may start until this policy is present, the CI-safe rollout gate passes, and the target child issue names its rollout stage, fallback, canary requirements, and rollback criteria.

## Rollout Stages

1. `hidden/internal registry`: registry metadata and handlers may exist, but legacy dispatch remains public behavior. CI must prove schema/MCP/generated artifacts stay in parity.
2. `read-only default`: read-only commands may default to registry-routed handlers after JSON/table/pretty parity and no-local-state tests pass.
3. `testnet mutating canary`: signed or fund-moving commands may run through registry routing only on testnet with a funded QA wallet, recorded artifacts, emitted-ID tracking, and cleanup checks.
4. `mainnet dry-run comparison`: mainnet remains non-submitting. Registry dry-run/confirmation output is compared to legacy intent for the same args.
5. `mainnet opt-in`: live mainnet registry routing is available only behind an explicit per-command opt-in flag or environment gate documented in the child issue.
6. `mainnet default`: mainnet registry routing may become default only after testnet canaries, mainnet dry-run comparison, and rollback coverage have passed for that command family.
7. `legacy removal`: legacy dispatch can be removed only after command-family parity, fallback burn-in, and release guidance are complete.

## Rollback And Legacy Fallback

Every migrated command family must declare one fallback mode:

- `legacy-child`: route back to the existing CLI child-process path.
- `legacy-dispatch`: route back to the existing `src/main.rs` dispatch arm.
- `fail-closed`: disable the migrated live path while keeping dry-run/schema/read-only behavior available.

Rollback is required when any of these criteria are met:

- Registry and legacy dry-run/confirmation previews disagree on signer, query address, network, asset, amount, destination, order IDs, or action type.
- Mocked request-shape tests drift from the legacy request shape without a documented safety break.
- MCP live policy allows a tool outside its declared account/network/tool/mainnet allowlists.
- A testnet canary leaves unexpected open orders, positions, vault exposure, borrow/lend exposure, pending TWAP/schedule-cancel state, or untracked emitted IDs.
- Any command emits unsanitized remote error text or secret-like local data.

## Testnet Canary Artifacts

Manual funded-live canaries must write artifacts under `.qa/registry-rollout-canary-<timestamp>/`. Each canary directory must include:

- `PLAN.md` naming the child issue, command family, rollout stage, fallback mode, signer source, network, and commands under test.
- Per-command JSON transcripts containing argv, network, signer address, query address, command output, exit code, and any emitted IDs.
- `cleanup-orders-open.json` from `orders open`.
- `cleanup-positions-list.json` from `positions list`.
- `cleanup-account-portfolio.json` from `account portfolio`.
- Vault/borrow-lend cleanup evidence when the command family touches vault or borrow/lend state.
- Command-specific emitted-ID cleanup evidence, such as canceled order IDs, TWAP IDs, scheduled-cancel state, API-wallet names, builder approvals, referral codes, vault addresses, or staking transaction context.
- `SUMMARY.md` with pass/fail status, rollback decision, and residual live-gated cases.

## Command-Family Canary Criteria

### Positions Update Leverage And Margin (#53)

- Rollout stage: `testnet mutating canary`.
- Fallback mode: `legacy-dispatch`.
- Commands under canary: `positions update-leverage` and `positions update-margin`.
- Preflight evidence must include dry-run JSON for both commands with matching `network`, signer, query/acting address, resolved market, asset id, leverage or margin mode, amount, and `ntli` where applicable.
- `positions update-leverage` must use the funded QA wallet on testnet and a reversible leverage value for `HL_QA_COIN`; record the submitted `/exchange` action shape and the resulting position/leverage view.
- `positions update-margin` must run only against a deliberately tiny isolated testnet position; record the submitted `/exchange` action shape, the isolated-margin delta, and the close/cleanup action for that position.
- Cleanup evidence must show no unexpected open positions after the canary, plus the standard `orders open`, `positions list`, and `account portfolio` artifacts.
- Roll back if the dry-run preview and submitted action disagree on network, signer, query address, asset id, leverage, `is_cross`, amount, or `ntli`, or if cleanup leaves an unexpected position.

### Staking, Vault, And Borrow/Lend Safety Gates (#54)

- Rollout stage: `testnet mutating canary`.
- Fallback mode: `legacy-dispatch`.
- Commands under canary: `staking delegate`, `staking undelegate`, `staking deposit`, `staking withdraw`, `staking claim-rewards`, `staking link initiate`, `staking link finalize`, `vault deposit`, `vault withdraw`, `borrowlend supply`, and `borrowlend withdraw`.
- Preflight evidence must include dry-run JSON for every command above with matching `network`, signer, acting/query address, target validator or vault or user where applicable, asset, amount or `max`, and declared reversibility.
- `staking withdraw` evidence must show the queued-withdrawal warning text in both dry-run and live confirmation output; `staking link` evidence must show prompt enforcement for live runs without `--yes` and record both initiate/finalize submitted action shapes.
- `vault deposit` and `vault withdraw` canaries must use a deliberately tiny testnet amount and record the submitted `vaultTransfer` shape plus post-action vault/account state before cleanup.
- `borrowlend supply` and `borrowlend withdraw` canaries must use a deliberately tiny reversible reserve amount and record the submitted `borrowLend` action shape plus post-action borrow/lend state before cleanup.
- Cleanup evidence must show no unexpected staking queue drift, vault exposure, or borrow/lend exposure after the canary, plus the standard `orders open`, `positions list`, and `account portfolio` artifacts.
- Roll back if dry-run preview, confirmation output, and submitted action disagree on network, signer, acting/query address, target object, asset, amount or `max`, reversibility, or verified action type, or if cleanup leaves unexpected staking, vault, or borrow/lend state.

### Live Order Create And Cancel (#60)

- Rollout stage: `testnet mutating canary`.
- Fallback mode: `legacy-dispatch`.
- Commands under canary: `orders create`, `orders batch-create`, `orders cancel`, and `orders cancel-all`.
- Preflight evidence must include dry-run JSON for every command above with matching `network`, signer, query/acting address, resolved asset id, side, price, size, order type, time-in-force, grouping, builder fee, client order ID, and vault/subaccount context where applicable.
- `orders create` must place a deliberately tiny reversible testnet order and record the submitted `order` action shape, emitted order ID or client order ID, status lookup, and cancel cleanup.
- `orders batch-create` must use a tiny testnet batch or stay live-gated with an explicit residual-gate note when market conditions make a safe batch unsuitable.
- `orders cancel` must prove both OID and client-order-ID cleanup paths when an emitted ID is available; otherwise the missing branch must stay live-gated in `SUMMARY.md`.
- `orders cancel-all` must run only after the canary has inventoried expected open orders and must record before/after `orders open` evidence.
- Cleanup evidence must show no unexpected open orders or positions after the canary, plus the standard `orders open`, `positions list`, and `account portfolio` artifacts.
- Roll back if dry-run preview, confirmation output, status lookup, and submitted action disagree on network, signer, query/acting address, asset id, side, price, size, order identifier, order type, grouping, builder fee, or vault/subaccount context, or if cleanup leaves an unexpected order or position.

### Remaining Live Order Paths (#61)

- Rollout stage: `testnet mutating canary`.
- Fallback mode: `legacy-dispatch`.
- Commands under canary: `orders modify`, `orders twap-create`, `orders twap-cancel`, and `orders schedule-cancel`.
- Preflight evidence must include dry-run JSON for every command above with matching `network`, signer, query address, resolved asset id, order ID or client order ID, replacement price/size, TWAP side/size/duration, TWAP ID, scheduled-cancel timestamp, and declared action type where applicable.
- `orders modify` must create a deliberately tiny resting testnet order, modify it, record the replacement status or emitted order reference, then cancel the replacement.
- `orders twap-create` must create a tiny short-duration testnet TWAP, record the emitted TWAP ID, and prove `orders twap-cancel` removes that same ID.
- `orders schedule-cancel` must record either a successful scheduled-cancel action plus follow-up state or a structured exchange gate, and must verify no pending schedule-cancel state remains untracked.
- Cleanup evidence must show no unexpected open orders, positions, pending TWAPs, or pending schedule-cancel state after the canary, plus the standard `orders open`, `positions list`, and `account portfolio` artifacts.
- Roll back if dry-run preview, confirmation output, and submitted action disagree on network, signer, query address, asset id, replacement fields, TWAP fields, TWAP ID, scheduled timestamp, or action type, or if cleanup leaves unexpected order/TWAP/schedule state.

## CI-Safe Gates

CI must stay mocked or static unless explicitly scheduled as funded-live QA. CI-safe rollout gates are:

- `cargo test`
- contract characterization tests
- `cargo clippy -- -D warnings`
- `cargo fmt --check`
- `bash scripts/qa-registry-rollout-gates.sh`
- targeted mocked tests for the command family under migration

Funded-live canaries are manual or scheduled QA only. They must not run from default pull-request CI.

## Manual Entry Points

- `task qa:registry-rollout` verifies this policy and its CI hooks.
- `task qa:registry-canary-plan` writes a timestamped `.qa/registry-rollout-canary-*/PLAN.md` checklist for a human-run funded-live testnet canary.
- `task qa:matrix:strict` remains the broad repeatable command sweep.
- `task qa:mcp` remains the MCP stdio smoke gate.
