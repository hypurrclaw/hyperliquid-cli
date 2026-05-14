## Summary

-

## Verification

- [ ] `cargo fmt --check`
- [ ] `cargo clippy -- -D warnings`
- [ ] `cargo test`
- [ ] Relevant contract/QA checks listed below

Commands run:

```text

```

## Safety Checklist

- [ ] No private keys, keystores, passphrases, account DBs, `.qa/`, `.env`, or local chat artifacts are included.
- [ ] Mutating-command behavior was tested with `--dry-run`, mocks, or explicit funded-testnet opt-in.
- [ ] JSON output/schema contracts were updated when command behavior changed.
- [ ] Documentation and generated agent artifacts were updated when user-facing behavior changed.
