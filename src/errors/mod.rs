//! Structured error system for Hyperliquid CLI.
//!
//! [`CliError`] is the one error type commands return. [`CliError::spec`] is
//! the single place that gives each variant its exit code, category, reason
//! and retryability; everything else (JSON envelope, human rendering, process
//! exit) reads from it.
//!
//! - `classify`: turns remote text and SDK errors into `CliError`s.
//! - `transport`: names `reqwest` transport failures.
//! - `recovery`: commands to run next.
//! - `envelope`: the JSON error document.
//! - `render`: human output and process exit.
//!
//! # Exit Codes
//!
//! | Code | Variant             | Meaning                              |
//! |------|---------------------|--------------------------------------|
//! | 0    | (success)           | Command completed successfully       |
//! | 1    | `Internal`          | Unexpected internal error            |
//! | 1    | `ExchangeRejected`  | Exchange rejected the action         |
//! | 2    | `Configuration`     | Invalid config or network setting    |
//! | 2    | `MissingInput`      | Required input missing, can't prompt |
//! | 2    | (clap usage)        | Invalid arguments (handled by clap)  |
//! | 10   | `AuthRequired`      | Missing wallet / private key         |
//! | 10   | `InvalidAuth`       | Bad key format / expired session     |
//! | 10   | `AuthRejected`      | Remote side rejected the credentials |
//! | 10   | `OwsWalletNotFound` | OWS wallet missing from the vault    |
//! | 10   | `OwsNoChainAccount` | OWS wallet has no usable account     |
//! | 11   | `RateLimited`       | API rate-limit response              |
//! | 12   | `Unavailable`       | Network / API unreachable            |
//! | 12   | `ApiStatus`         | API answered with a failure status   |
//! | 12   | `Timeout`           | Operation exceeded a requested bound |
//! | 12   | `UnknownOutcome`    | Mutation sent, result unknown        |
//! | 13   | `Unsupported`       | Invalid asset, bad parameter         |
//! | 13   | `ConfirmationRequired` | Confirmation needed, can't prompt |
//! | 13   | `AssetNotFound*`    | Unknown asset                        |
//! | 15   | `PartialResults`    | Some items failed in a batch         |

mod classify;
mod envelope;
mod recovery;
mod render;
mod transport;

use thiserror::Error;

use crate::response_sanitization::Untrusted;

pub use classify::{
    from_anyhow, from_exchange_rejection, from_sdk, http_response_indicates_rate_limit,
    is_server_status,
};
pub use envelope::ErrorEnvelope;
pub use recovery::{NextCommand, Recovery};
pub use render::{
    ErrorOutput, exit_with_error, print_human_error, print_json_error, render_human_error,
};
pub use transport::{network_error, transport_cause};

