//! Bankr Wallet API signer integration.
//!
//! Bankr is treated as an external signer source. The CLI resolves the EVM
//! wallet attached to a Bankr API key, asks Bankr to sign supported payloads,
//! and verifies the recovered signer address before accepting the signature.

use std::env;
use std::future::Future;
use std::sync::OnceLock;
use std::time::Duration;

use alloy::dyn_abi::TypedData;
use alloy_primitives::{Address as AlloyAddress, B256};
use hypersdk::Address;
use hypersdk::hypercore::types::Signature as HyperliquidSignature;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;

use crate::errors::{CliError, http_response_indicates_rate_limit};
use crate::response_sanitization::labelled_untrusted_text;

const DEFAULT_BANKR_API_BASE_URL: &str = "https://api.bankr.bot";
const DEFAULT_SELECTOR: &str = "default";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

pub const BANKR_API_KEY_ENV: &str = "BANKR_API_KEY";
pub const BANKR_API_BASE_URL_ENV: &str = "BANKR_API_BASE_URL";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BankrSigningConfig {
    selector: String,
    api_key: String,
    api_base_url: String,
    address: Address,
}

impl BankrSigningConfig {
    #[must_use]
    pub fn new(selector: String, api_key: String, api_base_url: String, address: Address) -> Self {
        Self {
            selector,
            api_key,
            api_base_url,
            address,
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

    fn api_key(&self) -> &str {
        &self.api_key
    }

    fn api_base_url(&self) -> &str {
        &self.api_base_url
    }
}

pub fn resolve_selector(selector: &str) -> Result<BankrSigningConfig, CliError> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err(CliError::Configuration(
            "--bankr-signer requires a non-empty selector".to_string(),
        ));
    }
    if selector != DEFAULT_SELECTOR {
        return Err(CliError::Unsupported(format!(
            "bankr_unsupported_selector: only '{DEFAULT_SELECTOR}' is currently supported"
        )));
    }

    let api_key = bankr_api_key()?;
    let api_base_url = bankr_api_base_url();
    let address = wallet_address(&api_base_url, &api_key)?;
    Ok(BankrSigningConfig::new(
        selector.to_string(),
        api_key,
        api_base_url,
        address,
    ))
}

pub fn unsupported_raw_l1_signing(selector: &str) -> CliError {
    CliError::Unsupported(format!(
        "Bankr signer '{selector}' cannot sign raw Hyperliquid L1 action hashes with the currently documented Bankr Wallet API; use OWS, a local signing account, keystore, or private key for this live action"
    ))
}

pub fn unsupported_local_private_key(selector: &str) -> CliError {
    CliError::Unsupported(format!(
        "Bankr signer '{selector}' does not expose a local private key; use a local signing account, keystore, or private key for this command"
    ))
}

pub fn sign_typed_data(
    config: &BankrSigningConfig,
    typed_data: &TypedData,
) -> Result<HyperliquidSignature, CliError> {
    let typed_data_value = bankr_typed_data_value(typed_data)?;
    let response: SignResponse = post_json(
        config.api_base_url(),
        "/wallet/sign",
        config.api_key(),
        &serde_json::json!({
            "signatureType": "eth_signTypedData_v4",
            "typedData": typed_data_value,
        }),
        "signing Bankr typed data",
    )?;
    let signature = hyperliquid_signature_from_bankr(response)?;
    let alloy_signature: alloy_primitives::Signature = signature.into();
    let signing_hash = typed_data.eip712_signing_hash().map_err(|err| {
        CliError::Internal(anyhow::anyhow!("failed to hash Bankr typed data: {err}"))
    })?;
    verify_recovered_address(config, &alloy_signature, signing_hash)?;
    Ok(signature)
}

