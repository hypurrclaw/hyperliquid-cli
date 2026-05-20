# Fun facts

A handful of details worth knowing about this codebase that are not load-bearing for using or contributing to it.

## Two Alloy versions in one binary

`hypersdk` 0.2 still re-exports Alloy 1's `PrivateKeySigner`, while the app-level code uses Alloy 2. Both versions ship in the binary. `src/lib.rs` ends with an anonymous `extern crate alloy_signer_local_v1 as _;` to keep Cargo's v1 `keystore` feature anchored:

```rust
// Intentional dependency anchor: hypersdk 0.2 re-exports Alloy 1.x's
// `PrivateKeySigner`, whose keystore helpers are feature-gated in
// `alloy-signer-local` 1.x. ...
extern crate alloy_signer_local_v1 as _;
```

It's one of the few places in modern Rust where you'll see an anonymous `extern crate` doing real work.

## Zero TODO / FIXME / HACK comments

Across all 52 Rust source files, there is not a single `TODO`, `FIXME`, or `HACK` comment. For a ~34,000-line codebase that signs financial actions and was public-released in a 48-hour window, that's unusually clean.

## The catalog is 10 % of the code

`src/command_catalog.json` is 3,491 lines — roughly 10 % of the total Rust + JSON source. Almost a tenth of the project is structured command metadata.

## Hyperliquid is `eip155:999`

The Hyperliquid chain identifies as CAIP-2 `eip155:999`. The constant lives at the top of `src/ows.rs`:

```rust
pub const HYPERLIQUID_CAIP2: &str = "eip155:999";
```

`999` is the protocol's chosen chain id; nine-hundred-and-ninety-nine specifically, not a placeholder.

## SHA-256 the whole way down

Both `install.sh` and `hyperliquid update` verify SHA-256 before swapping the binary. The release workflow publishes `.sha256` companion files for every artifact. Even the in-binary self-update refuses to swap without a matching checksum.

## The biggest file is the dispatcher

`src/cli_runtime.rs` (~3,400 lines) is the largest single source file. It is mostly per-command match arms — long, but flat. The second-largest is `src/commands/orders.rs` (2,524 lines), which is genuinely complex; the order family has more shapes than any other.

## See also

- [by-the-numbers](by-the-numbers.md)
- [lore](lore.md)
