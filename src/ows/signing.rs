//! Signing with a vault wallet.
//!
//! [`OwsSigningConfig`] is a resolved wallet (or, for an address-only
//! `--ows-signer 0x...`, an identity placeholder that can't sign). The helpers
//! sign through `ows-lib`, then check that the recovered address is the one the
//! caller selected before returning a signature.

use alloy::dyn_abi::TypedData;
use alloy::primitives::Address as AlloyAddress;
use alloy::primitives::B256;
use hypersdk::Address;
use hypersdk::hypercore::types::Signature as HyperliquidSignature;
use ows_lib::types::WalletInfo;

use super::HYPERLIQUID_CHAIN_ALIAS;
use super::vault::Vault;
use crate::errors::CliError;

#[derive(Debug, Clone)]
struct SigningWallet {
    id: String,
    name: String,
    vault: Vault,
}

#[derive(Debug, Clone)]
pub struct OwsSigningConfig {
    selector: String,
    address: Address,
    wallet: Option<SigningWallet>,
}

impl OwsSigningConfig {
    /// A vault wallet that can sign. `selector` is the text the user selected it with.
    #[must_use]
    pub fn for_wallet(
        selector: String,
        wallet: &WalletInfo,
        address: Address,
        vault: Vault,
    ) -> Self {
        Self {
            selector,
            address,
            wallet: Some(SigningWallet {
                id: wallet.id.clone(),
                name: wallet.name.clone(),
                vault,
            }),
        }
    }

