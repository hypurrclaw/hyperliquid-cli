# Systems

The systems section covers the cross-cutting building blocks that domain commands ride on. Each subsystem has clear boundaries and shows up in nearly every command path.

| System | Page | Files |
|--------|------|-------|
| Catalog-driven typed command contracts | [command-registry](command-registry.md) | `src/command_registry.rs`, `src/command_catalog.json`, `src/command_metadata.rs`, `src/commands/schema.rs` |
| Signer and wallet resolution | [signing-and-wallets](signing-and-wallets.md) | `src/auth.rs`, `src/signing.rs`, `src/resolvers.rs`, `src/ows.rs`, `src/db.rs` |
| Error and output pipeline | [error-and-output](error-and-output.md) | `src/errors.rs`, `src/output/mod.rs`, `src/response_sanitization.rs` |
| Watch mode and WebSocket streaming | [watch-and-streaming](watch-and-streaming.md) | `src/watch.rs` |
| Update check and self-update | [update-and-release](update-and-release.md) | `src/update_check.rs`, `install.sh`, `.github/workflows/release.yml` |
| Input hardening and raw payload limits | [input-hardening](input-hardening.md) | `src/input_hardening.rs` |
