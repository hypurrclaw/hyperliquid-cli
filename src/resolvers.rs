//! Typed resolver APIs for signer and address selector classes.
//!
//! These helpers make the selector boundary explicit:
//! - signer selectors may load local signing accounts,
//! - public user and acting-account selectors may resolve stored account aliases,
//! - protocol object and raw destination addresses are explicit addresses only.

use std::fmt;
use std::path::Path;

use hypersdk::Address;

use crate::auth::{self, ResolvedSigner};
use crate::db::{Account, AccountStore};
use crate::errors::CliError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultSignerFallback {
    AllowStoredDefaultOrFirst,
    Disallow,
}

#[derive(Debug, Clone, Copy)]
pub struct SignerResolverInput<'a> {
    pub resolved_private_key: Option<&'a str>,
    pub keystore_path: Option<&'a Path>,
    pub keystore_password: Option<&'a str>,
    pub account_selector: Option<&'a str>,
    pub ows_selector: Option<&'a str>,
    pub default_fallback: DefaultSignerFallback,
}

pub fn resolve_selected_signer(
    input: SignerResolverInput<'_>,
) -> Result<ResolvedSigner, anyhow::Error> {
    if let Some(selector) = input.ows_selector {
        return auth::resolve_signer_with_account_and_ows(None, None, None, None, Some(selector));
    }

    if let Some(private_key) = input.resolved_private_key {
        return auth::resolve_signer_with_account_and_ows(
            Some(private_key),
            None,
            None,
            None,
            None,
        );
    }

    if let Some(path) = input.keystore_path {
        return auth::resolve_signer_with_account_and_ows(
            None,
            Some(path),
            input.keystore_password,
            None,
            None,
        );
    }

    if let Some(selector) = input.account_selector {
        return auth::resolve_stored_account_signer(selector);
    }

    match input.default_fallback {
        DefaultSignerFallback::AllowStoredDefaultOrFirst => auth::resolve_stored_default_signer(),
        DefaultSignerFallback::Disallow => Err(CliError::AuthRequired.into()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolUserAddress(Address);

impl ProtocolUserAddress {
    #[must_use]
    pub fn address(self) -> Address {
        self.0
    }
}

impl fmt::Display for ProtocolUserAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActingAccountAddress(Address);

impl ActingAccountAddress {
    #[must_use]
    pub fn address(self) -> Address {
        self.0
    }
}

impl fmt::Display for ActingAccountAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolObjectAddress(Address);

impl ProtocolObjectAddress {
    #[must_use]
    pub fn address(self) -> Address {
        self.0
    }
}

impl fmt::Display for ProtocolObjectAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawDestinationAddress(Address);

impl RawDestinationAddress {
    #[must_use]
    pub fn address(self) -> Address {
        self.0
    }
}

impl fmt::Display for RawDestinationAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub fn resolve_protocol_user_address(selector: &str) -> Result<ProtocolUserAddress, anyhow::Error> {
    resolve_account_selector_address(selector).map(ProtocolUserAddress)
}

pub fn resolve_acting_account_address(
    selector: &str,
) -> Result<ActingAccountAddress, anyhow::Error> {
    resolve_account_selector_address(selector).map(ActingAccountAddress)
}

pub fn resolve_optional_acting_account_address(
    selector: Option<&str>,
) -> Result<Option<ActingAccountAddress>, anyhow::Error> {
    selector.map(resolve_acting_account_address).transpose()
}

pub fn parse_protocol_object_address(value: &str) -> Result<ProtocolObjectAddress, CliError> {
    parse_explicit_address(value, ZeroAddressPolicy::Allow).map(ProtocolObjectAddress)
}

pub fn parse_raw_destination_address(value: &str) -> Result<RawDestinationAddress, CliError> {
    parse_explicit_address(value, ZeroAddressPolicy::Reject).map(RawDestinationAddress)
}

fn resolve_account_selector_address(selector: &str) -> Result<Address, anyhow::Error> {
    let trimmed = selector.trim();
    if let Ok(address) = trimmed.parse::<Address>() {
        return Ok(address);
    }

    if trimmed.starts_with("0x") {
        return Err(CliError::Unsupported(format!(
            "Invalid address: {trimmed}; expected a 0x-prefixed 40-hex-character (20-byte) string"
        ))
        .into());
    }

    // Try OWS vault first — by name/id.
    let vault_path = crate::ows::ows_vault_path();
    if let Ok(wallet) = crate::ows::get_ows_wallet(trimmed, vault_path.as_deref())
        && let Ok((_addr, address)) = crate::ows::hyperliquid_address_from_wallet(&wallet)
    {
        return Ok(address);
    }

    // Fallback: legacy SQLite account store.
    let Some(store) = AccountStore::open_existing_default()? else {
        return Err(auth::account_selector_not_found(trimmed).into());
    };
    let account = store
        .account_by_selector(trimmed)?
        .ok_or_else(|| auth::account_selector_not_found(trimmed))?;
    account_lookup_address(&account)
}

fn account_lookup_address(account: &Account) -> Result<Address, anyhow::Error> {
    let lookup_address = account
        .master_address
        .as_deref()
        .unwrap_or(&account.address);
    lookup_address.parse::<Address>().map_err(|_| {
        CliError::Internal(anyhow::anyhow!(
            "stored account '{}' has invalid lookup address {}",
            account.alias,
            lookup_address
        ))
        .into()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZeroAddressPolicy {
    Allow,
    Reject,
}

fn parse_explicit_address(
    value: &str,
    zero_policy: ZeroAddressPolicy,
) -> Result<Address, CliError> {
    let trimmed = value.trim();
    let stripped = trimmed.strip_prefix("0x").ok_or_else(|| {
        CliError::Configuration("address must be a 0x-prefixed 40-byte hex string".to_string())
    })?;
    if stripped.len() != 40 || !stripped.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(CliError::Configuration(
            "address must be a 0x-prefixed 40-byte hex string".to_string(),
        ));
    }
    if zero_policy == ZeroAddressPolicy::Reject && stripped.chars().all(|ch| ch == '0') {
        return Err(CliError::Configuration(
            "address must not be the zero address".to_string(),
        ));
    }
    trimmed.parse::<Address>().map_err(|_| {
        CliError::Configuration("address must be a 0x-prefixed 40-byte hex string".to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{SignerSource, parse_private_key};
    use crate::db::AgentAccountMetadata;
    use tempfile::TempDir;

    const MASTER_KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000009";
    const AGENT_KEY: &str = "0x000000000000000000000000000000000000000000000000000000000000000a";

    #[test]
    fn stored_agent_alias_resolves_to_protocol_master_for_lookup() {
        let tmp = TempDir::new().unwrap();
        let mut store =
            AccountStore::open(tmp.path().join("accounts.db"), tmp.path().join("key")).unwrap();
        let master = parse_private_key(MASTER_KEY).unwrap().address();
        let agent = parse_private_key(AGENT_KEY).unwrap().address();
        let account = store
            .add_agent_account(
                "agent",
                &agent.to_string(),
                AGENT_KEY,
                AgentAccountMetadata {
                    master_address: &master.to_string(),
                    agent_name: Some("api"),
                    expires_at: None,
                },
                false,
            )
            .unwrap();

        let resolved = account_lookup_address(&account).unwrap();

        assert_eq!(resolved, master);
        assert_ne!(resolved, agent);
    }

    #[test]
    fn signer_resolution_preserves_source_priority_and_query_address() {
        let private = parse_private_key(MASTER_KEY).unwrap().address();
        let resolved = resolve_selected_signer(SignerResolverInput {
            resolved_private_key: Some(MASTER_KEY),
            keystore_path: None,
            keystore_password: None,
            account_selector: None,
            ows_selector: Some("0x0000000000000000000000000000000000000001"),
            default_fallback: DefaultSignerFallback::Disallow,
        })
        .unwrap();

        assert_ne!(resolved.address(), private);
        assert_eq!(resolved.query_address(), resolved.address());
        assert!(matches!(resolved.source(), SignerSource::Ows { .. }));
        assert!(resolved.local_private_key().is_err());
    }

    #[test]
    fn explicit_raw_destinations_do_not_resolve_aliases() {
        let err = parse_raw_destination_address("main").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("0x-prefixed"));

        let err = parse_protocol_object_address("main").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("0x-prefixed"));
    }
}
