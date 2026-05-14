# Configuration

## Config file

Location: platform config directory + `/hyperliquid/config.json`

| Platform | Path |
|----------|------|
| macOS | `~/Library/Application Support/hyperliquid/config.json` |
| Linux | `~/.config/hyperliquid/config.json` |
| Windows | `C:\Users\<user>\AppData\Roaming\hyperliquid\config.json` |

```json
{
  "private_key": null,
  "network": "mainnet",
  "default_wallet_id": null
}
```

Fields are optional — missing config is not an error for read-only commands.

## Environment variables

### Network and API

| Variable | Default | Description |
|----------|---------|-------------|
| `HYPERLIQUID_NETWORK` | `mainnet` | Network selection (`mainnet` or `testnet`) |
| `HYPERLIQUID_API_BASE_URL` | (network default) | Custom API base URL (overrides network) |
| `HYPERLIQUID_MAINNET_API_BASE_URL` | `https://api.hyperliquid.xyz` | Override mainnet API URL |
| `HYPERLIQUID_TESTNET_API_BASE_URL` | `https://api.hyperliquid-testnet.xyz` | Override testnet API URL |
| `HYPERLIQUID_NO_UPDATE_CHECK` | (unset) | Disable best-effort release update checks for normal command execution |

### Output format

| Variable | Default | Description |
|----------|---------|-------------|
| `HYPERLIQUID_FORMAT` | `pretty` | Output format: `pretty`, `table`, or `json` |
| `HYPERLIQUID_AGENT` | (unset) | Set to `1` to default to JSON format |

### Signing

| Variable | Description |
|----------|-------------|
| `HYPERLIQUID_PRIVATE_KEY` | Raw private key for signing (0x-prefixed hex) |

### Account storage

| Variable | Description |
|----------|-------------|
| `HYPERLIQUID_ACCOUNT_KEY_PASSPHRASE` | Passphrase for account encryption key derivation |
| `HYPERLIQUID_ACCOUNT_KEYCHAIN_DISABLED` | Set to `1` to disable OS keychain and require passphrase |

### OWS wallet

| Variable | Default | Description |
|----------|---------|-------------|
| `HYPERLIQUID_OWS_VAULT_PATH` | `~/.hyperliquid` | OWS vault directory |
| `OWS_PASSPHRASE` | (none) | Passphrase to unlock encrypted OWS wallet |

### Watch mode

| Variable | Description |
|----------|-------------|
| `HYPERLIQUID_WATCH_MAX_TICKS` | Default max ticks for watch mode in agent contexts |

### Builder and referral defaults

| Variable | Description |
|----------|-------------|
| `HYPERLIQUID_DEFAULT_BUILDER_ADDRESS` | Default builder address used when builder-aware commands need one |
| `HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE` | Default builder fee rate paired with the default builder address |
| `HYPERLIQUID_DEFAULT_REFERRAL_CODE` | Default referral code for `referral set` when no code is passed |

## Resolution priority

For any setting, the resolution order is:

1. CLI flag (e.g., `--private-key`, `--testnet`, `--format json`)
2. Environment variable (e.g., `HYPERLIQUID_PRIVATE_KEY`, `HYPERLIQUID_FORMAT`)
3. Config file (`config.json`)
4. Hardcoded default

## Account database

Location: platform data directory + `/hyperliquid/accounts.db`

| Platform | Path |
|----------|------|
| macOS | `~/Library/Application Support/hyperliquid/accounts.db` |
| Linux | `~/.local/share/hyperliquid/accounts.db` |

Encrypted with AES-256-GCM. Encryption key stored in OS keychain or derived from `HYPERLIQUID_ACCOUNT_KEY_PASSPHRASE`.

## OWS vault

Default location: `~/.hyperliquid/`

Contains OWS-managed wallets. Override with `HYPERLIQUID_OWS_VAULT_PATH`.
