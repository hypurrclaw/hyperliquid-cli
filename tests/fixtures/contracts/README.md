# Contract Fixtures

These fixtures are characterization snapshots for issues #42/#43. They freeze
the current CLI/schema/registry/dry-run/output contracts so drift fails in
tests before the command registry migration changes architecture.

To update them intentionally:

```bash
HYPERLIQUID_UPDATE_CONTRACTS=1 cargo test --test command_contracts --test schema_contracts --test registry_contracts --test dry_run_contracts --test output_contracts
```

Review the resulting diff. Do not update snapshots as a side effect of unrelated
feature work.