    /// An address named by `--ows-signer 0x...` that matches no wallet: usable
    /// for identity previews, never for live signing.
    #[must_use]
    pub fn address_only(selector: String, address: Address) -> Self {
        Self {
            selector,
            address,
            wallet: None,
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
    pub fn wallet_name(&self) -> Option<&str> {
        self.wallet.as_ref().map(|wallet| wallet.name.as_str())
    }

    #[must_use]
    pub fn has_resolved_wallet(&self) -> bool {
        self.wallet.is_some()
    }

    fn wallet(&self) -> Result<&SigningWallet, CliError> {
        self.wallet
            .as_ref()
            .ok_or_else(|| unsupported_live_signing(&self.selector))
    }
}

/// The error for live signing with an address-only OWS selector.
pub fn unsupported_live_signing(selector: &str) -> CliError {
    CliError::Unsupported(format!(
        "OWS signer '{selector}' does not have a resolved wallet for live signing; use a stored wallet name/id, or select a local signing account, keystore, or private key for live submission"
    ))
}

pub fn sign_typed_data(
    config: &OwsSigningConfig,
    typed_data: &TypedData,
) -> Result<HyperliquidSignature, CliError> {
    let wallet = config.wallet()?;
    let typed_data_json = ows_typed_data_json(typed_data)?;
    let result = ows_lib::sign_typed_data(
        &wallet.id,
        HYPERLIQUID_CHAIN_ALIAS,
        &typed_data_json,
        wallet.vault.passphrase(),
        None,
        wallet.vault.path(),
    )
    .map_err(super::map_ows_error)?;
    let signature = hyperliquid_signature_from_ows(result)?;
    let alloy_signature: alloy::primitives::Signature = signature.into();
    let signing_hash = typed_data.eip712_signing_hash().map_err(|err| {
        CliError::Internal(anyhow::anyhow!("failed to hash OWS typed data: {err}"))
    })?;
    verify_recovered_address(config, &alloy_signature, signing_hash)?;
    Ok(signature)
}

pub fn sign_hash(config: &OwsSigningConfig, hash: B256) -> Result<HyperliquidSignature, CliError> {
    let wallet = config.wallet()?;
    let result = ows_lib::sign_hash(
        &wallet.id,
        HYPERLIQUID_CHAIN_ALIAS,
        &format!("0x{}", hex::encode(hash)),
        wallet.vault.passphrase(),
        None,
        wallet.vault.path(),
    )
    .map_err(super::map_ows_error)?;
    let signature = hyperliquid_signature_from_ows(result)?;
    let alloy_signature: alloy::primitives::Signature = signature.into();
    verify_recovered_address(config, &alloy_signature, hash)?;
    Ok(signature)
}

pub fn sign_message(
    config: &OwsSigningConfig,
    message: &[u8],
) -> Result<alloy::primitives::Signature, CliError> {
    let wallet = config.wallet()?;
    let result = ows_lib::sign_message(
        &wallet.id,
        HYPERLIQUID_CHAIN_ALIAS,
        &hex::encode(message),
        wallet.vault.passphrase(),
        Some("hex"),
        None,
        wallet.vault.path(),
    )
    .map_err(super::map_ows_error)?;
    let signature = alloy_signature_from_ows(result)?;
    let recovered = signature.recover_address_from_msg(message).map_err(|err| {
        CliError::InvalidAuth(format!(
            "ows_malformed_response: failed to recover OWS message signer: {err}"
        ))
    })?;
    verify_address_matches(config, recovered)?;
    Ok(signature)
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
) -> Result<alloy::primitives::Signature, CliError> {
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
    signature: &alloy::primitives::Signature,
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

#[cfg(test)]
mod tests {
    use super::super::vault::test_support::{KEY, import_test_wallet};
    use super::*;

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

    fn signing_config(wallet: &WalletInfo, vault_path: &std::path::Path) -> OwsSigningConfig {
        OwsSigningConfig::for_wallet(
            wallet.name.clone(),
            wallet,
            test_signer_address(),
            Vault::at(vault_path),
        )
    }

    fn test_signer_address() -> Address {
        KEY.parse::<hypersdk::hypercore::PrivateKeySigner>()
            .unwrap()
            .address()
    }

    #[test]
    fn address_only_config_exposes_identity_but_rejects_signing() {
        let address: Address = "0x0000000000000000000000000000000000000001"
            .parse()
            .unwrap();
        let config = OwsSigningConfig::address_only(address.to_string(), address);

        assert_eq!(config.address(), address);
        assert!(!config.has_resolved_wallet());
        assert_eq!(sign_message(&config, b"nope").unwrap_err().exit_code(), 13);
        assert_eq!(sign_hash(&config, B256::ZERO).unwrap_err().exit_code(), 13);
    }

    #[test]
    fn ows_hash_signing_recovers_selected_wallet_address() {
        let tmp = tempfile::TempDir::new().unwrap();
        let wallet = import_test_wallet(tmp.path(), "test-wallet");
        let config = signing_config(&wallet, tmp.path());
        let hash = B256::repeat_byte(0x42);

        let signature = sign_hash(&config, hash).unwrap();
        let alloy_signature: alloy::primitives::Signature = signature.into();
        let recovered = alloy_signature.recover_address_from_prehash(&hash).unwrap();

        assert_eq!(recovered, config.address());
    }

    #[test]
    fn ows_message_signing_recovers_selected_wallet_address() {
        let tmp = tempfile::TempDir::new().unwrap();
        let wallet = import_test_wallet(tmp.path(), "test-wallet");
        let config = signing_config(&wallet, tmp.path());
        let message = b"hyperliquid-cli ows smoke";

        let signature = sign_message(&config, message).unwrap();
        let recovered = signature.recover_address_from_msg(message).unwrap();

        assert_eq!(recovered, config.address());
    }

    #[test]
    fn signing_rejects_a_signature_from_a_different_address() {
        let tmp = tempfile::TempDir::new().unwrap();
        let wallet = import_test_wallet(tmp.path(), "test-wallet");
        let other: Address = "0x0000000000000000000000000000000000000001"
            .parse()
            .unwrap();
        let config = OwsSigningConfig::for_wallet(
            wallet.name.clone(),
            &wallet,
            other,
            Vault::at(tmp.path()),
        );

        let err = sign_hash(&config, B256::repeat_byte(0x42)).unwrap_err();
        assert_eq!(err.exit_code(), 10);
        assert!(err.to_string().contains("ows_signer_mismatch"));
    }

    #[test]
    fn ows_typed_data_signing_recovers_selected_wallet_address() {
        let tmp = tempfile::TempDir::new().unwrap();
        let wallet = import_test_wallet(tmp.path(), "test-wallet");
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
        let alloy_signature: alloy::primitives::Signature = signature.into();
        let recovered = alloy_signature
            .recover_address_from_prehash(&signing_hash)
            .unwrap();

        assert_eq!(recovered, config.address());
    }
}
