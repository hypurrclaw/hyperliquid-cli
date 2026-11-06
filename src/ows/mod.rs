//! Open Wallet Standard integration.
//!
//! OWS is the only managed wallet backend. [`vault`] owns the wallet lifecycle
//! (create, import, list, rename, export, delete, default selection) and
//! [`signing`] owns signing with a vault wallet.

pub mod signing;
pub mod vault;

use crate::errors::CliError;

/// The Hyperliquid chain CAIP-2 identifier used for account derivation and lookup.
pub const HYPERLIQUID_CAIP2: &str = "eip155:999";

/// Hyperliquid uses Ethereum-compatible secp256k1 keys, so wallets without an
/// explicit Hyperliquid account fall back to their standard EVM account.
const EVM_CAIP2: &str = "eip155:1";

const HYPERLIQUID_CHAIN_ALIAS: &str = "hyperliquid";

fn map_ows_error(err: ows_lib::OwsLibError) -> CliError {
    use ows_lib::OwsLibError;

    match err {
        OwsLibError::WalletNotFound(wallet) => CliError::OwsWalletNotFound { wallet },
        OwsLibError::AmbiguousWallet { name, count } => CliError::InvalidAuth(format!(
            "ows_ambiguous_wallet: OWS wallet selector '{name}' matched {count} wallets; use the wallet id"
        )),
        OwsLibError::WalletNameExists(name) => CliError::Unsupported(format!(
            "ows_wallet_name_exists: a wallet named '{name}' already exists"
        )),
        OwsLibError::InvalidInput(message) => {
            CliError::Unsupported(format!("ows_unsupported_method: {message}"))
        }
        OwsLibError::Io(err) => CliError::Unavailable(format!("ows_unavailable: {err}")),
        OwsLibError::Crypto(err) => CliError::InvalidAuth(format!(
            "ows_wallet_locked: failed to decrypt OWS wallet: {err}"
        )),
        OwsLibError::Signer(err) => CliError::InvalidAuth(format!("ows_signing_failed: {err}")),
        OwsLibError::Core(err) => CliError::Unsupported(format!("ows_chain_mismatch: {err}")),
        other => CliError::InvalidAuth(format!("ows_malformed_response: {other}")),
    }
}