/// CLI error types mapped to structured exit codes.
///
/// Use [`CliError::exit_code`] to get the numeric code and the `Display`
/// implementation for the message.
#[derive(Error, Debug)]
pub enum CliError {
    /// Unexpected internal error (exit 1).
    ///
    /// Wraps anyhow errors from any part of the application. Build one that
    /// quotes remote text with [`CliError::internal_with_remote`].
    #[error("{0}")]
    Internal(#[from] anyhow::Error),

    /// The exchange answered and rejected the action (exit 1).
    #[error("{context}: {remote}")]
    ExchangeRejected {
        context: &'static str,
        remote: Untrusted,
    },

    /// A run that can't prompt reached a required confirmation (exit 13).
    ///
    /// `preview` is a runnable `--dry-run` command; `approve` is the same
    /// command with `--yes`, offered only for use after a human approves.
    #[error("{headline}")]
    ConfirmationRequired {
        headline: String,
        preview: Option<String>,
        approve: Option<String>,
    },

    /// Invalid CLI configuration, config file, or environment setting (exit 2).
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Required input is missing and this invocation can't prompt for it (exit 2).
    #[error("{0}")]
    MissingInput(String),

    /// No wallet or private key configured (exit 10).
    #[error("Authentication required.")]
    AuthRequired,

    /// Invalid key format or expired credentials (exit 10).
    #[error("Invalid authentication: {0}")]
    InvalidAuth(String),

    /// A remote service rejected the credentials (exit 10).
    #[error("Invalid authentication: {0}")]
    AuthRejected(Untrusted),

    /// OWS wallet not found in vault (exit 10).
    #[error("OWS wallet '{wallet}' was not found")]
    OwsWalletNotFound { wallet: String },

    /// OWS wallet has no Hyperliquid or EVM account for signer resolution (exit 10).
    #[error("OWS wallet '{wallet}' has no Hyperliquid ({caip2}) or EVM (eip155:1) account")]
    OwsNoChainAccount { wallet: String, caip2: String },

    /// Hyperliquid API returned a rate-limit response (exit 11).
    ///
    /// `retry_after` is the `Retry-After` header in seconds when present.
    /// `retryable` is true only for reads, which are safe to resend.
    /// Build the no-information form with [`CliError::rate_limited`].
    #[error("Too many requests to the Hyperliquid API. {}", rate_limit_hint(.retry_after))]
    RateLimited {
        retry_after: Option<u64>,
        retryable: bool,
    },

    /// Network or API unreachable. The message is complete user-facing copy (exit 12).
    #[error("{0}")]
    Unavailable(String),

    /// The API answered with a failure status (exit 12).
    ///
    /// The server was reached, so this is not a connectivity problem. The
    /// status goes in [`CliError::detail`], not in the headline.
    #[error("{}", api_status_headline(*.status))]
    ApiStatus {
        status: u16,
        /// Canonical reason phrase, for the detail line.
        phrase: String,
        remote: Option<Untrusted>,
    },

    /// Operation exceeded a requested timeout (exit 12).
    #[error("{0}")]
    Timeout(String),

    /// A mutation request was sent but its result is unknown (exit 12).
    ///
    /// Construct with [`CliError::unknown_outcome`].
    #[error("Can't confirm whether the exchange applied this action. {0}")]
    UnknownOutcome(String),

    /// Unsupported / invalid input (exit 13).
    #[error("Unsupported input: {0}")]
    Unsupported(String),

    /// Asset not found with a close-match suggestion (exit 13).
    #[error("Couldn't find asset \"{asset}\". Did you mean {suggestions}?")]
    AssetNotFound {
        /// The asset name the user typed.
        asset: String,
        /// The single closest match.
        suggestions: String,
    },

    /// Asset not found with no close suggestions (exit 13).
    #[error("Couldn't find asset \"{asset}\".")]
    AssetNotFoundNoSuggestion {
        /// The asset name the user typed.
        asset: String,
    },

    /// Partial success in a batch operation (exit 15).
    #[error("Partial results: {0}")]
    PartialResults(String),
}

fn rate_limit_hint(retry_after: &Option<u64>) -> String {
    match retry_after {
        Some(seconds) => format!("Try again in {seconds}s."),
        None => "Try again shortly.".to_string(),
    }
}

fn api_status_headline(status: u16) -> &'static str {
    if classify::is_server_status(status) {
        "The Hyperliquid API failed to process the request."
    } else {
        "The Hyperliquid API rejected the request."
    }
}

/// What a variant reports to callers and machine consumers.
struct Spec {
    exit_code: i32,
    category: &'static str,
    reason: &'static str,
    retryable: Option<bool>,
}

impl Spec {
    const fn new(exit_code: i32, category: &'static str, reason: &'static str) -> Self {
        Self {
            exit_code,
            category,
            reason,
            retryable: None,
        }
    }

    const fn retryable(mut self, retryable: Option<bool>) -> Self {
        self.retryable = retryable;
        self
    }
}

impl CliError {
    /// The one table of per-variant exit code, category, reason and retryability.
    fn spec(&self) -> Spec {
        match self {
            Self::Internal(_) => Spec::new(1, "internal", "internal_error"),
            Self::ExchangeRejected { .. } => Spec::new(1, "internal", "exchange_rejected"),
            Self::Configuration(_) => Spec::new(2, "configuration", "invalid_config"),
            Self::MissingInput(_) => Spec::new(2, "missing_input", "missing_input"),
            Self::AuthRequired => Spec::new(10, "auth", "auth_required"),
            Self::InvalidAuth(_) | Self::AuthRejected(_) => Spec::new(10, "auth", "invalid_auth"),
            Self::OwsWalletNotFound { .. } => Spec::new(10, "auth", "wallet_not_found"),
            Self::OwsNoChainAccount { .. } => Spec::new(10, "auth", "wallet_account_missing"),
            Self::RateLimited { retryable, .. } => {
                Spec::new(11, "rate_limited", "rate_limited").retryable(retryable.then_some(true))
            }
            Self::Unavailable(_) => Spec::new(12, "unavailable", "network_error"),
            Self::ApiStatus { status, .. } => Spec::new(12, "unavailable", "api_error")
                .retryable(classify::is_server_status(*status).then_some(true)),
            Self::Timeout(_) => Spec::new(12, "unavailable", "timeout"),
            Self::UnknownOutcome(_) => {
                Spec::new(12, "unavailable", "unknown_outcome").retryable(Some(false))
            }
            Self::Unsupported(_) => Spec::new(13, "unsupported", "unsupported_input"),
            Self::ConfirmationRequired { .. } => {
                Spec::new(13, "unsupported", "confirmation_required")
            }
            Self::AssetNotFound { .. } | Self::AssetNotFoundNoSuggestion { .. } => {
                Spec::new(13, "unsupported", "asset_not_found")
            }
            Self::PartialResults(_) => Spec::new(15, "partial_results", "partial_failure"),
        }
    }

