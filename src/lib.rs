//! Hyperliquid CLI library.
//!
//! Exposes internal modules for integration testing.

pub mod assets;
pub mod auth;
pub mod command_handlers;
pub(crate) mod command_metadata;
pub mod command_registry;
pub mod commands;
pub mod config;
pub mod dry_run;
pub mod errors;
pub(crate) mod http_api;
pub mod input_hardening;
pub mod invocation;
pub mod legacy_accounts;
pub mod mode;
pub mod network;
pub mod output;
pub mod ows;
pub mod paths;
pub mod private_fs;
pub mod prompt;
pub mod resolvers;
pub mod response_sanitization;
pub mod settings;
pub mod signing;
pub mod stream;
pub mod update;
pub mod wallet_store;
pub mod watch;
pub mod ws;