pub fn sign_message(
    config: &BankrSigningConfig,
    message: &[u8],
) -> Result<alloy_primitives::Signature, CliError> {
    let message = std::str::from_utf8(message).map_err(|_| {
        CliError::Unsupported(
            "bankr_unsupported_message: Bankr personal_sign requires UTF-8 message bytes"
                .to_string(),
        )
    })?;
    let response: SignResponse = post_json(
        config.api_base_url(),
        "/wallet/sign",
        config.api_key(),
        &serde_json::json!({
            "signatureType": "personal_sign",
            "message": message,
        }),
        "signing Bankr message",
    )?;
    let signature = alloy_signature_from_bankr(response)?;
    let recovered = signature
        .recover_address_from_msg(message.as_bytes())
        .map_err(|err| {
            CliError::InvalidAuth(format!(
                "bankr_malformed_response: failed to recover Bankr message signer: {err}"
            ))
        })?;
    verify_address_matches(config, recovered)?;
    Ok(signature)
}

fn wallet_address(api_base_url: &str, api_key: &str) -> Result<Address, CliError> {
    let response: WalletMeResponse = get_json(
        api_base_url,
        "/wallet/me",
        api_key,
        "resolving Bankr wallet",
    )?;
    let address = response
        .evm_address()
        .ok_or_else(|| {
            CliError::InvalidAuth(
                "bankr_no_evm_wallet: Bankr wallet response did not include an EVM address"
                    .to_string(),
            )
        })?
        .parse::<Address>()
        .map_err(|_| {
            CliError::InvalidAuth(
                "bankr_malformed_response: Bankr returned an invalid EVM address".to_string(),
            )
        })?;
    Ok(address)
}

fn bankr_http_client() -> Result<&'static reqwest::Client, CliError> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(DEFAULT_TIMEOUT)
                .build()
                .map_err(|err| err.to_string())
        })
        .as_ref()
        .map_err(|err| CliError::Internal(anyhow::anyhow!("failed to build Bankr HTTP client: {err}")))
}

/// Run async Bankr HTTP from sync call sites (CLI signer resolution, examples, tests).
fn run_bankr_async<F, Fut, T>(f: F) -> Result<T, CliError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, CliError>>,
    T: Send,
{
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(f())),
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?
            .block_on(f()),
    }
}

fn get_json<T: for<'de> Deserialize<'de> + Send>(
    api_base_url: &str,
    path: &str,
    api_key: &str,
    context: &'static str,
) -> Result<T, CliError> {
    let api_base_url = api_base_url.to_string();
    let api_key = api_key.to_string();
    run_bankr_async(move || async move {
        get_json_async(&api_base_url, path, &api_key, context).await
    })
}

async fn get_json_async<T: for<'de> Deserialize<'de> + Send>(
    api_base_url: &str,
    path: &str,
    api_key: &str,
    context: &'static str,
) -> Result<T, CliError> {
    let url = bankr_url(api_base_url, path)?;
    let response = bankr_http_client()?
        .get(url)
        .header("X-API-Key", api_key)
        .send()
        .await
        .map_err(|err| CliError::Unavailable(format!("Check your network connection. {err}")))?;
    decode_response(response, context).await
}

fn post_json<T: for<'de> Deserialize<'de> + Send>(
    api_base_url: &str,
    path: &str,
    api_key: &str,
    payload: &Value,
    context: &'static str,
) -> Result<T, CliError> {
    let api_base_url = api_base_url.to_string();
    let api_key = api_key.to_string();
    let payload = payload.clone();
    run_bankr_async(move || async move {
        post_json_async(&api_base_url, path, &api_key, &payload, context).await
    })
}

async fn post_json_async<T: for<'de> Deserialize<'de> + Send>(
    api_base_url: &str,
    path: &str,
    api_key: &str,
    payload: &Value,
    context: &'static str,
) -> Result<T, CliError> {
    let url = bankr_url(api_base_url, path)?;
    let response = bankr_http_client()?
        .post(url)
        .header("X-API-Key", api_key)
        .json(payload)
        .send()
        .await
        .map_err(|err| CliError::Unavailable(format!("Check your network connection. {err}")))?;
    decode_response(response, context).await
}