    /// Process exit code.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        self.spec().exit_code
    }

    /// Stable high-level category for JSON error envelopes.
    #[must_use]
    pub fn category(&self) -> &'static str {
        self.spec().category
    }

    /// Stable snake_case reason code for JSON error envelopes.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        self.spec().reason
    }

    /// Whether resending is known to be safe and useful. `None` when unknown.
    #[must_use]
    pub fn retryable(&self) -> Option<bool> {
        self.spec().retryable
    }

    /// Seconds to wait before retrying, from `Retry-After`.
    #[must_use]
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            Self::RateLimited { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// A rate-limit response with no retry information.
    #[must_use]
    pub fn rate_limited() -> Self {
        Self::RateLimited {
            retry_after: None,
            retryable: false,
        }
    }

    /// Build an [`CliError::UnknownOutcome`] from a cause such as `The request timed out after 10s`.
    #[must_use]
    pub fn unknown_outcome(cause: impl AsRef<str>) -> Self {
        let cause = cause.as_ref().trim();
        let cause = if cause.ends_with('.') {
            cause.to_string()
        } else {
            format!("{cause}.")
        };
        Self::UnknownOutcome(cause)
    }

    /// Build an [`CliError::ApiStatus`] from an HTTP status and response body.
    #[must_use]
    pub fn api_status(status: reqwest::StatusCode, body: &str) -> Self {
        let remote = (!body.trim().is_empty()).then(|| Untrusted::from_remote(body));
        Self::ApiStatus {
            status: status.as_u16(),
            phrase: status.canonical_reason().unwrap_or_default().to_string(),
            remote,
        }
    }

    /// Build an [`CliError::Internal`] whose message ends with remote text.
    #[must_use]
    pub fn internal_with_remote(prefix: impl std::fmt::Display, remote: &Untrusted) -> Self {
        Self::Internal(anyhow::anyhow!("{prefix}{remote}"))
    }

    /// Secondary detail line shown under the headline in human output.
    #[must_use]
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::ApiStatus { status, phrase, .. } => Some(if phrase.is_empty() {
                format!("HTTP {status}")
            } else {
                format!("HTTP {status} {phrase}")
            }),
            _ => None,
        }
    }

    /// The single close-match suggestion carried by an asset-not-found error.
    #[must_use]
    pub fn suggestion(&self) -> Option<&str> {
        match self {
            Self::AssetNotFound { suggestions, .. } => suggestions
                .split(", ")
                .next()
                .filter(|name| !name.is_empty()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_variant() -> Vec<CliError> {
        vec![
            CliError::Internal(anyhow::anyhow!("test")),
            CliError::ExchangeRejected {
                context: "order rejected",
                remote: Untrusted::from_remote("rejected"),
            },
            CliError::ConfirmationRequired {
                headline: "needs confirmation".into(),
                preview: None,
                approve: None,
            },
            CliError::Configuration("bad config".into()),
            CliError::MissingInput("missing".into()),
            CliError::AuthRequired,
            CliError::InvalidAuth("bad key".into()),
            CliError::AuthRejected(Untrusted::from_remote("Unauthorized")),
            CliError::OwsWalletNotFound {
                wallet: "missing-wallet".into(),
            },
            CliError::OwsNoChainAccount {
                wallet: "wallet-without-account".into(),
                caip2: "hl:testnet".into(),
            },
            CliError::rate_limited(),
            CliError::RateLimited {
                retry_after: Some(3),
                retryable: true,
            },
            CliError::Unavailable("timeout".into()),
            CliError::api_status(reqwest::StatusCode::BAD_GATEWAY, "body"),
            CliError::Timeout("deadline exceeded".into()),
            CliError::unknown_outcome("cause"),
            CliError::Unsupported("bad".into()),
            CliError::AssetNotFound {
                asset: "BT".into(),
                suggestions: "BTC".into(),
            },
            CliError::AssetNotFoundNoSuggestion { asset: "ZZ".into() },
            CliError::PartialResults("half".into()),
        ]
    }

    #[test]
    fn spec_table_gives_each_variant_its_exit_code_category_and_reason() {
        let expected = [
            (1, "internal", "internal_error"),
            (1, "internal", "exchange_rejected"),
            (13, "unsupported", "confirmation_required"),
            (2, "configuration", "invalid_config"),
            (2, "missing_input", "missing_input"),
            (10, "auth", "auth_required"),
            (10, "auth", "invalid_auth"),
            (10, "auth", "invalid_auth"),
            (10, "auth", "wallet_not_found"),
            (10, "auth", "wallet_account_missing"),
            (11, "rate_limited", "rate_limited"),
            (11, "rate_limited", "rate_limited"),
            (12, "unavailable", "network_error"),
            (12, "unavailable", "api_error"),
            (12, "unavailable", "timeout"),
            (12, "unavailable", "unknown_outcome"),
            (13, "unsupported", "unsupported_input"),
            (13, "unsupported", "asset_not_found"),
            (13, "unsupported", "asset_not_found"),
            (15, "partial_results", "partial_failure"),
        ];
        let variants = every_variant();
        assert_eq!(variants.len(), expected.len());
        for (err, (exit_code, category, reason)) in variants.iter().zip(expected) {
            assert_eq!(err.exit_code(), exit_code, "{err:?}");
            assert_eq!(err.category(), category, "{err:?}");
            assert_eq!(err.reason(), reason, "{err:?}");
        }
    }

    #[test]
    fn retryability_follows_the_variant_not_a_second_table() {
        assert_eq!(CliError::unknown_outcome("x").retryable(), Some(false));
        assert_eq!(CliError::rate_limited().retryable(), None);
        assert_eq!(
            CliError::RateLimited {
                retry_after: Some(7),
                retryable: true
            }
            .retryable(),
            Some(true)
        );
        assert_eq!(
            CliError::api_status(reqwest::StatusCode::BAD_GATEWAY, "").retryable(),
            Some(true)
        );
        assert_eq!(
            CliError::api_status(reqwest::StatusCode::BAD_REQUEST, "").retryable(),
            None
        );
        assert_eq!(CliError::AuthRequired.retryable(), None);
    }

    #[test]
    fn display_internal_and_configuration() {
        let err = CliError::Internal(anyhow::anyhow!("db connection failed"));
        assert!(err.to_string().contains("db connection failed"));
        let err = CliError::Configuration("invalid HYPERLIQUID_NETWORK".into());
        assert!(err.to_string().contains("Configuration error"));
        assert!(err.to_string().contains("HYPERLIQUID_NETWORK"));
    }

    #[test]
    fn display_auth_required() {
        assert_eq!(
            CliError::AuthRequired.to_string(),
            "Authentication required."
        );
    }

    #[test]
    fn display_asset_not_found() {
        let err = CliError::AssetNotFound {
            asset: "BT".into(),
            suggestions: "BTC".into(),
        };
        assert_eq!(
            err.to_string(),
            "Couldn't find asset \"BT\". Did you mean BTC?"
        );
        let err = CliError::AssetNotFoundNoSuggestion {
            asset: "ZZZZZZZZ".into(),
        };
        assert_eq!(err.to_string(), "Couldn't find asset \"ZZZZZZZZ\".");
    }

    #[test]
    fn display_unavailable_is_complete_copy() {
        let err =
            CliError::Unavailable("Couldn't reach api.hyperliquid.xyz. Connection refused.".into());
        assert_eq!(
            err.to_string(),
            "Couldn't reach api.hyperliquid.xyz. Connection refused."
        );
    }

    #[test]
    fn display_rate_limited() {
        assert_eq!(
            CliError::rate_limited().to_string(),
            "Too many requests to the Hyperliquid API. Try again shortly."
        );
        let err = CliError::RateLimited {
            retry_after: Some(7),
            retryable: true,
        };
        assert_eq!(
            err.to_string(),
            "Too many requests to the Hyperliquid API. Try again in 7s."
        );
    }

    #[test]
    fn display_remote_text_is_labelled() {
        let rejected = CliError::ExchangeRejected {
            context: "order rejected",
            remote: Untrusted::from_remote("Order must have minimum value of $10."),
        };
        assert_eq!(
            rejected.to_string(),
            "order rejected: [untrusted remote data] Order must have minimum value of $10."
        );
        let auth = CliError::AuthRejected(Untrusted::from_remote("Unauthorized"));
        assert_eq!(
            auth.to_string(),
            "Invalid authentication: [untrusted remote data] Unauthorized"
        );
    }

    #[test]
    fn display_copy_has_no_banned_phrasing() {
        for err in [
            CliError::rate_limited(),
            CliError::AuthRequired,
            CliError::unknown_outcome("The request timed out"),
            CliError::api_status(reqwest::StatusCode::SERVICE_UNAVAILABLE, "down"),
        ] {
            let text = err.to_string();
            for banned in ["Unable to", "Please", "successfully", "Oops"] {
                assert!(!text.contains(banned), "{text}");
            }
        }
    }

    #[test]
    fn anyhow_converts_into_internal() {
        let err = CliError::from(anyhow::anyhow!("something unexpected"));
        assert_eq!(err.exit_code(), 1);
        assert!(err.to_string().contains("something unexpected"));
    }
}
