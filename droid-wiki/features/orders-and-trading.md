# Orders and trading

Active contributors: Sayo

Authenticated commands for order lifecycle and position management. All mutating commands support `--dry-run` and `--testnet`.

## Commands

| Command | Description | Implementation |
|---------|-------------|---------------|
| `orders open [-w]` | List open orders | `src/commands/orders.rs` (queries module) |
| `orders history` | Order history | `src/commands/orders.rs` (queries module) |
| `orders status --user <ADDR> --oid <OID>` | Public order status by ID | `src/commands/orders.rs` (queries module) |
| `orders create` | Create limit, market, stop-loss, take-profit, stop-limit, or take-limit order | `src/commands/orders.rs` (planning + validation) |
| `orders scale` | Create evenly spaced batch of limit orders | `src/commands/orders.rs` (planning module) |
| `orders batch-create` | Create batch of limit orders from JSON file | `src/commands/orders.rs` (planning + validation) |
| `orders tpsl` | Create position-attached TP/SL orders | `src/commands/orders.rs` (planning module) |
| `orders cancel <OID>` | Cancel by order ID | `src/commands/orders.rs` (planning module) |
| `orders cancel --cloid <CLOID>` | Cancel by client order ID | `src/commands/orders.rs` (planning module) |
| `orders cancel-all [--coin <COIN>]` | Cancel all open orders | `src/commands/orders.rs` (planning module) |
| `orders modify <OID>` | Modify an existing order | `src/commands/orders.rs` (planning module) |
| `orders twap-create` | Create a TWAP order | `src/commands/orders.rs` (planning module) |
| `orders twap-cancel` | Cancel a TWAP order | `src/commands/orders.rs` (planning module) |
| `orders schedule-cancel` | Dead man's switch | `src/commands/orders.rs` (planning module) |
| `positions list [-w]` | List open positions | `src/commands/positions.rs` |
| `positions update-leverage` | Update leverage (cross or isolated) | `src/commands/positions.rs` |
| `positions update-margin` | Add or remove isolated margin | `src/commands/positions.rs` |

## Order module structure

The orders module in `src/commands/orders/` is split into 5 sub-modules:

| Module | Purpose |
|--------|---------|
| `args.rs` | Clap argument structs for all order commands |
| `validation.rs` | Input validation: price/size bounds, slippage, batch limits, asset resolution |
| `planning.rs` | Order plan construction, dry-run preview generation, batch preparation |
| `queries.rs` | Read-only order queries (open, history, status) |
| `rendering.rs` | Output structs and `TableData` implementations for order confirmations |

## Order types

| Type | Key args | Description |
|------|----------|-------------|
| `limit` | `--price`, `--size` | Standard limit order at specified price |
| `market` | `--amount` (quote/collateral token) | Market order with optional `--max-slippage-bps` (default 500, max 1000) |
| `stop-loss` | `--trigger-price` | Market trigger order at trigger price |
| `take-profit` | `--trigger-price` | Market trigger order at trigger price |
| `stop-limit` | `--trigger-price`, `--price`, `--size` | Limit order placed when trigger is hit |
| `take-limit` | `--trigger-price`, `--price`, `--size` | Limit order placed when trigger is hit |

## TP/SL grouping

Two grouping modes map to Hyperliquid's signed order grouping:

- `normal-tpsl` → `normalTpsl`: parent order with fixed-size TP/SL children
- `position-tpsl` → `positionTpsl`: TP/SL orders attached to the current position

## Key abstractions

| Type | File | Description |
|------|------|-------------|
| `CreateArgs` | `src/commands/orders/args.rs` | Clap args for `orders create` with all order type variants |
| `PreparedOrder` | `src/commands/orders.rs` | Resolved order with request, asset, side, type, and TIF |
| `OrderDryRunPlan` | `src/commands/orders/planning.rs` | Dry-run preview showing what would be submitted |
| `OrderConfirmation` | `src/commands/orders/rendering.rs` | Post-submission confirmation with OID and status |
| `OrderListRow` | `src/commands/orders/rendering.rs` | Renderable row for open order listings |

## Validation constants

| Constant | Value | Purpose |
|----------|-------|---------|
| `DEFAULT_MARKET_ORDER_SLIPPAGE_BPS` | 500 | Default 5% slippage for market orders |
| `MAX_BATCH_ORDER_COUNT` | 500 | Maximum orders in a batch |
| `MIN_MARKET_ORDER_SLIPPAGE_BPS` | 1 | Minimum slippage |
| `MAX_MARKET_ORDER_SLIPPAGE_BPS` | 1,000 | Maximum slippage (10%) |

## Entry points for modification

- **Add a new order type**: add variant to `CreateOrderType`, extend `validate_create_args`, add planning logic
- **Add a new position command**: add clap variant, implement in `src/commands/positions.rs`
- **Change order validation**: modify constants and validation functions in `src/commands/orders/validation.rs`