async fn decode_response<T: for<'de> Deserialize<'de> + Send>(
    response: reqwest::Response,
    context: &'static str,
) -> Result<T, CliError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| CliError::Unavailable(format!("Failed to read Bankr response. {err}")))?;
    ensure_bankr_success(status, &body)?;
    serde_json::from_str::<T>(&body).map_err(|err| {
        let body = labelled_untrusted_text(&body);
        CliError::InvalidAuth(format!(
            "bankr_malformed_response: decode failed while {context}: {err}; body={body}"
        ))
    })
}

fn ensure_bankr_success(status: StatusCode, body: &str) -> Result<(), CliError> {
    if http_response_indicates_rate_limit(status.as_u16(), body) {
        return Err(CliError::RateLimited);
    }
    if status == StatusCode::UNAUTHORIZED {
        return Err(CliError::InvalidAuth(bankr_error_message(
            "bankr_auth_required",
            body,
        )));
    }
    if status == StatusCode::FORBIDDEN {
        return Err(CliError::InvalidAuth(bankr_error_message(
            "bankr_access_denied",
            body,
        )));
    }
    if status == StatusCode::BAD_REQUEST {
        return Err(CliError::InvalidAuth(bankr_error_message(
            "bankr_bad_request",
            body,
        )));
    }
    if !status.is_success() {
        return Err(CliError::Unavailable(bankr_error_message(
            "bankr_unavailable",
            body,
        )));
    }
    if let Ok(value) = serde_json::from_str::<Value>(body)
        && value.get("success") == Some(&Value::Bool(false))
    {
        return Err(CliError::InvalidAuth(bankr_error_message(
            "bankr_sign_failed",
            body,
        )));
    }
    Ok(())
}

fn bankr_error_message(prefix: &'static str, body: &str) -> String {
    let body = labelled_untrusted_text(body);
    format!("{prefix}: {body}")
}

fn bankr_url(api_base_url: &str, path: &str) -> Result<reqwest::Url, CliError> {
    let mut url = reqwest::Url::parse(api_base_url)
        .map_err(|err| CliError::Configuration(format!("invalid Bankr API base URL: {err}")))?;
    url.set_path(path);
    url.set_query(None);
    Ok(url)
}

fn bankr_api_key() -> Result<String, CliError> {
    env::var(BANKR_API_KEY_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            CliError::InvalidAuth(format!(
                "{BANKR_API_KEY_ENV} is required when using --bankr-signer"
            ))
        })
}

fn bankr_api_base_url() -> String {
    env::var(BANKR_API_BASE_URL_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_BANKR_API_BASE_URL.to_string())
}

fn bankr_typed_data_value(typed_data: &TypedData) -> Result<Value, CliError> {
    let mut value = serde_json::to_value(typed_data).map_err(|err| {
        CliError::Internal(anyhow::anyhow!("failed to encode Bankr typed data: {err}"))
    })?;
    ensure_eip712_domain_type(&mut value);
    pad_odd_hex_typed_values(&mut value);
    normalize_bankr_numeric_fields(&mut value);
    Ok(value)
}

