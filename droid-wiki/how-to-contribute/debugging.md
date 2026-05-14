# Debugging

## Common errors

### Authentication required (exit 10)

```
Error: Authentication required. Run `hyperliquid setup` to configure your wallet.
```

No signer is configured. Run `hyperliquid setup` to create or import a wallet, or pass `--private-key`, `--keystore`, `--account`, or `--ows-signer`.

### Rate limited (exit 11)

```
Error: Rate limited by Hyperliquid API. Please wait and retry.
```

The API returned a rate-limit response (HTTP 429 or structured error). Wait and retry.

### API unreachable (exit 12)

```
Error: Unable to reach Hyperliquid API. Check your network connection.
```

The HTTP client could not connect. Check network connectivity and the API base URL. For testnet, ensure `--testnet` is passed.

### Asset not found (exit 13)

```
"BT" not found. Did you mean: BTC, BLUR, BONK?
```

The asset name didn't match any known market. The CLI uses Levenshtein distance for fuzzy suggestions.

## Logs and diagnostics

The CLI does not have a debug log mode. For troubleshooting:

- Use `--format json` to get structured error output including the full error message
- Check `~/.config/hyperliquid/config.json` for saved configuration
- Check `~/.hyperliquid/` for the OWS vault state
- Add `-v` (verbose) is not yet implemented; use `--format json` for diagnostic output

## Common pitfalls

### Signer conflicts

`--account`, `--private-key`, `--keystore`, and `--ows-signer` are mutually exclusive. Pick one signer source per command.

### Dry-run restrictions

`--dry-run` only works with mutating commands. Read-only commands reject it with exit 13 ("unsupported input").

### Payload input requires dry-run

`--payload-json` and `--payload-file` currently require `--dry-run` so raw payloads can be validated without side effects.

### OWS wallet not found

If `--ows-signer` references a wallet that doesn't exist in the vault, the CLI exits with code 10 and `OwsWalletNotFound`. Run `hyperliquid wallet list` to see available wallets.

### OWS wallet has no Hyperliquid account

Some OWS wallets may have other chain accounts but no Hyperliquid account. The CLI checks for both `eip155:999` (Hyperliquid chain) and `eip155:1` (Ethereum mainnet) as fallback.

## Running individual test cases

```bash
cargo test test_exit_code_auth_required          # specific test
cargo test --test cli_integration -- --nocapture # integration test with output
```
