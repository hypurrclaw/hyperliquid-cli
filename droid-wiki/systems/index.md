# Systems

Internal building blocks that underpin the CLI. These don't map to a single application or feature — they are shared infrastructure used by multiple command domains.

| System | Description |
|--------|-------------|
| [Command registry](command-registry.md) | Typed command contracts, tool catalog loading, handler binding metadata |
| [Output rendering](output-rendering.md) | Pretty/table/JSON formatting, color theme, field projection |
| [Configuration and storage](configuration-and-storage.md) | Config resolution, encrypted account database, OWS vault integration |
| [Signing and auth](signing-and-auth.md) | Signer resolution chain, action signing via EIP-712, OWS integration |
