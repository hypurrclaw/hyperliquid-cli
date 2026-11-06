//! Typed address classes for command arguments.
//!
//! These helpers make the selector boundary explicit:
//! - public user and acting-account selectors may resolve stored account aliases,
//! - protocol object and raw destination addresses are explicit addresses only.
//!
//! Signer selection lives in [`crate::auth`].

use std::fmt;

use hypersdk::Address;

use crate::auth::lookup_account_address;
use crate::errors::CliError;
use crate::mode::Mode;
use crate::settings::Settings;

macro_rules! address_class {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name(Address);

        impl $name {
            #[must_use]
            pub fn address(self) -> Address {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

address_class!(
    /// Whose account data a read command queries.
    ProtocolUserAddress
);
address_class!(
    /// The account a signed action acts on behalf of.
    ActingAccountAddress
);
address_class!(
    /// A protocol object such as a vault, validator or builder.
    ProtocolObjectAddress
);
address_class!(
    /// A transfer recipient.
    RawDestinationAddress
);

pub fn resolve_protocol_user_address(
    selector: &str,
    settings: &Settings,
    mode: &Mode,
) -> Result<ProtocolUserAddress, anyhow::Error> {
    lookup_account_address(selector, settings, mode).map(ProtocolUserAddress)
}

pub fn resolve_acting_account_address(
    selector: &str,
    settings: &Settings,
    mode: &Mode,
) -> Result<ActingAccountAddress, anyhow::Error> {
    lookup_account_address(selector, settings, mode).map(ActingAccountAddress)
}

pub fn parse_protocol_object_address(value: &str) -> Result<ProtocolObjectAddress, CliError> {
    parse_explicit_address(value, ZeroAddressPolicy::Allow).map(ProtocolObjectAddress)
}

pub fn parse_raw_destination_address(value: &str) -> Result<RawDestinationAddress, CliError> {
    parse_explicit_address(value, ZeroAddressPolicy::Reject).map(RawDestinationAddress)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZeroAddressPolicy {
    Allow,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HexAddressError {
    Format,
    Zero,
}

/// Parse `value` as a 0x-prefixed 40-hex-digit address. Does not trim.
pub fn parse_exact_hex_address(value: &str, reject_zero: bool) -> Result<Address, HexAddressError> {
    let stripped = value.strip_prefix("0x").ok_or(HexAddressError::Format)?;
    if stripped.len() != 40 || !stripped.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(HexAddressError::Format);
    }
    if reject_zero && stripped.chars().all(|ch| ch == '0') {
        return Err(HexAddressError::Zero);
    }
    value
        .parse::<Address>()
        .map_err(|_| HexAddressError::Format)
}

fn parse_explicit_address(
    value: &str,
    zero_policy: ZeroAddressPolicy,
) -> Result<Address, CliError> {
    parse_exact_hex_address(value.trim(), zero_policy == ZeroAddressPolicy::Reject).map_err(|err| {
        match err {
            HexAddressError::Zero => {
                CliError::Configuration("address must not be the zero address".to_string())
            }
            HexAddressError::Format => CliError::Configuration(
                "address must be a 0x-prefixed 40-byte hex string".to_string(),
            ),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_raw_destinations_do_not_resolve_aliases() {
        let err = parse_raw_destination_address("main").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("0x-prefixed"));

        let err = parse_protocol_object_address("main").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("0x-prefixed"));
    }

    #[test]
    fn zero_address_is_a_valid_object_but_not_a_destination() {
        let zero = "0x0000000000000000000000000000000000000000";
        assert!(parse_protocol_object_address(zero).is_ok());
        assert!(parse_raw_destination_address(zero).is_err());
    }

    #[test]
    fn literal_addresses_resolve_without_touching_the_vault() {
        let settings =
            Settings::detached(crate::settings::EnvSource::fixed::<&str, &str>([]), None);
        let address = "0x0000000000000000000000000000000000000001";
        let user =
            resolve_protocol_user_address(&format!(" {address} "), &settings, &Mode::default())
                .unwrap();
        assert_eq!(user.to_string().to_lowercase(), address);

        let err =
            resolve_acting_account_address("0xnothex", &settings, &Mode::default()).unwrap_err();
        assert!(err.to_string().contains("Invalid address"));
    }
}