fn ensure_eip712_domain_type(value: &mut Value) {
    let Some(domain) = value.get("domain").and_then(Value::as_object).cloned() else {
        return;
    };
    let Some(types) = value.get_mut("types").and_then(Value::as_object_mut) else {
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
    types.insert("EIP712Domain".to_string(), Value::Array(fields));
}

fn pad_odd_hex_typed_values(value: &mut Value) {
    let Some(types) = value.get("types").and_then(Value::as_object).cloned() else {
        return;
    };
    if let Some(domain) = value.get_mut("domain") {
        pad_struct_odd_hex_values("EIP712Domain", domain, &types);
    }
    let Some(primary_type) = value
        .get("primaryType")
        .and_then(Value::as_str)
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
    value: &mut Value,
    types: &serde_json::Map<String, Value>,
) {
    let Some(fields) = types.get(type_name).and_then(Value::as_array).cloned() else {
        return;
    };
    let Some(object) = value.as_object_mut() else {
        return;
    };

    for field in fields {
        let Some(name) = field.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(field_type) = field.get("type").and_then(Value::as_str) else {
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
    value: &mut Value,
    types: &serde_json::Map<String, Value>,
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
        *value = Value::String(format!("0x0{hex}"));
    }
}

fn is_hex_encoded_eip712_scalar(field_type: &str) -> bool {
    field_type == "bytes"
        || field_type.starts_with("bytes")
        || field_type.starts_with("uint")
        || field_type.starts_with("int")
}

fn normalize_bankr_numeric_fields(value: &mut Value) {
    let Some(types) = value.get("types").and_then(Value::as_object).cloned() else {
        return;
    };
    if let Some(domain) = value.get_mut("domain") {
        normalize_struct_bankr_numeric_fields("EIP712Domain", domain, &types);
    }
    let Some(primary_type) = value
        .get("primaryType")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    if let Some(message) = value.get_mut("message") {
        normalize_struct_bankr_numeric_fields(&primary_type, message, &types);
    }
}

fn normalize_struct_bankr_numeric_fields(
    type_name: &str,
    value: &mut Value,
    types: &serde_json::Map<String, Value>,
) {
    let Some(fields) = types.get(type_name).and_then(Value::as_array).cloned() else {
        return;
    };
    let Some(object) = value.as_object_mut() else {
        return;
    };

    for field in fields {
        let Some(name) = field.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(field_type) = field.get("type").and_then(Value::as_str) else {
            continue;
        };
        let Some(field_value) = object.get_mut(name) else {
            continue;
        };
        normalize_field_bankr_numeric_values(field_type, field_value, types);
    }
}

fn normalize_field_bankr_numeric_values(
    field_type: &str,
    value: &mut Value,
    types: &serde_json::Map<String, Value>,
) {
    if let Some(element_type) = field_type.strip_suffix("[]") {
        if let Some(values) = value.as_array_mut() {
            for value in values {
                normalize_field_bankr_numeric_values(element_type, value, types);
            }
        }
        return;
    }

    if types.contains_key(field_type) {
        normalize_struct_bankr_numeric_fields(field_type, value, types);
        return;
    }

    if is_eip712_integer_scalar(field_type) {
        normalize_bankr_integer_scalar(value);
    }
}

fn is_eip712_integer_scalar(field_type: &str) -> bool {
    field_type.starts_with("uint") || field_type.starts_with("int")
}

fn normalize_bankr_integer_scalar(value: &mut Value) {
    let Some(text) = value.as_str() else {
        return;
    };
    if let Some(number) = parse_bankr_integer_scalar(text) {
        *value = serde_json::Number::from(number).into();
    }
}

fn parse_bankr_integer_scalar(text: &str) -> Option<u64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        return u64::from_str_radix(hex, 16).ok();
    }
    trimmed.parse::<u64>().ok()
}

fn hyperliquid_signature_from_bankr(
    response: SignResponse,
) -> Result<HyperliquidSignature, CliError> {
    let signature = canonical_signature_hex(&response.signature)?;
    signature.parse::<HyperliquidSignature>().map_err(|err| {
        CliError::InvalidAuth(format!(
            "bankr_malformed_response: failed to parse Bankr signature: {err}"
        ))
    })
}

fn alloy_signature_from_bankr(
    response: SignResponse,
) -> Result<alloy_primitives::Signature, CliError> {
    let signature = hyperliquid_signature_from_bankr(response)?;
    Ok(signature.into())
}

