# Execution strategies

Parent-order strategy execution for Hyperliquid trading. The `execution` command family sits above raw `orders` commands and decomposes a trading intent into timed child orders managed by the local CLI engine.

## Commands

| Command | Description | Implementation |
|---------|-------------|---------------|
| `execution strategies` | List supported strategies and metadata (read-only) | `src/commands/execution/` |
| `execution plan` | Validate strategy parameters, emit parent-order plan (read-only) | `src/commands/execution/` |
| `execution submit` | Submit a plan; expands into child orders | `src/commands/execution/` |
| `execution status` | Report parent/child progress and recovery state | `src/commands/execution/` |
| `execution cancel` | Cancel active children, mark parent canceled | `src/commands/execution/` |
| `execution recover` | Reconcile child orders/fills after interruption | `src/commands/execution/` |

## Raw orders vs parent-order strategies

| Aspect | Raw orders (`orders create`, etc.) | Execution strategies (`execution plan/submit`) |
|--------|-----------------------------------|----------------------------------------------|
| Lifecycle | Submit once, exchange manages | CLI engine manages child scheduling and state |
| State | No local state | SQLite-backed parent/child state with CLOIDs |
| Recovery | N/A (exchange is authority) | `execution recover` reconciles after interruption |
| Privacy | Orders are public on HyperCore | Parent intent is local; child orders become public when submitted |
| Auth required | Yes (signing) | Planning is auth-free; submission requires signing |
| Strategy controls | None (single order) | Urgency, strictness, slippage, clip sizing, participation rate, etc. |

Native `orders twap-create` delegates to the Hyperliquid server TWAP with fixed 30-second suborders and 3% max slippage. Quote-style `execution plan --strategy twap` uses the local engine with configurable slice count, duration, randomization, and variance controls. Use native TWAP for simple execution; use Quote-style TWAP when you need local policy controls.

## Supported strategies

| Strategy | Description | Live data | Adaptive |
|----------|-------------|-----------|----------|
| Chase | Passive limit tracks best bid/offer, optionally escalates | Book updates | Partial (reprice) |
| TWAP | Time-weighted slices across a duration | No | Optional (measured) |
| VWAP | Volume-curve-driven sizing and timing | Optional | Optional |
| POV | Percent-of-volume clips based on real-time volume | Volume observations | Yes |
| Iceberg | Hidden quantity with display-size clips and replenish | No | Partial (fill-triggered) |

## Privacy and visibility

- **Parent orders are local/engine-private.** The parent intent, strategy parameters, and child schedule exist only in local CLI state and plan output. They are never submitted to the Hyperliquid API.
- **Child orders are public HyperCore actions.** Once submitted, child orders are visible just like any `orders create` order.
- The CLI does **not** promise hidden execution, cryptographic privacy, or guaranteed anti-front-running for submitted child orders.

## Execution state

Parent/child execution state is stored in SQLite, scoped by network and account context. State includes identifiers, statuses, fills, and recovery metadata — but **never** private keys, mnemonics, passphrases, or reusable signed payloads. See [Recovery and reconciliation](../../docs/execution/recovery-and-reconciliation.md).

## Builder fees in execution

Builder fee context (`--builder` + `--builder-fee-rate`) is frozen in the plan and attached to every child order. It is immutable from plan through submit/status/recover. The master account must approve the builder before live submission; API wallets cannot approve builder fees. See [Agent wallets and builder fees in execution](../../docs/execution/agent-wallets-and-builder-fees.md).

## Documentation

| Document | Description |
|----------|-------------|
| [Quote-style execution overview](../../docs/execution/quote-style-execution.md) | Strategy overview, raw orders vs strategies, privacy, safety |
| [Strategy parameter reference](../../docs/execution/strategy-parameters.md) | Full parameter reference with units, defaults, bounds, and examples |
| [Recovery and reconciliation](../../docs/execution/recovery-and-reconciliation.md) | State storage, recovery, cancel, and terminal states |
| [Agent wallets and builder fees](../../docs/execution/agent-wallets-and-builder-fees.md) | API wallet authority, builder fees, signer sources in execution |
| [Trading and execution issues](../../docs/support/trading-and-execution-issues.md) | Troubleshooting stuck children, partial fills, rate limits, etc. |
| [Deposits, withdrawals, compromised accounts](../../docs/support/deposits-withdrawals-and-compromised-accounts.md) | Fund movement issues and key compromise response |

## Entry points for modification

- **Add a new strategy**: add variant to `ExecutionStrategy`, extend planning/validation, add strategy-specific parameters
- **Change strategy controls**: modify shared parameter handling in execution planning
- **Add engine behavior**: extend execution engine with strategy-specific child scheduling logic
