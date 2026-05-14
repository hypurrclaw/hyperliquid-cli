# Market data

Active contributors: Sayo

Read-only commands for querying Hyperliquid market state. No authentication required.

## Commands

| Command | Description | Implementation |
|---------|-------------|---------------|
| `perps list [--dex <DEX>]` | List all perpetual markets | `src/commands/perps.rs` |
| `perps get <COIN> [--dex <DEX>]` | Show one perpetual market | `src/commands/perps.rs` |
| `spot list` | List all spot markets | `src/commands/spot.rs` |
| `spot get <PAIR>` | Show one spot pair (e.g., `PURR/USDC`) | `src/commands/spot.rs` |
| `book <COIN> [-w]` | L2 order book snapshot or watch updates | `src/commands/orderbook.rs` |
| `mids [-w]` | All mid prices | `src/commands/orderbook.rs` |
| `candles <COIN> [--interval] [--limit] [-w]` | Candle history | `src/commands/orderbook.rs` |
| `spread <COIN>` | Bid, ask, and spread | `src/commands/orderbook.rs` |
| `funding <COIN>` | Current and predicted funding rate | `src/commands/orderbook.rs` |
| `meta` | Raw exchange metadata | `src/commands/meta.rs` |
| `status` | API health and rate-limit context | `src/commands/status.rs` |
| `outcomes list [--limit <N>]` | List active outcome market sides | `src/commands/outcomes.rs` |
| `outcomes get <NOTATION>` | Show outcome side metadata (`#N` or `+N`) | `src/commands/outcomes.rs` |

## Key abstractions

| Type | File | Description |
|------|------|-------------|
| `AssetQuery` | `src/commands/mod.rs` | Parsed asset input: `Perp`, `Spot`, `Hip3` (DEX-qualified), or `Outcome` |
| `parse_asset_query` | `src/commands/mod.rs` | Parses `BTC`, `PURR/USDC`, `dex:TOKEN`, `#10`/`+10` notation |
| `AssetResolver` | `src/commands/mod.rs` | Trait for resolving asset names to on-chain metadata |
| `ResolvedAsset` | `src/commands/mod.rs` | `Perp(PerpAsset)` or `Spot(SpotAsset)` with index and decimals |
| `MetadataCache` | `src/commands/mod.rs` | 60-second cache for exchange metadata to avoid repeated fetches |

## Asset resolution

The CLI supports four asset input formats:

| Format | Example | Resolves to |
|--------|---------|-------------|
| Plain symbol | `BTC` | Default perpetual market |
| Spot pair | `PURR/USDC` | Spot market pair |
| HIP-3 DEX | `dex:TOKEN` | Perpetual market on a specific DEX |
| Outcome notation | `#10`, `+10` | Outcome market side |

Fuzzy matching via Levenshtein distance provides "did you mean?" suggestions when an asset is not found.

## Output format support

All market data commands support `--format pretty|table|json`, `--select` for field projection, and `--results-only` to strip envelopes. Example JSON output:

```bash
hyperliquid --format json --select name,max_leverage perps list
hyperliquid --format json --select coin,price mids
```

## Watch mode

Several commands support `-w` / `--watch` for live-updating terminal display using crossterm's alternate screen. See [Watch and subscribe](watch-and-subscribe.md).

## Entry points for modification

- **Add a new market data command**: implement in `src/commands/orderbook.rs` or a new module, add clap variant in `src/main.rs`, dispatch in `src/cli_runtime.rs`
- **Add a new asset format**: extend `parse_asset_query` in `src/commands/mod.rs`
- **Change metadata caching**: modify `METADATA_TTL` constant in `src/commands/mod.rs`