fn canonical_signature_hex(signature: &str) -> Result<String, CliError> {
    let sig_hex = signature.strip_prefix("0x").unwrap_or(signature);
    if sig_hex.len() != 130 {
        return Err(CliError::InvalidAuth(format!(
            "bankr_malformed_response: unexpected Bankr signature length {} hex chars; expected 130",
            sig_hex.len()
        )));
    }
    let v = u8::from_str_radix(&sig_hex[128..130], 16).map_err(|err| {
        CliError::InvalidAuth(format!(
            "bankr_malformed_response: invalid Bankr recovery byte: {err}"
        ))
    })?;
    let canonical_v = if v >= 27 { v } else { v + 27 };
    Ok(format!("0x{}{canonical_v:02x}", &sig_hex[..128]))
}

fn verify_recovered_address(
    config: &BankrSigningConfig,
    signature: &alloy_primitives::Signature,
    signing_hash: B256,
) -> Result<(), CliError> {
    let recovered = signature
        .recover_address_from_prehash(&signing_hash)
        .map_err(|err| {
            CliError::InvalidAuth(format!(
                "bankr_malformed_response: failed to recover Bankr signer: {err}"
            ))
        })?;
    verify_address_matches(config, recovered)
}

fn verify_address_matches(
    config: &BankrSigningConfig,
    recovered: AlloyAddress,
) -> Result<(), CliError> {
    let expected: AlloyAddress = config.address;
    if recovered == expected {
        return Ok(());
    }

    Err(CliError::InvalidAuth(format!(
        "bankr_signer_mismatch: Bankr signature recovered {recovered}, expected {}",
        config.address
    )))
}

#[derive(Debug, Deserialize)]
struct WalletMeResponse {
    #[serde(default, rename = "evmAddress")]
    evm_address: Option<String>,
    #[serde(default)]
    wallets: Vec<BankrWalletAddress>,
}

impl WalletMeResponse {
    fn evm_address(&self) -> Option<&str> {
        self.evm_address.as_deref().or_else(|| {
            self.wallets
                .iter()
                .find(|wallet| wallet.chain.eq_ignore_ascii_case("evm"))
                .map(|wallet| wallet.address.as_str())
        })
    }
}

#[derive(Debug, Deserialize)]
struct BankrWalletAddress {
    chain: String,
    address: String,
}

#[derive(Debug, Deserialize)]
struct SignResponse {
    signature: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    use alloy_v1::signers::SignerSync;
    use hypersdk::hypercore::PrivateKeySigner;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000009";
    const API_KEY: &str = "bk_test_key";

    fn test_typed_data() -> TypedData {
        serde_json::from_value(serde_json::json!({
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
            "message": {"message": "hello bankr"}
        }))
        .unwrap()
    }

    fn config(api_base_url: String) -> BankrSigningConfig {
        let signer = KEY.parse::<PrivateKeySigner>().unwrap();
        BankrSigningConfig::new(
            "default".to_string(),
            API_KEY.to_string(),
            api_base_url,
            signer.address(),
        )
    }

    #[test]
    fn bankr_typed_data_value_normalizes_integer_fields_for_bankr_api() {
        let typed_data = test_typed_data();
        let value = bankr_typed_data_value(&typed_data).unwrap();

        assert_eq!(value["domain"]["chainId"], 999);
    }

