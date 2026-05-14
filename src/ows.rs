//! Open Wallet Standard signer selection and wallet lifecycle integration.
//!
//! OWS is the primary wallet backend for hyperliquid-cli. Wallet creation,
//! import, listing, and signing all flow through the OWS vault at
//! ~/.hyperliquid (or HYPERLIQUID_OWS_VAULT_PATH when set).

use std::env;
use std::path::PathBuf;

use alloy::dyn_abi::TypedData;
use alloy_primitives::Address as AlloyAddress;
use alloy_primitives::B256;
use hypersdk::Address;
use hypersdk::hypercore::types::Signature as HyperliquidSignature;

use crate::errors::CliError;

/// The Hyperliquid chain CAIP-2 identifier used for account derivation and lookup.
pub const HYPERLIQUID_CAIP2: &str = "eip155:999";

const HYPERLIQUID_CHAIN_ALIAS: &str = "hyperliquid";

/// Environment variable for the OWS vault passphrase.
pub const OWS_PASSPHRASE_ENV: &str = "OWS_PASSPHRASE";

/// Environment variable for a custom OWS vault path.
pub const OWS_VAULT_PATH_ENV: &str = "HYPERLIQUID_OWS_VAULT_PATH";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwsSignerConfig {
    selector: String,
    address: Address,
    wallet: Option<OwsWalletSelection>,
    vault_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwsWalletSelection {
    wallet_id: String,
    wallet_name: String,
    chain_id: String,
}

impl OwsSignerConfig {
    #[must_use]
    pub fn selector(&self) -> &str {
        &self.selector
    }

    #[must_use]
    pub fn address(&self) -> Address {
        self.address
    }

    #[must_use]
    pub fn wallet(&self) -> Option<&OwsWalletSelection> {
        self.wallet.as_ref()
    }

    #[must_use]
    pub fn vault_path(&self) -> Option<&PathBuf> {
        self.vault_path.as_ref()
    }
}

impl OwsWalletSelection {
    #[must_use]
    pub fn wallet_id(&self) -> &str {
        &self.wallet_id
    }

    #[must_use]
    pub fn wallet_name(&self) -> &str {
        &self.wallet_name
    }

    #[must_use]
    pub fn chain_id(&self) -> &str {
        &self.chain_id
    }
}

/// Resolve an OWS signer selector to a signer config.
///
/// Accepts an explicit 0x address (which won't be able to live-sign) or a
/// wallet name/id that will be looked up in the OWS vault.
pub fn resolve_selector(selector: &str) -> Result<OwsSignerConfig, CliError> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err(CliError::Configuration(
            "--ows-signer requires a non-empty selector".to_string(),
        ));
    }

    if let Ok(address) = selector.parse::<Address>() {
        return Ok(OwsSignerConfig {
            selector: selector.to_string(),
            address,
            wallet: None,
            vault_path: None,
        });
    }

    resolve_wallet_selector(selector)
}

fn resolve_wallet_selector(selector: &str) -> Result<OwsSignerConfig, CliError> {
    resolve_wallet_selector_with_vault(selector, ows_vault_path())
}

fn resolve_wallet_selector_with_vault(
    selector: &str,
    vault_path: Option<PathBuf>,
) -> Result<OwsSignerConfig, CliError> {
    let wallet = ows_lib::get_wallet(selector, vault_path.as_deref()).map_err(map_ows_error)?;
    let account = select_hyperliquid_account(&wallet.accounts).ok_or_else(|| {
        CliError::OwsNoChainAccount {
            wallet: wallet.name.clone(),
            caip2: HYPERLIQUID_CAIP2.to_string(),
        }
    })?;
    let address = account.address.parse::<Address>().map_err(|_| {
        CliError::InvalidAuth(format!(
            "ows_malformed_response: OWS wallet '{}' returned invalid EVM address '{}'",
            wallet.name, account.address
        ))
    })?;

    Ok(OwsSignerConfig {
        selector: selector.to_string(),
        address,
        wallet: Some(OwsWalletSelection {
            wallet_id: wallet.id,
            wallet_name: wallet.name,
            chain_id: account.chain_id.clone(),
        }),
        vault_path,
    })
}

