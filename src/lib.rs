//! Hyperliquid CLI library.
//!
//! Exposes internal modules for integration testing.

// Intentional dependency anchor: hypersdk 0.2 re-exports Alloy 1.x's
// `PrivateKeySigner`, whose keystore helpers are feature-gated in
// `alloy-signer-local` 1.x. The crate otherwise imports the signer through
// hypersdk, so this anonymous extern keeps Cargo's v1 `keystore` feature
// unified while app-level EIP-712 helpers move to Alloy 2.
extern crate alloy_signer_local_v1 as _;

pub mod auth;
pub mod command_context;
pub mod command_handlers;
pub(crate) mod command_metadata;
pub mod command_registry;
pub mod commands;
pub mod config;
pub mod db;
pub mod dry_run;
pub mod errors;
pub(crate) mod http_api;
pub mod input_hardening;
pub mod output;
pub mod ows;
pub mod resolvers;
pub mod response_sanitization;
pub mod signing;
pub mod update_check;
pub mod watch;
