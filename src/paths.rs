//! Where the CLI keeps its files. Environment overrides live in [`crate::settings`].

use std::path::PathBuf;

const APP_DIR: &str = "hyperliquid";

/// Config directory:
/// - macOS: `~/Library/Application Support/hyperliquid`
/// - Linux: `~/.config/hyperliquid`
/// - Windows: `C:\Users\<user>\AppData\Roaming\hyperliquid`
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join(APP_DIR))
}

#[must_use]
pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("config.json"))
}

/// Cached release info for the update notice. Lives beside the config file.
#[must_use]
pub fn version_cache_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("version.json"))
}

fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join(APP_DIR))
}

/// Encrypted accounts written by releases before the OWS vault.
#[must_use]
pub fn legacy_accounts_db() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("accounts.db"))
}

/// Plain key file that releases before the OS keychain stored next to the database.
#[must_use]
pub fn legacy_accounts_key() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("accounts.key"))
}

/// Default OWS vault directory when `HYPERLIQUID_OWS_VAULT_PATH` is unset.
#[must_use]
pub fn default_vault_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".hyperliquid"))
}
