//! Wallet-neutral signer abstractions.
//!
//! This module is intentionally additive for now: existing command paths still
//! use `PrivateKeySigner` directly, while new code can depend on `SelectedSigner`
//! as the stable boundary that future OWS support will implement.

// TypedData comes from the app-level Alloy 2 dependency, while local signing
// still uses hypersdk's Alloy 1 signer trait. This is intentionally temporary:
// both versions share the same alloy-dyn-abi/core typed-data representation in
// this dependency set, and `alloy_v1` can be removed when hypersdk moves to
// Alloy 2.
use alloy::dyn_abi::TypedData;
use alloy_v1::signers::SignerSync;
use hypersdk::Address;
use hypersdk::hypercore::signing::{agent_signing_hash, sign_l1_action};
use hypersdk::hypercore::types::{Action, ActionRequest, Signature};
use hypersdk::hypercore::{Chain, PrivateKeySigner};

use crate::errors::CliError;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SignerSource {
    PrivateKey,
    Keystore,
    StoredAccount { alias: String },
    Ows { selector: String },
}

#[derive(Debug, Clone)]
enum SignerBackend {
    LocalPrivateKey(PrivateKeySigner),
    Ows(crate::ows::OwsSigningConfig),
}

#[derive(Debug, Clone)]
pub struct SelectedSigner {
    backend: SignerBackend,
    source: SignerSource,
    query_address: Address,
}

impl SelectedSigner {
    #[must_use]
    pub fn local_private_key(
        signer: PrivateKeySigner,
        source: SignerSource,
        query_address: Address,
    ) -> Self {
        Self {
            backend: SignerBackend::LocalPrivateKey(signer),
            source,
            query_address,
        }
    }

    #[must_use]
    pub fn experimental_ows(selector: String, address: Address) -> Self {
        Self::ows(crate::ows::OwsSigningConfig::new(selector, None, address))
    }

    #[must_use]
    pub fn ows(config: crate::ows::OwsSigningConfig) -> Self {
        let selector = config.selector().to_string();
        let address = config.address();
        Self {
            backend: SignerBackend::Ows(config),
            source: SignerSource::Ows { selector },
            query_address: address,
        }
    }

    #[must_use]
    pub fn address(&self) -> Address {
        match &self.backend {
            SignerBackend::LocalPrivateKey(signer) => signer.address(),
            SignerBackend::Ows(config) => config.address(),
        }
    }

    #[must_use]
    pub fn query_address(&self) -> Address {
        self.query_address
    }

    #[must_use]
    pub fn source(&self) -> &SignerSource {
        &self.source
    }

    pub fn private_key_signer(&self) -> Result<&PrivateKeySigner, CliError> {
        match &self.backend {
            SignerBackend::LocalPrivateKey(signer) => Ok(signer),
            SignerBackend::Ows(config) => {
                Err(crate::ows::unsupported_live_signing(config.selector()))
            }
        }
    }

    /// Guard network lookups that should fail before side effects when an OWS selector
    /// is only an address placeholder. This is deliberately not a command allowlist:
    /// command-level OWS status is exposed in schema/docs, while the signing helpers
    /// still verify the recovered address for every payload before submission.
    pub fn ensure_can_attempt_live_signing(&self) -> Result<(), CliError> {
        match &self.backend {
            SignerBackend::LocalPrivateKey(_) => Ok(()),
            SignerBackend::Ows(config) if config.has_resolved_wallet() => Ok(()),
            SignerBackend::Ows(config) => {
                Err(crate::ows::unsupported_live_signing(config.selector()))
            }
        }
    }

    pub fn sign_l1_action_sync(
        &self,
        action: Action,
        nonce: u64,
        vault_address: Option<Address>,
        chain: Chain,
    ) -> Result<serde_json::Value, CliError> {
        match &self.backend {
            SignerBackend::LocalPrivateKey(signer) => {
                let request = action
                    .sign_sync(signer, nonce, vault_address, None, chain)
                    .map_err(|err| {
                        CliError::Internal(anyhow::anyhow!("failed to sign action: {err}"))
                    })?;
                serde_json::to_value(request).map_err(|err| {
                    CliError::Internal(anyhow::anyhow!("failed to encode signed action: {err}"))
                })
            }
            SignerBackend::Ows(config) => {
                let signing_hash =
                    action
                        .prehash(nonce, vault_address, None, chain)
                        .map_err(|err| {
                            CliError::Internal(anyhow::anyhow!("failed to hash OWS action: {err}"))
                        })?;
                let signature = crate::ows::sign_hash(config, signing_hash)?;
                let request = ActionRequest {
                    action,
                    nonce,
                    signature,
                    vault_address,
                    expires_after: None,
                };
                serde_json::to_value(request).map_err(|err| {
                    CliError::Internal(anyhow::anyhow!("failed to encode signed action: {err}"))
                })
            }
        }
    }