    async fn run_bankr<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        tokio::task::spawn_blocking(f).await.unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn wallet_address_extracts_evm_wallet_from_bankr_response() {
        let server = MockServer::start().await;
        let signer = KEY.parse::<PrivateKeySigner>().unwrap();
        Mock::given(method("GET"))
            .and(path("/wallet/me"))
            .and(header("X-API-Key", API_KEY))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "wallets": [
                    {"chain": "solana", "address": "5DcK"},
                    {"chain": "evm", "address": signer.address().to_string()}
                ]
            })))
            .mount(&server)
            .await;

        let server_uri = server.uri();
        let address = run_bankr(move || wallet_address(&server_uri, API_KEY).unwrap()).await;

        assert_eq!(address, signer.address());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sign_typed_data_verifies_recovered_bankr_address() {
        let server = MockServer::start().await;
        let typed_data = test_typed_data();
        let signer = KEY.parse::<PrivateKeySigner>().unwrap();
        let signature = signer.sign_dynamic_typed_data_sync(&typed_data).unwrap();
        Mock::given(method("POST"))
            .and(path("/wallet/sign"))
            .and(header("X-API-Key", API_KEY))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "signature": signature.to_string(),
                "signer": signer.address().to_string(),
                "signatureType": "eth_signTypedData_v4"
            })))
            .mount(&server)
            .await;

        let server_uri = server.uri();
        let typed_data_for_signing = typed_data.clone();
        let signature = run_bankr(move || {
            sign_typed_data(&config(server_uri), &typed_data_for_signing).unwrap()
        })
        .await;
        let alloy_signature: alloy_primitives::Signature = signature.into();
        let recovered = alloy_signature
            .recover_address_from_prehash(&typed_data.eip712_signing_hash().unwrap())
            .unwrap();

        assert_eq!(recovered, signer.address());
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests[0].body_json::<serde_json::Value>().unwrap()["signatureType"],
            "eth_signTypedData_v4"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sign_typed_data_rejects_signer_mismatch() {
        let server = MockServer::start().await;
        let typed_data = test_typed_data();
        let signer = KEY.parse::<PrivateKeySigner>().unwrap();
        let other = "0x000000000000000000000000000000000000000000000000000000000000000a"
            .parse::<PrivateKeySigner>()
            .unwrap();
        let signature = other.sign_dynamic_typed_data_sync(&typed_data).unwrap();
        Mock::given(method("POST"))
            .and(path("/wallet/sign"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "signature": signature.to_string()
            })))
            .mount(&server)
            .await;

        let server_uri = server.uri();
        let typed_data_for_signing = typed_data.clone();
        let err = run_bankr(move || {
            sign_typed_data(&config(server_uri), &typed_data_for_signing).unwrap_err()
        })
        .await;

        assert_eq!(err.exit_code(), 10);
        assert!(err.to_string().contains("bankr_signer_mismatch"));
        assert_ne!(signer.address(), other.address());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sign_message_verifies_recovered_bankr_address() {
        let server = MockServer::start().await;
        let signer = KEY.parse::<PrivateKeySigner>().unwrap();
        let signature = signer.sign_message_sync(b"hello bankr").unwrap();
        Mock::given(method("POST"))
            .and(path("/wallet/sign"))
            .and(header("X-API-Key", API_KEY))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "signature": signature.to_string(),
                "signer": signer.address().to_string(),
                "signatureType": "personal_sign"
            })))
            .mount(&server)
            .await;

        let server_uri = server.uri();
        let signature =
            run_bankr(move || sign_message(&config(server_uri), b"hello bankr").unwrap()).await;
        let recovered = signature.recover_address_from_msg(b"hello bankr").unwrap();

        assert_eq!(recovered, signer.address());
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests[0].body_json::<serde_json::Value>().unwrap()["signatureType"],
            "personal_sign"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sign_message_rejects_non_utf8_messages_before_request() {
        let server = MockServer::start().await;
        let err = sign_message(&config(server.uri()), &[0xff]).unwrap_err();

        assert_eq!(err.exit_code(), 13);
        assert!(err.to_string().contains("bankr_unsupported_message"));
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn bankr_forbidden_response_maps_to_auth_failure_without_leaking_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wallet/me"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "error": "Read-only API key"
            })))
            .mount(&server)
            .await;

        let server_uri = server.uri();
        let err = run_bankr(move || wallet_address(&server_uri, API_KEY).unwrap_err()).await;

        assert_eq!(err.exit_code(), 10);
        assert!(err.to_string().contains("bankr_access_denied"));
        assert!(!err.to_string().contains(API_KEY));
    }
}
