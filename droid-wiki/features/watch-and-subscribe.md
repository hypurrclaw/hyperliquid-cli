# Watch and subscribe

Active contributors: Sayo

Real-time data display via terminal watch mode and WebSocket subscriptions.

## Watch mode

The `-w` / `--watch` flag enables live-updating terminal display for snapshot commands. Available on: `book`, `mids`, `candles`, `orders open`, `positions list`.

```bash
hyperliquid book BTC -w
hyperliquid mids -w
hyperliquid --testnet orders open -w --max-ticks 10
```

### How it works

`src/watch.rs` implements two modes:

1. **Snapshot watch** (`SnapshotWatchRenderMode`): polls the API at 2-second intervals (`WATCH_REFRESH_INTERVAL`) and re-renders in-place using crossterm's alternate screen
2. **WebSocket subscription** (`SubscribeEventKind`): connects to Hyperliquid's WebSocket and processes streaming events

For human formats (pretty/table), watch mode enters crossterm's alternate screen with raw mode for keyboard input. Press `q` or `Ctrl-C` to exit. For JSON mode, each snapshot is emitted as a newline-delimited JSON line.

### Watch bounds

- `--max-ticks <N>`: stop after N snapshots (useful for automation)
- `HYPERLIQUID_WATCH_MAX_TICKS`: environment variable default for max ticks

## WebSocket subscriptions

The `subscribe` command group streams real-time events:

| Command | Event kind | Description |
|---------|-----------|-------------|
| `subscribe trades --asset <COIN>` | `Trades` | Real-time trade feed |
| `subscribe orderbook --asset <COIN>` | `Orderbook` | L2 order book updates |
| `subscribe candles --asset <COIN> [--interval]` | `Candles` | Candle updates |
| `subscribe all-mids` | `AllMids` | All mid price updates |
| `subscribe order-updates` | `OrderUpdates` | Order status updates (authenticated) |
| `subscribe fills` | `Fills` | Fill events (authenticated) |

### Subscription bounds

- `--max-events <N>`: stop after emitting N matching events
- `--idle-timeout-ms <MILLISECONDS>`: stop if no matching events for the specified duration

### Key abstractions

| Type | File | Description |
|------|------|-------------|
| `SubscribeEventKind` | `src/watch.rs` | Enum of supported subscription event families |
| `SnapshotWatchArgs` | `src/main.rs` | Clap args for `-w`, `--max-ticks` |
| `SubscribeStreamArgs` | `src/main.rs` | Clap args for `--max-events`, `--idle-timeout-ms` |
| `WatchInput` | `src/watch.rs` | Trait for polling next event (timer, WebSocket message, keypress) |
| `WatchOutput` | `src/watch.rs` | Trait for emitting JSON lines or rendering TUI |
| `TerminalGuard` | `src/watch.rs` | RAII guard for alternate screen entry/exit and raw mode |

## Agent usage

In JSON/agent contexts, watch output must be bounded. Set `--max-ticks` for snapshot watch and `--max-events` / `--idle-timeout-ms` for subscriptions:

```bash
hyperliquid --format json mids -w --max-ticks 5
hyperliquid --format json subscribe trades --asset BTC --max-events 10
```

## Entry points for modification

- **Add watch support to a new command**: implement the polling function, add `SnapshotWatchArgs` to clap args
- **Add a new subscription type**: add variant to `SubscribeEventKind`, implement event matching in `subscription_event_matches`
- **Change watch rendering**: modify the TUI rendering logic in `src/watch.rs`
