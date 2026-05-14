# Hyperliquid feedback Worker

Cloudflare Worker endpoint for `hyperliquid feedback`. It accepts structured scenario JSON and stores each submission in D1.

## Request

`POST /feedback`

```json
{
  "source": "hyperliquid-cli",
  "version": "0.1.0",
  "scenario": {
    "command": "orders create",
    "context": { "network": "testnet" },
    "expected": "dry-run preview",
    "actual": "unexpected validation error",
    "steps": ["ran hyperliquid --dry-run orders create ..."]
  },
  "contact": "optional@example.com",
  "tags": ["bug", "agent"]
}
```

Response:

```json
{ "status": "accepted", "id": "uuid" }
```

## Setup

```bash
cd workers/feedback
npm install
npx wrangler d1 create hyperliquid_feedback
# Paste the returned database_id into wrangler.toml
npm run db:migrate
npm run deploy
```

Build the CLI with the deployed endpoint embedded:

```bash
HYPERLIQUID_FEEDBACK_URL="https://<worker-subdomain>/feedback" cargo build --release
hyperliquid feedback --scenario-json '{"command":"mids","actual":"worked","expected":"worked"}'
```

For local testing, pass `--url http://127.0.0.1:8787/feedback`.
