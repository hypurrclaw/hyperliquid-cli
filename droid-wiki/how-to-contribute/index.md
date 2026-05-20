# How to contribute

Start with `CONTRIBUTING.md` and `AGENTS.md` at the repository root. This section expands on both for working inside the codebase day-to-day.

| Page | Purpose |
|------|---------|
| [development-workflow](development-workflow.md) | Branch, build, test, PR cycle |
| [testing](testing.md) | Test layers, helpers, contract characterization |
| [debugging](debugging.md) | Error categories, dry-run for live previews, logs |
| [patterns-and-conventions](patterns-and-conventions.md) | Coding conventions and selector semantics |
| [tooling](tooling.md) | Taskfile, QA scripts, CI workflows |

Quick checklist before opening a PR:

1. `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`
2. Refresh contract fixtures if command metadata changed: `HYPERLIQUID_UPDATE_CONTRACTS=1 task contracts`
3. For new commands, add catalog metadata and at least one integration test
4. Note any live-mutating behavior and the exact dry-run or mocked evidence used
