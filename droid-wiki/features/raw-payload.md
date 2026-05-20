# Raw payload submission

For commands that opt in, agents can provide a raw JSON action payload via `--payload-json` or `--payload-file`. This bypasses the CLI's typed argument layer but still goes through validation, signing, and the live-submission policy.

## Global flags

```text
--payload-json <JSON>     # inline JSON object
--payload-file <PATH>     # file path; "-" reads stdin
```

These are mutually exclusive in `src/main.rs`.

## Policy

Each command's `raw_payload` field in the schema decides what the CLI accepts. Default is **fail-closed**: a command must be explicitly allowlisted before raw-payload submission is honored for live execution. Until allowlisted, raw payloads are accepted only for dry-run validation.

This is intentional. Raw payloads are unsanitized, and a typo can swap `cancel` for `cancelAll`, address fields, or signs. The fail-closed default makes agent shipping safer.

## File limits

`src/input_hardening.rs::FilePolicy::payload()`:

| Limit | Value |
|-------|-------|
| Max bytes | 1 MiB |
| Max JSON depth | 64 |
| Max keys per object | 4096 |
| Max string length | 64 KiB |

Stdin (`-`) is accepted only when `allow_stdin` is true on the policy.

## Dry-run with payload

When `--dry-run` and `--payload-json` are combined, the dry-run envelope includes the parsed payload under `payload`:

```bash
hyperliquid --format json --dry-run orders create \
  --payload-file ./create-order.json
```

The envelope's `would_execute`, `signer`, and `args` are still produced from the typed plan; the payload is captured for diff-ability.

## Where it makes sense

- Replaying a captured action JSON in an automated test.
- Composing a non-trivial batch (e.g., `orders batch-create`) where building the same JSON the typed args would produce is impractical.

For everything else, prefer the typed flags.

## Entry points for modification

- To allowlist a new command for live raw-payload submission, change its `raw_payload` policy in `src/command_catalog.json` and add explicit characterization tests under `tests/security_contracts.rs`.
- To extend the size or depth limits for a specific command, add a new `FilePolicy` constructor in `src/input_hardening.rs` rather than weakening the global defaults.

See also: [systems/input-hardening](../systems/input-hardening.md), [dry-run](dry-run.md).
