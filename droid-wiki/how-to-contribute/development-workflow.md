# Development workflow

## Branch, code, test, PR, merge

1. Create a feature branch from `main`
2. Make focused changes with clear commit messages
3. Run the quality gates locally before pushing:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo test --test command_contracts --test schema_contracts --test registry_contracts --test dry_run_contracts --test output_contracts
```

4. Open a PR against `main`
5. CI runs: build, all tests, contract tests, registry rollout gates, clippy, OWS tests, formatting
6. Address review feedback
7. Merge when approved and CI green

## Command surface changes

When adding or changing a command:

1. Add the command to `src/command_catalog.json` with proper metadata (`lifecycle`, `risk`, `dry_run`, `confirmation`, `ows_signer`, `raw_payload`)
2. Implement the handler in `src/commands/` following existing patterns
3. Add the dispatch arm in `src/cli_runtime.rs`
4. Add the clap subcommand variant in `src/main.rs`
5. Add integration tests in `tests/`
6. Update characterization contracts: `HYPERLIQUID_UPDATE_CONTRACTS=1 task contracts`

## Registry rollout policy

Command-family routing changes follow the rollout pipeline documented in [`docs/registry-rollout-policy.md`](../../docs/registry-rollout-policy.md). Signed and fund-moving commands must pass canary stages before mainnet routing.
