# Debugging

## Read the exit code first

Every non-zero exit maps to a structured cause:

| Code | Meaning |
|------|---------|
| 1 | Internal (anyhow) — unexpected; capture the message |
| 2 | Configuration / clap usage |
| 10 | Auth (missing key, OWS wallet not found, no chain account) |
| 11 | Rate limited — back off |
| 12 | API unavailable / timeout |
| 13 | Unsupported asset, DEX, or parameter |
| 14 | Stale cached data |
| 15 | Partial results (batch with mixed success) |

See [reference/exit-codes](../reference/exit-codes.md).

## Use JSON for parseable errors

```bash
hyperliquid --format json orders create --coin XYZ --side buy --price 1 --size 1
# → {"error": "[untrusted remote data] ..."}
# exits 13 if asset is unknown
```

JSON errors go to **stdout**; pretty/table errors go to stderr.

## Dry-run anything mutating

```bash
hyperliquid --format json --dry-run orders create \
  --coin BTC --side buy --price 50000 --size 0.001 --tif alo
```

The envelope shows `would_execute`, the resolved asset, the resolved signer, and `acting_as` / `vault_address` if any. Diffing the envelope against your intent catches most bugs before they hit the protocol. See [features/dry-run](../features/dry-run.md).

## Inspect the schema

```bash
hyperliquid --format json schema orders create
```

Schema metadata is authoritative when README disagrees.

## Common errors and what to do

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| Exit 10 `Authentication required` | No wallet configured | `hyperliquid setup` or `hyperliquid wallet create` |
| Exit 10 `OWS wallet '...' was not found` | Wrong selector | `wallet list` to see names; check `HYPERLIQUID_OWS_VAULT_PATH` |
| Exit 13 `"XYZ" not found. Did you mean: ...?` | Asset typo | Use a suggestion or run `asset search XYZ` |
| Exit 13 on HIP-3 trade | Missing margin in `dex:<DEX>` | Transfer USDC via `transfer send-asset` first |
| Exit 11 rate limited | Burst of `/info` calls | Add backoff between calls |
| Exit 12 timeout | Network or API issue | Retry; check `hyperliquid status` |
| "Self-transfer is not allowed" | `transfer send` with own address | Use `subaccount transfer` or a real recipient |
| Mainnet `schedule cancel-all` prompts | Safety gate | Pass `-y` only if intentional |

## Untrusted remote data

Every API/protocol string surfaces with the prefix `[untrusted remote data]`. Do not strip the label in artifacts. If you see suspicious characters in pretty output, they've already been ANSI-stripped via `src/response_sanitization.rs`.

## Tracing

The CLI does not ship a verbose log mode. For ad-hoc diagnostics, instrument the path with `eprintln!` locally; production builds intentionally avoid logging signer state, balances, or order intents.

## See also

- [features/dry-run](../features/dry-run.md)
- [features/agent-output-contract](../features/agent-output-contract.md)
- [background/pitfalls](../background/pitfalls.md)
