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

## Abuse controls

The Worker is intentionally public so released CLIs can submit feedback without an operator-specific secret. To reduce spam and quota abuse it:

- accepts only `POST /feedback` with `Content-Type: application/json`;
- caps request bodies at 20 KiB and scenario JSON at 16 KiB;
- rate-limits each client IP hash to 1 accepted submission per day;
- rate-limits each agent/signer/wallet address found in the scenario JSON to 1 accepted submission per day;
- rate-limits total accepted submissions to 1 per minute across the Worker;
- stores only a SHA-256 hash of the client IP for rate limiting and triage.

For high-volume releases, also configure Cloudflare dashboard WAF/rate-limiting rules for `/feedback` as a perimeter control.

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
