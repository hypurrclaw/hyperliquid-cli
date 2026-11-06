//! Shared helpers for signed Hyperliquid exchange actions.

use alloy::dyn_abi::{Eip712Types, Resolver, TypedData};
use alloy::primitives::keccak256;
use alloy::sol_types::SolStruct;
use chrono::Utc;
use hypersdk::Address;
use hypersdk::hypercore::Chain;
use hypersdk::hypercore::types::{Action, Response, Signature};
use serde::Serialize;

use crate::errors::{self, CliError};
use crate::http_api::post_exchange_json;
use crate::response_sanitization::Untrusted;
use crate::signing::SelectedSigner;

const HYPERLIQUID_EIP_PREFIX: &str = "HyperliquidTransaction:";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SignedJsonActionRequest {
    action: serde_json::Value,
    nonce: u64,
    signature: Signature,
    vault_address: Option<hypersdk::Address>,
    expires_after: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RawL1ActionRequest<'a, A> {
    action: &'a A,
    nonce: u64,
    signature: Signature,
    vault_address: Option<Address>,
    expires_after: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RawL1ActionMetadata {
    nonce: u64,
    vault_address: Option<Address>,
    expires_after: Option<u64>,
}

impl RawL1ActionMetadata {
    pub(crate) fn new(nonce: u64) -> Self {
        Self {
            nonce,
            vault_address: None,
            expires_after: None,
        }
    }

    pub(crate) fn with_vault_address(mut self, vault_address: Option<Address>) -> Self {
        self.vault_address = vault_address;
        self
    }
}

/// Current timestamp in milliseconds for Hyperliquid nonces.
#[must_use]
pub(crate) fn nonce_now() -> u64 {
    Utc::now().timestamp_millis() as u64
}

/// Sign and send an L1 action supported by hypersdk's raw [`Action`] enum.
pub(crate) async fn send_l1_action(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    action: Action,
    nonce: u64,
) -> Result<(), CliError> {
    let request = signer.sign_l1_action_sync(action, nonce, None, chain)?;
    post_json(api_base_url, &request, "exchange action").await
}

/// Sign and send an L1 action supported by hypersdk, returning the raw ok response payload.
pub(crate) async fn send_l1_action_raw(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    action: Action,
    nonce: u64,
    vault_address: Option<Address>,
    context: &'static str,
) -> Result<serde_json::Value, CliError> {
    let request = signer.sign_l1_action_sync(action, nonce, vault_address, chain)?;
    let parsed = post_exchange_json::<serde_json::Value>(api_base_url, &request, "").await?;
    ensure_ok_raw_response(parsed, context)
}

/// Sign a user-signed EIP-712 action with a custom primary type.
pub(crate) fn sign_user_action<T: SolStruct>(
    signer: &SelectedSigner,
    chain: Chain,
    message: &impl Serialize,
) -> Result<Signature, CliError> {
    let typed_data = typed_data::<T>(message, chain);
    signer.sign_typed_data(&typed_data)
}

/// Send a user-signed JSON action that is not currently modeled in hypersdk's Action enum.
pub(crate) async fn send_user_signed_json_action(
    api_base_url: &str,
    action: serde_json::Value,
    nonce: u64,
    signature: Signature,
) -> Result<(), CliError> {
    let request = SignedJsonActionRequest {
        action,
        nonce,
        signature,
        vault_address: None,
        expires_after: None,
    };
    post_json(api_base_url, &request, "exchange action").await
}

/// Sign and send a raw L1 action not currently modeled by hypersdk's [`Action`] enum.
pub(crate) async fn send_raw_l1_json_action<A: Serialize>(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    action: &A,
    metadata: RawL1ActionMetadata,
    context: &'static str,
) -> Result<serde_json::Value, CliError> {
    let signature = sign_raw_l1_action(
        signer,
        chain,
        action,
        metadata.nonce,
        metadata.vault_address,
        metadata.expires_after,
    )
    .await?;
    let request = RawL1ActionRequest {
        action,
        nonce: metadata.nonce,
        signature,
        vault_address: metadata.vault_address,
        expires_after: metadata.expires_after,
    };
    post_json_raw(api_base_url, &request, context).await
}

fn typed_data<T: SolStruct>(message: &impl Serialize, chain: Chain) -> TypedData {
    let mut resolver = Resolver::from_struct::<T>();
    resolver
        .ingest_string(T::eip712_encode_type())
        .expect("failed to ingest EIP-712 type");

    let mut types = Eip712Types::from(&resolver);
    let primary_type = types.remove(T::NAME).expect("missing primary EIP-712 type");
    types.insert(format!("{HYPERLIQUID_EIP_PREFIX}{}", T::NAME), primary_type);

    TypedData {
        domain: chain.domain(),
        resolver: Resolver::from(types),
        primary_type: format!("{HYPERLIQUID_EIP_PREFIX}{}", T::NAME),
        message: serde_json::to_value(message).expect("serialize typed-data message"),
    }
}

async fn sign_raw_l1_action<A: Serialize>(
    signer: &SelectedSigner,
    chain: Chain,
    action: &A,
    nonce: u64,
    vault_address: Option<Address>,
    expires_after: Option<u64>,
) -> Result<Signature, CliError> {
    let connection_id = raw_rmp_hash(action, nonce, vault_address, expires_after)
        .map_err(|err| CliError::Internal(anyhow::anyhow!("failed to encode action: {err}")))?;
    signer.sign_l1_connection_id(chain, connection_id).await
}

fn raw_rmp_hash<T: Serialize>(
    value: &T,
    nonce: u64,
    maybe_vault_address: Option<Address>,
    maybe_expires_after: Option<u64>,
) -> Result<alloy::primitives::B256, rmp_serde::encode::Error> {
    let mut bytes = rmp_serde::to_vec_named(value)?;
    bytes.extend(nonce.to_be_bytes());

    if let Some(vault_address) = maybe_vault_address {
        bytes.push(1);
        bytes.extend(vault_address.as_slice());
    } else {
        bytes.push(0);
    }

    if let Some(expires_after) = maybe_expires_after {
        bytes.push(0);
        bytes.extend(expires_after.to_be_bytes());
    }

    Ok(keccak256(bytes))
}

async fn post_json(
    api_base_url: &str,
    request: &impl Serialize,
    context: &'static str,
) -> Result<(), CliError> {
    let parsed = post_exchange_json::<Response>(api_base_url, request, "").await?;
    ensure_default_response(parsed, context)
}

async fn post_json_raw(
    api_base_url: &str,
    request: &impl Serialize,
    context: &'static str,
) -> Result<serde_json::Value, CliError> {
    let parsed = post_exchange_json::<serde_json::Value>(api_base_url, request, "").await?;
    ensure_ok_raw_response(parsed, context)
}

fn ensure_ok_raw_response(
    response: serde_json::Value,
    context: &'static str,
) -> Result<serde_json::Value, CliError> {
    let status = response
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();

    if status.eq_ignore_ascii_case("ok") {
        return Ok(response
            .get("response")
            .cloned()
            .unwrap_or(serde_json::Value::Null));
    }

    let message = response
        .get("response")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| response.to_string());
    Err(errors::from_exchange_rejection(&message, context))
}

pub(crate) fn ensure_default_response(
    response: Response,
    context: &'static str,
) -> Result<(), CliError> {
    match response {
        Response::Ok(hypersdk::hypercore::types::OkResponse::Default) => Ok(()),
        Response::Err(err) => Err(errors::from_exchange_rejection(&err, context)),
        other => Err(CliError::internal_with_remote(
            format!("{context}: unexpected response type: "),
            &Untrusted::from_remote(&format!("{other:?}")),
        )),
    }
}
