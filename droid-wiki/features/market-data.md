# Market data

Read-only commands for prices, books, candles, funding, metadata, and asset discovery. All accept `--format pretty|table|json` and the standard agent flags.

## Top-level commands

| Command | Purpose |
|---------|---------|
| `status` | API health and rate-limit status |
| `meta` | Raw exchange metadata |
| `mids [--watch]` | All mid prices (HIP-3 keys are DEX-qualified, e.g. `xyz:TSLA`) |
| `book <COIN> [--watch]` | L2 order book |
| `candles <COIN> [--interval --limit --watch]` | Candle history |
| `spread <COIN>` | Bid-ask spread |
| `funding <COIN>` | Current funding rate |
| `perps list [--dex]` | Perpetual markets, optionally scoped to a HIP-3 DEX |
| `perps get <COIN> [--dex]` | Perp details |
| `spot list` | Spot markets |
| `spot get <PAIR>` | Spot pair details |
| `outcomes list [--limit]` | Active outcome market sides |
| `outcomes get <NOTATION>` | Outcome details (`#N` / `+N`) |
| `asset decode <RAW_ID>` | Decode a raw protocol asset id |
| `asset search <QUERY>` | Search assets by symbol/title/slug/notation/protocol-id |

## Asset query parsing

`src/commands/mod.rs::parse_asset_query` accepts four formats:

| Input | Variant |
|-------|---------|
| `BTC` | `AssetQuery::Perp("BTC")` (default perpetual) |
| `PURR/USDC` | `AssetQuery::Spot("PURR/USDC")` |
| `xyz:TSLA` | `AssetQuery::Hip3 { dex: "xyz", token: "TSLA" }` (HIP-3 DEX-qualified perp) |
| `#10` or `+10` | `AssetQuery::Outcome("#10")` |

`AssetResolver` looks up the typed asset against a `MetadataCache` (60-second TTL via `METADATA_TTL`) and surfaces fuzzy-match suggestions on misses via `CliError::AssetNotFound { suggestions }`.

## HIP-3 specifics

HIP-3 DEXes are builder-specific; the symbol `xyz` is one example. Mids keys are DEX-qualified (`xyz:TSLA`), and orders on a HIP-3 market need margin in the `dex:<DEX>` context (see [transfers](transfers.md)).

```bash
hyperliquid --format json perps list --dex xyz
hyperliquid --format json perps get TSLA --dex xyz
hyperliquid --format json book xyz:TSLA
```

## Candle intervals

`hypersdk::hypercore::CandleInterval` covers `1m`, `3m`, `5m`, `15m`, `30m`, `1h`, `2h`, `4h`, `8h`, `12h`, `1d`, `3d`, `1w`, `1M`. `--limit` is capped to protect the agent's context (see `parse_candle_limit` in `src/commands/orderbook.rs`).

## Watch mode

`--watch` on snapshot commands re-renders the snapshot every two seconds in the terminal alternate screen, or emits NDJSON when `--format json` is set. See [systems/watch-and-streaming](../systems/watch-and-streaming.md). Agent callers must use `--max-ticks` or `HYPERLIQUID_WATCH_MAX_TICKS`.

## Entry points for modification

- To support a new asset notation, extend `parse_asset_query` and add the corresponding `AssetResolver` lookup branch.
- To add a new market-data view, route the query through `http_api::post_info_json` and add a renderer in the relevant `src/commands/*.rs` module.

See also: [agent-output-contract](agent-output-contract.md), [orders](orders.md).
