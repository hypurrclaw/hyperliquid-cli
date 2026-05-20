# Exit codes

From `src/errors.rs`. The exit code is the agent's primary structured signal — pair it with the JSON `{"error": "..."}` envelope for the human-readable message.

| Code | Variant | Meaning | Example trigger |
|------|---------|---------|-----------------|
| 0 | (success) | Command completed | `hyperliquid mids` |
| 1 | `Internal(anyhow::Error)` | Unexpected internal error | Unhandled invariant; capture and report |
| 2 | `Configuration(String)` | Invalid config/env | `HYPERLIQUID_NETWORK=foo` |
| 2 | (clap usage) | Invalid arguments | Missing required flag |
| 10 | `AuthRequired` | No wallet/private key configured | `hyperliquid orders create ...` with no signer |
| 10 | `InvalidAuth(String)` | Bad key format / expired credentials | Malformed `--private-key` |
| 10 | `OwsWalletNotFound { wallet }` | OWS selector unknown | `--ows-signer alice` with no such vault entry |
| 10 | `OwsNoChainAccount { wallet, caip2 }` | OWS wallet has no `eip155:999` or `eip155:1` account | Wallet imported without Hyperliquid chain |
| 11 | `RateLimited` | Hyperliquid API rate-limit response | Burst of `/info` calls |
| 12 | `Unavailable(String)` | Network/API unreachable | DNS failure, 5xx |
| 12 | `Timeout(String)` | Exceeded a requested timeout | Subscribe idle-timeout fires |
| 13 | `Unsupported(String)` | Invalid input or unsupported parameter | HIP-3 trade without `dex:` margin |
| 13 | `AssetNotFound { asset, suggestions }` | Unknown asset with close matches | `--coin BTCC` → "Did you mean: BTC?" |
| 13 | `AssetNotFoundNoSuggestion { asset }` | Unknown asset with no matches | Typo with no close match |
| 14 | `StaleData(String)` | Cached data expired | Metadata cache TTL exceeded under load |
| 15 | `PartialResults(String)` | Some items failed in a batch | Mixed batch order outcome |

## Routing

- JSON mode: errors print to **stdout** as `{"error": "..."}`.
- Pretty mode: errors print to **stderr** with a red prefix.
- Table mode: errors print to **stderr** without ANSI.

Process exits with the structured code regardless of format.

## See also

- [systems/error-and-output](../systems/error-and-output.md)
- [features/agent-output-contract](../features/agent-output-contract.md)
