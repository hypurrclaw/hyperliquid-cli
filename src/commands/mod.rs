//! Command modules.

pub mod account;
pub(crate) mod actions;
pub mod api_wallet;
pub mod asset;
pub mod borrowlend;
pub mod builder;
pub mod feedback;
pub mod meta;
pub mod orderbook;
pub mod orders;
pub mod outcomes;
pub mod perps;
pub mod positions;
pub mod prio;
pub mod referral;
pub mod schema;
pub mod setup;
pub mod spot;
pub(crate) mod spot_balances;
pub mod staking;
pub mod status;
pub mod subaccounts;
pub mod transfers;
pub mod vaults;
pub mod wallet;
pub mod watch_risk;

use crate::assets::AssetError;
use crate::errors::CliError;
use crate::response_sanitization::labelled_untrusted_text;

impl From<AssetError> for CliError {
    fn from(err: AssetError) -> Self {
        match err {
            AssetError::NotFound {
                asset,
                suggestion: Some(suggestion),
            } => CliError::AssetNotFound {
                asset,
                suggestions: suggestion,
            },
            AssetError::NotFound {
                asset,
                suggestion: None,
            } => CliError::AssetNotFoundNoSuggestion { asset },
            AssetError::UnsupportedOutcome { input } => {
                outcomes::unsupported_outcome_trading_error(&input)
            }
            AssetError::Load(err) => err,
        }
    }
}

pub(crate) fn map_api_error(err: anyhow::Error) -> CliError {
    let err = match err.downcast::<CliError>() {
        Ok(cli_err) => return cli_err,
        Err(err) => err,
    };
    if crate::errors::is_typed_hypersdk_error(&err) {
        return crate::errors::from_hypersdk(err);
    }

    let raw_message = err.to_string();
    if crate::errors::looks_like_rate_limit_text(&raw_message) {
        return CliError::RateLimited;
    }
    if let Some(server_err) = crate::errors::http_server_error_from_text(&raw_message) {
        return server_err;
    }
    let lower = raw_message.to_lowercase();

    let transport_text = lower.split("body=").next().unwrap_or_default();
    if let Some(cause) = transport_failure_cause(transport_text) {
        return CliError::Unavailable(format!("Couldn't reach the Hyperliquid API. {cause}."));
    }
    CliError::Internal(anyhow::anyhow!("{}", labelled_untrusted_text(&raw_message)))
}

/// Name the transport failure described by locally generated error text.
fn transport_failure_cause(lower: &str) -> Option<&'static str> {
    if lower.contains("timeout") || lower.contains("timed out") {
        Some("The request timed out")
    } else if lower.contains("dns") {
        Some("DNS lookup failed")
    } else if lower.contains("connection refused") {
        Some("Connection refused")
    } else if lower.contains("error sending request")
        || lower.contains("connection")
        || lower.contains("service unavailable")
    {
        Some("The connection failed")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_5xx_text_maps_to_api_status_not_network() {
        let err = map_api_error(anyhow::anyhow!(
            "HTTP 503 Service Unavailable body=connection pool exhausted"
        ));

        assert_eq!(err.exit_code(), 12);
        assert!(matches!(err, CliError::ApiStatus { status: 503, .. }));
        assert!(!err.to_string().contains("503"));
        assert_eq!(
            err.detail().as_deref(),
            Some("HTTP 503 Service Unavailable")
        );
    }

    #[test]
    fn typed_cli_errors_pass_through_map_api_error() {
        let err = map_api_error(anyhow::Error::new(CliError::AuthRequired));
        assert!(matches!(err, CliError::AuthRequired));
    }

    #[test]
    fn maps_metadata_request_errors_to_unavailable() {
        let err = map_api_error(anyhow::anyhow!("error sending request: dns error"));

        assert_eq!(err.exit_code(), 12);
        assert!(matches!(err, CliError::Unavailable(_)));
    }

    #[test]
    fn maps_metadata_rate_limit_errors_to_rate_limited() {
        let err = map_api_error(anyhow::anyhow!("rate limit exceeded"));

        assert_eq!(err.exit_code(), 11);
        assert!(matches!(err, CliError::RateLimited));
    }

    #[test]
    fn maps_metadata_http_429_errors_to_rate_limited() {
        let err = map_api_error(anyhow::anyhow!("HTTP 429 Too Many Requests"));

        assert_eq!(err.exit_code(), 11);
        assert!(matches!(err, CliError::RateLimited));
    }

    #[test]
    fn maps_metadata_internal_errors_with_untrusted_label() {
        let err = map_api_error(anyhow::anyhow!("HTTP 418 body=<script>"));

        assert_eq!(err.exit_code(), 1);
        assert!(err.to_string().contains("[untrusted remote data]"));
        assert!(err.to_string().contains("HTTP 418"));
    }

    #[test]
    fn asset_errors_convert_to_their_cli_errors() {
        let not_found = CliError::from(AssetError::NotFound {
            asset: "BTCC".to_string(),
            suggestion: Some("BTC".to_string()),
        });
        assert_eq!(not_found.exit_code(), 13);
        assert_eq!(
            not_found.to_string(),
            "Couldn't find asset \"BTCC\". Did you mean BTC?"
        );
        assert_eq!(not_found.suggestion(), Some("BTC"));

        let bare = CliError::from(AssetError::NotFound {
            asset: "ZZZ".to_string(),
            suggestion: None,
        });
        assert!(matches!(bare, CliError::AssetNotFoundNoSuggestion { .. }));

        let outcome = CliError::from(AssetError::UnsupportedOutcome {
            input: "#10".to_string(),
        });
        assert_eq!(outcome.exit_code(), 13);
        assert!(outcome.to_string().contains("outcome-aware command path"));
        assert!(outcome.to_string().contains("outcomes get #10"));

        let load = CliError::from(AssetError::Load(CliError::AuthRequired));
        assert!(matches!(load, CliError::AuthRequired));
    }
}
