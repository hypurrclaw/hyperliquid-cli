# Feedback

`hyperliquid feedback` lets operators submit structured CLI feedback as a JSON scenario document. The submission is rate-limited on the worker side and emits JSON-friendly attribution so agents can parse rate-limit responses.

## Key file

`src/commands/feedback.rs` (~434 lines plus a corresponding worker under `workers/`).

## Usage

```bash
hyperliquid feedback --scenario ./my-scenario.json
hyperliquid --format json feedback --scenario ./my-scenario.json
```

The scenario file is validated through [input hardening](../systems/input-hardening.md) (1 MiB, depth 64, etc.) before submission.

## Rate limits

The CLI-side rate limiter and the worker-side limiter share an attribution field so JSON output reveals exactly which gate fired (commit `53f7bbe`: "test(feedback): assert rate limits in json output"). The release endpoint is configured separately from local-only test fixtures (commit `fe1d0e8`).

## What it submits

A scenario JSON object that typically includes:

- A title and description
- The command invocation that triggered the report
- A sanitized error message (already passing through `[untrusted remote data]` if it came from the protocol)
- Optional environment context

The submission never includes secrets. Agents should redact any user-provided context before passing it through.

## Entry points for modification

- To change the worker endpoint, update the constants in `src/commands/feedback.rs` and the corresponding worker config.
- To tighten rate limits, edit the limiter near the top of the same file. Verify with `tests/feedback.rs`.

See also: [agent-output-contract](agent-output-contract.md).