pub fn select_hyperliquid_account(
    accounts: &[ows_lib::types::AccountInfo],
) -> Option<&ows_lib::types::AccountInfo> {
    accounts
        .iter()
        .find(|account| account.chain_id == HYPERLIQUID_CAIP2)
        .or_else(|| accounts.iter().find(|a| a.chain_id == "eip155:1"))
}

/// Returns the error for when a raw 0x address is used as an OWS selector
/// without a resolved wallet — live signing is not possible without a wallet.
pub fn unsupported_live_signing(selector: &str) -> CliError {
    CliError::Unsupported(format!(
        "OWS signer '{selector}' does not have a resolved wallet for live signing; use a stored wallet name/id, or select a local signing account, keystore, or private key for live submission"
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwsSigningConfig {
    selector: String,
    wallet_id: Option<String>,
    address: Address,
    vault_path: Option<PathBuf>,
}

impl OwsSigningConfig {
    #[must_use]
    pub fn new(selector: String, wallet_id: Option<String>, address: Address) -> Self {
        Self::with_vault_path(selector, wallet_id, address, None)
    }

    #[must_use]
    pub fn with_vault_path(
        selector: String,
        wallet_id: Option<String>,
        address: Address,
        vault_path: Option<PathBuf>,
    ) -> Self {
        Self {
            selector,
            wallet_id,
            address,
            vault_path,
        }
    }

    #[must_use]
    pub fn selector(&self) -> &str {
        &self.selector
    }

    #[must_use]
    pub fn address(&self) -> Address {
        self.address
    }

    #[must_use]
    pub fn has_resolved_wallet(&self) -> bool {
        self.wallet_id.is_some()
    }

    fn wallet_id(&self) -> Result<&str, CliError> {
        self.wallet_id
            .as_deref()
            .ok_or_else(|| unsupported_live_signing(&self.selector))
    }

    fn vault_path(&self) -> Option<&std::path::Path> {
        self.vault_path.as_deref()
    }
}

pub fn sign_typed_data(
    config: &OwsSigningConfig,
    typed_data: &TypedData,
) -> Result<HyperliquidSignature, CliError> {
    let typed_data_json = ows_typed_data_json(typed_data)?;
    let result = ows_lib::sign_typed_data(
        config.wallet_id()?,
        HYPERLIQUID_CHAIN_ALIAS,
        &typed_data_json,
        ows_passphrase().as_deref(),
        None,
        config.vault_path(),
    )
    .map_err(map_ows_error)?;
    let signature = hyperliquid_signature_from_ows(result)?;
    let alloy_signature: alloy_primitives::Signature = signature.into();
    let signing_hash = typed_data.eip712_signing_hash().map_err(|err| {
        CliError::Internal(anyhow::anyhow!("failed to hash OWS typed data: {err}"))
    })?;
    verify_recovered_address(config, &alloy_signature, signing_hash)?;
    Ok(signature)
}

pub fn sign_hash(config: &OwsSigningConfig, hash: B256) -> Result<HyperliquidSignature, CliError> {
    let result = ows_lib::sign_hash(
        config.wallet_id()?,
        HYPERLIQUID_CHAIN_ALIAS,
        &format!("0x{}", hex::encode(hash)),
        ows_passphrase().as_deref(),
        None,
        config.vault_path(),
    )
    .map_err(map_ows_error)?;
    let signature = hyperliquid_signature_from_ows(result)?;
    let alloy_signature: alloy_primitives::Signature = signature.into();
    verify_recovered_address(config, &alloy_signature, hash)?;
    Ok(signature)
}

pub fn sign_message(
    config: &OwsSigningConfig,
    message: &[u8],
) -> Result<alloy_primitives::Signature, CliError> {
    let result = ows_lib::sign_message(
        config.wallet_id()?,
        HYPERLIQUID_CHAIN_ALIAS,
        &hex::encode(message),
        ows_passphrase().as_deref(),
        Some("hex"),
        None,
        config.vault_path(),
    )
    .map_err(map_ows_error)?;
    let signature = alloy_signature_from_ows(result)?;
    let recovered = signature.recover_address_from_msg(message).map_err(|err| {
        CliError::InvalidAuth(format!(
            "ows_malformed_response: failed to recover OWS message signer: {err}"
        ))
    })?;
    verify_address_matches(config, recovered)?;
    Ok(signature)
}

/// Resolve the OWS vault passphrase from environment.
pub fn ows_passphrase() -> Option<String> {
    env::var(OWS_PASSPHRASE_ENV)
        .ok()
        .filter(|value| !value.is_empty())
}

/// Resolve the OWS vault path from environment.
pub fn ows_vault_path() -> Option<PathBuf> {
    env::var(OWS_VAULT_PATH_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".hyperliquid")))
}

fn ows_typed_data_json(typed_data: &TypedData) -> Result<String, CliError> {
    let mut value = serde_json::to_value(typed_data).map_err(|err| {
        CliError::Internal(anyhow::anyhow!("failed to encode OWS typed data: {err}"))
    })?;
    ensure_eip712_domain_type(&mut value);
    pad_odd_hex_typed_values(&mut value);
    serde_json::to_string(&value).map_err(|err| {
        CliError::Internal(anyhow::anyhow!("failed to encode OWS typed data: {err}"))
    })
}

fn ensure_eip712_domain_type(value: &mut serde_json::Value) {
    let Some(domain) = value
        .get("domain")
        .and_then(serde_json::Value::as_object)
        .cloned()
    else {
        return;
    };
    let Some(types) = value
        .get_mut("types")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    if types.contains_key("EIP712Domain") {
        return;
    }

    let mut fields = Vec::new();
    for (name, type_name) in [
        ("name", "string"),
        ("version", "string"),
        ("chainId", "uint256"),
        ("verifyingContract", "address"),
        ("salt", "bytes32"),
    ] {
        if domain.contains_key(name) {
            fields.push(serde_json::json!({"name": name, "type": type_name}));
        }
    }
    types.insert("EIP712Domain".to_string(), serde_json::Value::Array(fields));
}

fn pad_odd_hex_typed_values(value: &mut serde_json::Value) {
    let Some(types) = value
        .get("types")
        .and_then(serde_json::Value::as_object)
        .cloned()
    else {
        return;
    };
    if let Some(domain) = value.get_mut("domain") {
        pad_struct_odd_hex_values("EIP712Domain", domain, &types);
    }
    let Some(primary_type) = value
        .get("primaryType")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    if let Some(message) = value.get_mut("message") {
        pad_struct_odd_hex_values(&primary_type, message, &types);
    }
}

fn pad_struct_odd_hex_values(
    type_name: &str,
    value: &mut serde_json::Value,
    types: &serde_json::Map<String, serde_json::Value>,
) {
    let Some(fields) = types
        .get(type_name)
        .and_then(serde_json::Value::as_array)
        .cloned()
    else {
        return;
    };
    let Some(object) = value.as_object_mut() else {
        return;
    };

    for field in fields {
        let Some(name) = field.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(field_type) = field.get("type").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(field_value) = object.get_mut(name) else {
            continue;
        };
        pad_field_odd_hex_values(field_type, field_value, types);
    }
}

fn pad_field_odd_hex_values(
    field_type: &str,
    value: &mut serde_json::Value,
    types: &serde_json::Map<String, serde_json::Value>,
) {
    if let Some(element_type) = field_type.strip_suffix("[]") {
        if let Some(values) = value.as_array_mut() {
            for value in values {
                pad_field_odd_hex_values(element_type, value, types);
            }
        }
        return;
    }

    if types.contains_key(field_type) {
        pad_struct_odd_hex_values(field_type, value, types);
        return;
    }

    if is_hex_encoded_eip712_scalar(field_type)
        && let Some(text) = value.as_str()
        && let Some(hex) = text.strip_prefix("0x")
        && hex.len() % 2 == 1
    {
        *value = serde_json::Value::String(format!("0x0{hex}"));
    }
}

fn is_hex_encoded_eip712_scalar(field_type: &str) -> bool {
    field_type == "bytes"
        || field_type.starts_with("bytes")
        || field_type.starts_with("uint")
        || field_type.starts_with("int")
}

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

fn hyperliquid_signature_from_ows(
    result: ows_lib::types::SignResult,
) -> Result<HyperliquidSignature, CliError> {
    let signature = canonical_signature_hex(result)?;
    signature.parse::<HyperliquidSignature>().map_err(|err| {
        CliError::InvalidAuth(format!(
            "ows_malformed_response: failed to parse OWS signature: {err}"
        ))
    })
}

fn alloy_signature_from_ows(
    result: ows_lib::types::SignResult,
) -> Result<alloy_primitives::Signature, CliError> {
    let signature = hyperliquid_signature_from_ows(result)?;
    Ok(signature.into())
}

fn canonical_signature_hex(result: ows_lib::types::SignResult) -> Result<String, CliError> {
    canonical_signature_hex_parts(&result.signature, result.recovery_id)
}

fn canonical_signature_hex_parts(
    signature: &str,
    recovery_id: Option<u8>,
) -> Result<String, CliError> {
    let sig_hex = signature.strip_prefix("0x").unwrap_or(signature);
    match sig_hex.len() {
        130 => {
            let v = u8::from_str_radix(&sig_hex[128..130], 16).map_err(|err| {
                CliError::InvalidAuth(format!(
                    "ows_malformed_response: invalid OWS recovery byte: {err}"
                ))
            })?;
            let canonical_v = if v >= 27 { v } else { v + 27 };
            Ok(format!("0x{}{canonical_v:02x}", &sig_hex[..128]))
        }
        128 => {
            let recovery_id = recovery_id.ok_or_else(|| {
                CliError::InvalidAuth(
                    "ows_malformed_response: OWS returned r+s signature without recovery id"
                        .to_string(),
                )
            })?;
            let v = if recovery_id >= 27 {
                recovery_id
            } else {
                recovery_id + 27
            };
            Ok(format!("0x{sig_hex}{v:02x}"))
        }
        len => Err(CliError::InvalidAuth(format!(
            "ows_malformed_response: unexpected OWS signature length {len} hex chars; expected 128 or 130"
        ))),
    }
}

fn verify_recovered_address(
    config: &OwsSigningConfig,
    signature: &alloy_primitives::Signature,
    signing_hash: B256,
) -> Result<(), CliError> {
    let recovered = signature
        .recover_address_from_prehash(&signing_hash)
        .map_err(|err| {
            CliError::InvalidAuth(format!(
                "ows_malformed_response: failed to recover OWS signer: {err}"
            ))
        })?;
    verify_address_matches(config, recovered)
}

fn verify_address_matches(
    config: &OwsSigningConfig,
    recovered: AlloyAddress,
) -> Result<(), CliError> {
    let expected: AlloyAddress = config.address;
    if recovered == expected {
        return Ok(());
    }

    Err(CliError::InvalidAuth(format!(
        "ows_signer_mismatch: OWS signature recovered {recovered}, expected {}",
        config.address
    )))
}

/// Create a new OWS wallet with a BIP-39 mnemonic and return its info.
///
/// This is the primary wallet creation entry point. Creates a 12-word
/// mnemonic, derives accounts for all chain families including Hyperliquid,
/// encrypts with the vault passphrase, and saves to the OWS vault.
pub fn create_ows_wallet(
    name: &str,
    passphrase: Option<&str>,
    vault_path: Option<&std::path::Path>,
) -> Result<ows_lib::types::WalletInfo, CliError> {
    ows_lib::create_wallet(name, Some(12), passphrase, vault_path).map_err(map_ows_error)
}

/// Import a wallet from a hex-encoded private key.
pub fn import_ows_wallet_private_key(
    name: &str,
    private_key_hex: &str,
    passphrase: Option<&str>,
    vault_path: Option<&std::path::Path>,
) -> Result<ows_lib::types::WalletInfo, CliError> {
    ows_lib::import_wallet_private_key(
        name,
        private_key_hex,
        Some(HYPERLIQUID_CHAIN_ALIAS),
        passphrase,
        vault_path,
        None,
        None,
    )
    .map_err(map_ows_error)
}

/// Import a wallet from a BIP-39 mnemonic phrase.
pub fn import_ows_wallet_mnemonic(
    name: &str,
    mnemonic_phrase: &str,
    passphrase: Option<&str>,
    vault_path: Option<&std::path::Path>,
) -> Result<ows_lib::types::WalletInfo, CliError> {
    ows_lib::import_wallet_mnemonic(name, mnemonic_phrase, passphrase, None, vault_path)
        .map_err(map_ows_error)
}

/// List all wallets in the OWS vault.
pub fn list_ows_wallets(
    vault_path: Option<&std::path::Path>,
) -> Result<Vec<ows_lib::types::WalletInfo>, CliError> {
    ows_lib::list_wallets(vault_path).map_err(map_ows_error)
}

/// Get a single OWS wallet by name or id.
pub fn get_ows_wallet(
    name_or_id: &str,
    vault_path: Option<&std::path::Path>,
) -> Result<ows_lib::types::WalletInfo, CliError> {
    ows_lib::get_wallet(name_or_id, vault_path).map_err(map_ows_error)
}

/// Delete an OWS wallet by name or id.
pub fn delete_ows_wallet(
    name_or_id: &str,
    vault_path: Option<&std::path::Path>,
) -> Result<(), CliError> {
    ows_lib::delete_wallet(name_or_id, vault_path).map_err(map_ows_error)
}

/// Rename an OWS wallet.
pub fn rename_ows_wallet(
    name_or_id: &str,
    new_name: &str,
    vault_path: Option<&std::path::Path>,
) -> Result<(), CliError> {
    ows_lib::rename_wallet(name_or_id, new_name, vault_path).map_err(map_ows_error)
}

/// Export an OWS wallet's secret (mnemonic or private key).
pub fn export_ows_wallet(
    name_or_id: &str,
    passphrase: Option<&str>,
    vault_path: Option<&std::path::Path>,
) -> Result<String, CliError> {
    ows_lib::export_wallet(name_or_id, passphrase, vault_path).map_err(map_ows_error)
}

/// Find the Hyperliquid address from a wallet's accounts.
///
/// Tries the explicit Hyperliquid CAIP-2 (`eip155:999`) first, then falls back to
/// the standard EVM CAIP-2 (`eip155:1`) because Hyperliquid uses Ethereum-compatible
/// secp256k1 keys. Returns an error if neither is present.
pub fn hyperliquid_address_from_wallet(
    wallet: &ows_lib::types::WalletInfo,
) -> Result<(String, Address), CliError> {
    if let Some(account) = wallet
        .accounts
        .iter()
        .find(|a| a.chain_id == HYPERLIQUID_CAIP2)
    {
        let address = account
            .address
            .parse::<Address>()
            .map_err(|_| CliError::InvalidAuth("invalid address in OWS wallet".into()))?;
        return Ok((account.address.clone(), address));
    }
    if let Some(account) = wallet.accounts.iter().find(|a| a.chain_id == "eip155:1") {
        let address = account
            .address
            .parse::<Address>()
            .map_err(|_| CliError::InvalidAuth("invalid address in OWS wallet".into()))?;
        return Ok((account.address.clone(), address));
    }
    Err(CliError::OwsNoChainAccount {
        wallet: wallet.name.clone(),
        caip2: HYPERLIQUID_CAIP2.to_string(),
    })
}

/// Resolve the default OWS wallet.
///
/// Uses the configured default_wallet_id if set, otherwise returns the first
/// Hyperliquid-capable wallet in the vault.
pub fn resolve_default_ows_wallet(
    default_wallet_id: Option<&str>,
    vault_path: Option<&std::path::Path>,
) -> Result<ows_lib::types::WalletInfo, CliError> {
    if let Some(id) = default_wallet_id
        && let Ok(wallet) = ows_lib::get_wallet(id, vault_path)
        && hyperliquid_address_from_wallet(&wallet).is_ok()
    {
        return Ok(wallet);
    }
    let wallets = ows_lib::list_wallets(vault_path).map_err(map_ows_error)?;
    wallets
        .into_iter()
        .find(|w| hyperliquid_address_from_wallet(w).is_ok())
        .ok_or_else(|| CliError::AuthRequired)
}

/// Result of resolving an OWS wallet selector.
///
/// Distinguishes between a real vault wallet and a raw address that was passed
/// directly (no vault entry to sign with).
pub enum ResolvedOwsSelector {
    Wallet {
        wallet: ows_lib::types::WalletInfo,
        address_str: String,
        address: Address,
    },
    RawAddress {
        address_str: String,
        address: Address,
    },
}

/// Resolve an OWS wallet selector (name, id, or address) to a resolved result.
pub fn resolve_ows_wallet_selector(
    selector: &str,
    vault_path: Option<&std::path::Path>,
) -> Result<ResolvedOwsSelector, CliError> {
    // If the selector is a valid address, first check if any OWS wallet owns it.
    if let Ok(address) = selector.parse::<Address>() {
        if let Ok(wallets) = ows_lib::list_wallets(vault_path) {
            for wallet in &wallets {
                if let Ok((addr_str, _)) = hyperliquid_address_from_wallet(wallet)
                    && addr_str.eq_ignore_ascii_case(selector)
                {
                    let (_addr_str, address) = hyperliquid_address_from_wallet(wallet)?;
                    return Ok(ResolvedOwsSelector::Wallet {
                        wallet: wallet.clone(),
                        address_str: _addr_str,
                        address,
                    });
                }
            }
        }
        // No matching wallet — return as raw address (preview-only, no signing).
        return Ok(ResolvedOwsSelector::RawAddress {
            address_str: selector.to_string(),
            address,
        });
    }
    let wallet = ows_lib::get_wallet(selector, vault_path).map_err(map_ows_error)?;
    let (address_str, address) = hyperliquid_address_from_wallet(&wallet)?;
    Ok(ResolvedOwsSelector::Wallet {
        wallet,
        address_str,
        address,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_explicit_address_selector() {
        let config = resolve_selector("0x0000000000000000000000000000000000000001").unwrap();
        assert_eq!(
            config.address().to_string(),
            "0x0000000000000000000000000000000000000001"
        );
        assert_eq!(
            config.selector(),
            "0x0000000000000000000000000000000000000001"
        );
        assert!(config.wallet().is_none());
    }

    #[test]
    fn canonical_signature_accepts_embedded_v() {
        let sig = format!("{}1b", "11".repeat(64));
        assert_eq!(
            canonical_signature_hex_parts(&sig, None).unwrap(),
            format!("0x{sig}")
        );
    }

    #[test]
    fn canonical_signature_canonicalizes_embedded_raw_parity() {
        let sig = format!("{}01", "11".repeat(64));
        assert_eq!(
            canonical_signature_hex_parts(&sig, None).unwrap(),
            format!("0x{}1c", "11".repeat(64))
        );
    }

    #[test]
    fn canonical_signature_appends_raw_recovery_id() {
        let rs = "11".repeat(64);
        assert_eq!(
            canonical_signature_hex_parts(&rs, Some(1)).unwrap(),
            format!("0x{rs}1c")
        );
    }

    #[test]
    fn canonical_signature_appends_canonical_recovery_id() {
        let rs = "11".repeat(64);
        assert_eq!(
            canonical_signature_hex_parts(&rs, Some(28)).unwrap(),
            format!("0x{rs}1c")
        );
    }

    #[test]
    fn canonical_signature_rejects_missing_recovery_id_for_rs() {
        let err = canonical_signature_hex_parts(&"11".repeat(64), None).unwrap_err();
        assert_eq!(err.exit_code(), 10);
        assert!(err.to_string().contains("without recovery id"));
    }

    #[test]
    fn canonical_signature_rejects_unexpected_length() {
        let err = canonical_signature_hex_parts("abcd", Some(0)).unwrap_err();
        assert_eq!(err.exit_code(), 10);
        assert!(err.to_string().contains("unexpected OWS signature length"));
    }

    fn import_test_wallet(vault_path: &std::path::Path) -> ows_lib::types::WalletInfo {
        const KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000009";
        ows_lib::import_wallet_private_key(
            "test-wallet",
            KEY,
            Some("hyperliquid"),
            None,
            Some(vault_path),
            None,
            None,
        )
        .unwrap()
    }

    fn test_signer_address() -> Address {
        "0x0000000000000000000000000000000000000000000000000000000000000009"
            .parse::<hypersdk::hypercore::PrivateKeySigner>()
            .unwrap()
            .address()
    }

    fn signing_config(
        wallet: &ows_lib::types::WalletInfo,
        vault_path: &std::path::Path,
    ) -> OwsSigningConfig {
        OwsSigningConfig::with_vault_path(
            wallet.name.clone(),
            Some(wallet.id.clone()),
            test_signer_address(),
            Some(vault_path.to_path_buf()),
        )
    }

    #[test]
    fn eip712_domain_type_is_added_for_ows_parser() {
        let mut value = serde_json::json!({
            "types": {
                "HyperliquidTransaction:UsdClassTransfer": [
                    {"name": "hyperliquidChain", "type": "string"},
                    {"name": "amount", "type": "string"}
                ]
            },
            "primaryType": "HyperliquidTransaction:UsdClassTransfer",
            "domain": {"name": "Exchange", "version": "1", "chainId": "0x66eee"},
            "message": {"hyperliquidChain": "Testnet", "amount": "5"}
        });

        ensure_eip712_domain_type(&mut value);

        assert_eq!(
            value["types"]["EIP712Domain"],
            serde_json::json!([
                {"name": "name", "type": "string"},
                {"name": "version", "type": "string"},
                {"name": "chainId", "type": "uint256"}
            ])
        );
    }

    #[test]
    fn odd_hex_padding_respects_declared_eip712_types() {
        let mut value = serde_json::json!({
            "types": {
                "EIP712Domain": [
                    {"name": "chainId", "type": "uint256"},
                    {"name": "salt", "type": "bytes32"}
                ],
                "TestAction": [
                    {"name": "label", "type": "string"},
                    {"name": "amount", "type": "uint256"},
                    {"name": "blob", "type": "bytes"}
                ]
            },
            "primaryType": "TestAction",
            "domain": {"chainId": "0x3e7", "salt": "0xabc"},
            "message": {"label": "0xabc", "amount": "0x5", "blob": "0xdef"}
        });

        pad_odd_hex_typed_values(&mut value);

        assert_eq!(value["domain"]["chainId"], "0x03e7");
        assert_eq!(value["domain"]["salt"], "0x0abc");
        assert_eq!(value["message"]["label"], "0xabc");
        assert_eq!(value["message"]["amount"], "0x05");
        assert_eq!(value["message"]["blob"], "0x0def");
    }

    #[test]
    fn imported_ows_wallet_without_explicit_hyperliquid_account_resolves_via_evm_fallback() {
        let tmp = tempfile::TempDir::new().unwrap();
        let wallet = import_test_wallet(tmp.path());

        assert!(
            wallet
                .accounts
                .iter()
                .any(|account| account.chain_id == "eip155:1")
        );
        let config =
            resolve_wallet_selector_with_vault("test-wallet", Some(tmp.path().to_path_buf()))
                .unwrap();
        // Resolved via eip155:1 fallback — address should match the EVM account.
        let evm_account = wallet
            .accounts
            .iter()
            .find(|a| a.chain_id == "eip155:1")
            .unwrap();
        assert_eq!(
            format!("{:?}", config.address),
            evm_account.address.to_lowercase()
        );
        assert_eq!(config.wallet.as_ref().unwrap().chain_id, "eip155:1");
    }

    #[test]
    fn ows_hash_signing_recovers_selected_wallet_address() {
        let tmp = tempfile::TempDir::new().unwrap();
        let wallet = import_test_wallet(tmp.path());
        let config = signing_config(&wallet, tmp.path());
        let hash = B256::repeat_byte(0x42);

        let signature = sign_hash(&config, hash).unwrap();
        let alloy_signature: alloy_primitives::Signature = signature.into();
        let recovered = alloy_signature.recover_address_from_prehash(&hash).unwrap();

        assert_eq!(recovered, config.address());
    }

    #[test]
    fn ows_message_signing_recovers_selected_wallet_address() {
        let tmp = tempfile::TempDir::new().unwrap();
        let wallet = import_test_wallet(tmp.path());
        let config = signing_config(&wallet, tmp.path());
        let message = b"hyperliquid-cli ows smoke";

        let signature = sign_message(&config, message).unwrap();
        let recovered = signature.recover_address_from_msg(message).unwrap();

        assert_eq!(recovered, config.address());
    }

    #[test]
    fn ows_typed_data_signing_recovers_selected_wallet_address() {
        let tmp = tempfile::TempDir::new().unwrap();
        let wallet = import_test_wallet(tmp.path());
        let config = signing_config(&wallet, tmp.path());
        let typed_data: TypedData = serde_json::from_value(serde_json::json!({
            "types": {
                "EIP712Domain": [
                    {"name": "name", "type": "string"},
                    {"name": "chainId", "type": "uint256"}
                ],
                "TestAction": [
                    {"name": "message", "type": "string"}
                ]
            },
            "primaryType": "TestAction",
            "domain": {"name": "Hyperliquid", "chainId": "0x3e7"},
            "message": {"message": "hello ows"}
        }))
        .unwrap();
        let signing_hash = typed_data.eip712_signing_hash().unwrap();

        let signature = sign_typed_data(&config, &typed_data).unwrap();
        let alloy_signature: alloy_primitives::Signature = signature.into();
        let recovered = alloy_signature
            .recover_address_from_prehash(&signing_hash)
            .unwrap();

        assert_eq!(recovered, config.address());
    }
}