    pub fn sign_typed_data(&self, typed_data: &TypedData) -> Result<Signature, CliError> {
        match &self.backend {
            SignerBackend::LocalPrivateKey(signer) => signer
                .sign_dynamic_typed_data_sync(typed_data)
                .map(Into::into)
                .map_err(|err| CliError::Internal(anyhow::anyhow!("failed to sign action: {err}"))),
            SignerBackend::Ows(config) => crate::ows::sign_typed_data(config, typed_data),
        }
    }

    pub async fn sign_l1_connection_id(
        &self,
        chain: Chain,
        connection_id: alloy_primitives::B256,
    ) -> Result<Signature, CliError> {
        match &self.backend {
            SignerBackend::LocalPrivateKey(signer) => sign_l1_action(signer, chain, connection_id)
                .await
                .map_err(|err| CliError::Internal(anyhow::anyhow!("failed to sign action: {err}"))),
            SignerBackend::Ows(config) => {
                crate::ows::sign_hash(config, agent_signing_hash(chain, connection_id))
            }
        }
    }

    pub fn sign_message(&self, message: &[u8]) -> Result<alloy_primitives::Signature, CliError> {
        match &self.backend {
            SignerBackend::LocalPrivateKey(signer) => {
                signer.sign_message_sync(message).map_err(|err| {
                    CliError::Internal(anyhow::anyhow!("failed to sign message: {err}"))
                })
            }
            SignerBackend::Ows(config) => crate::ows::sign_message(config, message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000009";

    #[test]
    fn selected_local_signer_exposes_address_source_and_query_address() {
        let signer = KEY.parse::<PrivateKeySigner>().unwrap();
        let address = signer.address();
        let selected = SelectedSigner::local_private_key(signer, SignerSource::PrivateKey, address);

        assert_eq!(selected.address(), address);
        assert_eq!(selected.query_address(), address);
        assert_eq!(selected.source(), &SignerSource::PrivateKey);
    }

    #[test]
    fn selected_local_signer_can_sign_messages() {
        let signer = KEY.parse::<PrivateKeySigner>().unwrap();
        let address = signer.address();
        let selected = SelectedSigner::local_private_key(signer, SignerSource::PrivateKey, address);

        let signature = selected.sign_message(b"hyperliquid-cli").unwrap();
        let recovered = signature
            .recover_address_from_msg(b"hyperliquid-cli")
            .unwrap();

        assert_eq!(recovered, address);
    }

    #[test]
    fn resolved_ows_signer_can_build_signed_l1_action_request() {
        let tmp = tempfile::TempDir::new().unwrap();
        let wallet = ows_lib::import_wallet_private_key(
            "signing-test-wallet",
            KEY,
            Some("hyperliquid"),
            None,
            Some(tmp.path()),
            None,
            None,
        )
        .unwrap();
        let address = KEY.parse::<PrivateKeySigner>().unwrap().address();
        let selected = SelectedSigner::ows(crate::ows::OwsSigningConfig::with_vault_path(
            wallet.name,
            Some(wallet.id),
            address,
            Some(tmp.path().to_path_buf()),
        ));

        let request = selected
            .sign_l1_action_sync(
                Action::UpdateLeverage(hypersdk::hypercore::api::UpdateLeverage {
                    asset: 3,
                    is_cross: true,
                    leverage: 2,
                }),
                1_777_963_000_000,
                None,
                Chain::Testnet,
            )
            .unwrap();

        assert_eq!(request["action"]["type"], "updateLeverage");
        assert_eq!(request["action"]["asset"], 3);
        assert_eq!(request["signature"]["r"].as_str().unwrap().len(), 66);
        assert_eq!(request["signature"]["s"].as_str().unwrap().len(), 66);
    }

    #[test]
    fn experimental_ows_signer_exposes_address_but_rejects_live_signing() {
        let address: Address = "0x0000000000000000000000000000000000000001"
            .parse()
            .unwrap();
        let selected = SelectedSigner::experimental_ows(
            "0x0000000000000000000000000000000000000001".to_string(),
            address,
        );

        assert_eq!(selected.address(), address);
        assert_eq!(selected.query_address(), address);
        assert_eq!(
            selected.source(),
            &SignerSource::Ows {
                selector: "0x0000000000000000000000000000000000000001".to_string(),
            }
        );
        assert_eq!(selected.sign_message(b"nope").unwrap_err().exit_code(), 13);
    }
}
