//! Structured error system for Hyperliquid CLI.
//!
//! Provides `CliError` enum with thiserror-derived error messages, exit code mapping,
//! hypersdk error conversion, and helpers for JSON/pretty error output.
//!
//! # Exit Codes
//!
//! | Code | Variant             | Meaning                              |
//! |------|---------------------|--------------------------------------|
//! | 0    | (success)           | Command completed successfully       |
//! | 1    | `Internal`          | Unexpected internal error            |
//! | 2    | `Configuration`     | Invalid config or network setting    |
//! | 2    | (clap usage)        | Invalid arguments (handled by clap)  |
//! | 10   | `AuthRequired`      | Missing wallet / private key         |
//! | 10   | `InvalidAuth`       | Bad key format / expired session     |
//! | 11   | `RateLimited`       | API rate-limit response              |
//! | 12   | `Unavailable`       | Network / API unreachable            |
//! | 12   | `Timeout`           | Operation exceeded a requested bound |
//! | 13   | `Unsupported`       | Invalid asset, bad parameter         |
//! | 13   | `AssetNotFound`     | Unknown asset (with suggestions)     |
//! | 14   | `StaleData`         | Cached data expired                  |
//! | 15   | `PartialResults`    | Some items failed in a batch         |

use thiserror::Error;

// ── CliError enum ───────────────────────────────────────────────────────

/// CLI error types mapped to structured exit codes.
///
/// Each variant maps to a specific exit code (see module docs).
/// Use [`CliError::exit_code`] to get the numeric code and
/// [`CliError::user_message`] for the display string.
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum CliError {
    /// Unexpected internal error (exit 1).
    ///
    /// Wraps anyhow errors from any part of the application.
    #[error("{0}")]
    Internal(#[from] anyhow::Error),

    /// Invalid CLI configuration, config file, or environment setting (exit 2).
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// No wallet or private key configured (exit 10).
    #[error("Authentication required. Run `hyperliquid setup` to configure your wallet.")]
    AuthRequired,

    /// Invalid key format or expired credentials (exit 10).
    #[error("Invalid authentication: {0}")]
    InvalidAuth(String),

    /// OWS wallet not found in vault (exit 10).
    #[error("OWS wallet '{wallet}' was not found")]
    OwsWalletNotFound { wallet: String },

    /// OWS wallet has no Hyperliquid or EVM account for signer resolution (exit 10).
    #[error("OWS wallet '{wallet}' has no Hyperliquid ({caip2}) or EVM (eip155:1) account")]
    OwsNoChainAccount { wallet: String, caip2: String },

    /// Hyperliquid API returned a rate-limit response (exit 11).
    #[error("Rate limited by Hyperliquid API. Please wait and retry.")]
    RateLimited,

    /// Network or API unreachable (exit 12).
    #[error("Unable to reach Hyperliquid API. {0}")]
    Unavailable(String),

    /// Operation exceeded a requested timeout (exit 12).
    #[error("{0}")]
    Timeout(String),

    /// Unsupported / invalid input (exit 13).
    #[error("Unsupported input: {0}")]
    Unsupported(String),

    /// Asset not found with fuzzy-match suggestions (exit 13).
    #[error("\"{asset}\" not found. Did you mean: {suggestions}?")]
    AssetNotFound {
        /// The asset name the user typed.
        asset: String,
        /// Comma-separated list of close matches.
        suggestions: String,
    },

    /// Asset not found with no close suggestions (exit 13).
    #[error("\"{asset}\" not found.")]
    AssetNotFoundNoSuggestion {
        /// The asset name the user typed.
        asset: String,
    },

    /// Cached data is stale (exit 14).
    #[error("Stale data: {0}")]
    StaleData(String),

    /// Partial success in a batch operation (exit 15).
    #[error("Partial results: {0}")]
    PartialResults(String),
}

// ── Exit code mapping ───────────────────────────────────────────────────

impl CliError {
    /// Map error variant to the corresponding exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Internal(_) => 1,
            Self::Configuration(_) => 2,
            Self::AuthRequired
            | Self::InvalidAuth(_)
            | Self::OwsWalletNotFound { .. }
            | Self::OwsNoChainAccount { .. } => 10,
            Self::RateLimited => 11,
            Self::Unavailable(_) | Self::Timeout(_) => 12,
            Self::Unsupported(_)
            | Self::AssetNotFound { .. }
            | Self::AssetNotFoundNoSuggestion { .. } => 13,
            Self::StaleData(_) => 14,
            Self::PartialResults(_) => 15,
        }
    }
}

/// Convert an [`anyhow::Error`] into a [`CliError`] while preserving typed `CliError`s.
///
/// Command handlers often return `anyhow::Error` so they can use `?` with multiple error
/// sources. If the original error was already a `CliError`, this helper recovers it instead
/// of wrapping it as an internal error and losing the structured exit code.
pub fn from_anyhow(err: anyhow::Error) -> CliError {
    match err.downcast::<CliError>() {
        Ok(cli_err) => cli_err,
        Err(err) => CliError::Internal(err),
    }
}

/// Return true when an HTTP response should be classified as a rate-limit error.
///
/// Body phrase matching is intentionally limited to non-success HTTP statuses so
/// successful payloads can mention rate-limit concepts as ordinary data. Success
/// responses still count as rate-limited when they use an explicit structured
/// error envelope such as `{"status":"err","response":"rate limit exceeded"}`
/// or `{"error":"too many requests"}`.
pub fn http_response_indicates_rate_limit(status_code: u16, body: &str) -> bool {
    status_code == 429
        || structured_rate_limit_error(body)
        || (!(200..300).contains(&status_code) && looks_like_rate_limit_text(body))
}

fn structured_rate_limit_error(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };

    let explicit_error_status = value
        .get("status")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|status| {
            matches!(
                status.to_ascii_lowercase().as_str(),
                "err" | "error" | "failed" | "failure"
            )
        });

    if explicit_error_status && looks_like_rate_limit_text(&value.to_string()) {
        return true;
    }

    value
        .get("error")
        .is_some_and(value_contains_rate_limit_text)
        || value
            .get("code")
            .and_then(serde_json::Value::as_str)
            .is_some_and(code_indicates_rate_limit)
}

fn value_contains_rate_limit_text(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(text) => looks_like_rate_limit_text(text),
        serde_json::Value::Array(values) => values.iter().any(value_contains_rate_limit_text),
        serde_json::Value::Object(fields) => fields.values().any(value_contains_rate_limit_text),
        _ => false,
    }
}

fn looks_like_rate_limit_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("rate limit")
        || lower.contains("rate-limit")
        || lower.contains("too many requests")
        || lower.contains("http 429")
        || lower.contains("429 too many")
}

fn code_indicates_rate_limit(code: &str) -> bool {
    let normalized = code.trim().to_ascii_lowercase().replace('-', "_");
    matches!(
        normalized.as_str(),
        "rate_limit" | "rate_limited" | "too_many_requests" | "http_429" | "429"
    )
}

// ── hypersdk → CliError mapping ─────────────────────────────────────────

impl From<hypersdk::hypercore::Error> for CliError {
    fn from(err: hypersdk::hypercore::Error) -> Self {
        match &err {
            hypersdk::hypercore::Error::Network(_) | hypersdk::hypercore::Error::Timeout => {
                CliError::Unavailable(format!("Check your network connection. {}", err))
            }
            hypersdk::hypercore::Error::Api(msg) => {
                let lower = msg.to_lowercase();
                if lower.contains("rate limit")
                    || lower.contains("rate-limit")
                    || lower.contains("too many requests")
                    || lower.contains("http 429")
                    || lower.contains("429 too many")
                {
                    CliError::RateLimited
                } else if lower.contains("invalid key")
                    || lower.contains("invalid private key")
                    || lower.contains("unauthorized")
                {
                    CliError::InvalidAuth(msg.clone())
                } else {
                    CliError::Internal(anyhow::anyhow!("{}", err))
                }
            }
            hypersdk::hypercore::Error::Signing(_) => {
                CliError::InvalidAuth(format!("Signing failed: {}", err))
            }
            hypersdk::hypercore::Error::InvalidAddress(addr) => {
                CliError::Unsupported(format!("Invalid address: {}", addr))
            }
            hypersdk::hypercore::Error::InvalidOrder { message } => {
                CliError::Unsupported(format!("Invalid order: {}", message))
            }
            hypersdk::hypercore::Error::WebSocket(msg) => {
                CliError::Unavailable(format!("WebSocket error: {}", msg))
            }
            hypersdk::hypercore::Error::Json(_) | hypersdk::hypercore::Error::Other(_) => {
                CliError::Internal(anyhow::anyhow!("{}", err))
            }
        }
    }
}

// ── JSON error envelope ─────────────────────────────────────────────────

/// A JSON-serializable error envelope.
///
/// In JSON mode, errors are rendered to stdout with stable top-level fields so
/// agents can route failures without parsing prose.
#[derive(serde::Serialize)]
#[allow(dead_code)]
pub struct ErrorEnvelope {
    pub error: String,
    pub category: &'static str,
    pub exit_code: i32,
}

impl ErrorEnvelope {
    /// Create a new error envelope from a message string.
    #[allow(dead_code)]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            error: message.into(),
            category: "internal",
            exit_code: 1,
        }
    }

    /// Create from a [`CliError`], using its Display representation.
    pub fn from_cli_error(err: &CliError) -> Self {
        Self {
            error: err.to_string(),
            category: err.category(),
            exit_code: err.exit_code(),
        }
    }

    /// Create from a clap usage error.
    pub fn usage(message: impl Into<String>) -> Self {
        Self {
            error: message.into(),
            category: "usage",
            exit_code: 2,
        }
    }

    /// Serialize to a JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"error":"serialization failed","category":"internal","exit_code":1}"#.to_string()
        })
    }

    /// Pretty-print as JSON.
    #[allow(dead_code)]
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| {
            "{\n  \"error\": \"serialization failed\",\n  \"category\": \"internal\",\n  \"exit_code\": 1\n}"
                .to_string()
        })
    }
}

impl CliError {
    /// Stable high-level category for JSON error envelopes.
    pub fn category(&self) -> &'static str {
        match self {
            Self::Internal(_) => "internal",
            Self::Configuration(_) => "configuration",
            Self::AuthRequired
            | Self::InvalidAuth(_)
            | Self::OwsWalletNotFound { .. }
            | Self::OwsNoChainAccount { .. } => "auth",
            Self::RateLimited => "rate_limited",
            Self::Unavailable(_) | Self::Timeout(_) => "unavailable",
            Self::Unsupported(_)
            | Self::AssetNotFound { .. }
            | Self::AssetNotFoundNoSuggestion { .. } => "unsupported",
            Self::StaleData(_) => "stale_data",
            Self::PartialResults(_) => "partial_results",
        }
    }
}

// ── Print helpers (format-aware routing) ────────────────────────────────

/// Print a CLI error with proper routing based on output format.
///
/// - JSON mode: prints `{"error":"..."}` to **stdout**
/// - Pretty mode: prints a red `Error` prefix to **stderr**
/// - Table mode: prints a plain `Error` prefix to **stderr**
pub fn print_error(err: &CliError, format: crate::output::OutputFormat) {
    match format {
        crate::output::OutputFormat::Json => {
            let envelope = ErrorEnvelope::from_cli_error(err);
            println!("{}", envelope.to_json());
        }
        crate::output::OutputFormat::Pretty => {
            use crate::output::colors;
            eprintln!("{}: {}", colors::red("Error"), err);
        }
        crate::output::OutputFormat::Table => {
            eprintln!("Error: {err}");
        }
    }
}

/// Terminate the process with the correct exit code after printing the error.
pub fn exit_with_error(err: CliError, format: crate::output::OutputFormat) -> ! {
    print_error(&err, format);
    std::process::exit(err.exit_code())
}

// ── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Exit code mapping ───────────────────────────────────────────

    #[test]
    fn test_exit_code_internal() {
        let err = CliError::Internal(anyhow::anyhow!("something broke"));
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn test_exit_code_auth_required() {
        assert_eq!(CliError::AuthRequired.exit_code(), 10);
    }

    #[test]
    fn test_exit_code_invalid_auth() {
        assert_eq!(CliError::InvalidAuth("bad key".into()).exit_code(), 10);
    }

    #[test]
    fn test_exit_code_configuration() {
        assert_eq!(CliError::Configuration("bad config".into()).exit_code(), 2);
    }

    #[test]
    fn test_exit_code_rate_limited() {
        assert_eq!(CliError::RateLimited.exit_code(), 11);
    }

    #[test]
    fn http_rate_limit_detection_always_accepts_429() {
        assert!(http_response_indicates_rate_limit(
            429,
            r#"{"status":"ok","message":"ordinary success"}"#
        ));
    }

    #[test]
    fn http_rate_limit_detection_accepts_non_success_body_phrase() {
        assert!(http_response_indicates_rate_limit(
            503,
            "temporarily unavailable due to rate-limit pressure"
        ));
    }

    #[test]
    fn http_rate_limit_detection_accepts_structured_error_on_success_status() {
        assert!(http_response_indicates_rate_limit(
            200,
            r#"{"status":"err","response":"rate limit exceeded"}"#
        ));
        assert!(http_response_indicates_rate_limit(
            200,
            r#"{"error":"too many requests"}"#
        ));
    }

    #[test]
    fn http_rate_limit_detection_ignores_success_body_mentions() {
        assert!(!http_response_indicates_rate_limit(
            200,
            r#"{"status":"ok","message":"read the rate-limit docs","code":"RATE-LIMIT-DOCS"}"#
        ));
    }

    #[test]
    fn test_exit_code_unavailable() {
        assert_eq!(CliError::Unavailable("timeout".into()).exit_code(), 12);
    }

    #[test]
    fn test_exit_code_timeout() {
        assert_eq!(
            CliError::Timeout("deadline exceeded".into()).exit_code(),
            12
        );
    }

    #[test]
    fn test_exit_code_unsupported() {
        assert_eq!(CliError::Unsupported("bad param".into()).exit_code(), 13);
    }

    #[test]
    fn test_exit_code_asset_not_found() {
        let err = CliError::AssetNotFound {
            asset: "BT".into(),
            suggestions: "BTC, BLUR, BONK".into(),
        };
        assert_eq!(err.exit_code(), 13);
    }

    #[test]
    fn test_exit_code_asset_not_found_no_suggestion() {
        let err = CliError::AssetNotFoundNoSuggestion {
            asset: "ZZZZZZZZ".into(),
        };
        assert_eq!(err.exit_code(), 13);
    }

    #[test]
    fn test_exit_code_stale_data() {
        assert_eq!(CliError::StaleData("cache expired".into()).exit_code(), 14);
    }

    #[test]
    fn test_exit_code_partial_results() {
        assert_eq!(
            CliError::PartialResults("3 of 5 failed".into()).exit_code(),
            15
        );
    }

    // ── Display / thiserror messages ────────────────────────────────

    #[test]
    fn test_display_internal() {
        let err = CliError::Internal(anyhow::anyhow!("db connection failed"));
        assert!(err.to_string().contains("db connection failed"));
    }

    #[test]
    fn test_display_configuration() {
        let err = CliError::Configuration("invalid HYPERLIQUID_NETWORK".into());
        assert!(err.to_string().contains("Configuration error"));
        assert!(err.to_string().contains("HYPERLIQUID_NETWORK"));
    }

    #[test]
    fn test_display_auth_required() {
        assert!(
            CliError::AuthRequired
                .to_string()
                .contains("hyperliquid setup")
        );
    }

    #[test]
    fn test_display_asset_not_found_with_suggestions() {
        let err = CliError::AssetNotFound {
            asset: "BT".into(),
            suggestions: "BTC, BLUR, BONK".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("\"BT\""));
        assert!(msg.contains("Did you mean"));
        assert!(msg.contains("BTC, BLUR, BONK"));
    }

    #[test]
    fn test_display_asset_not_found_no_suggestion() {
        let err = CliError::AssetNotFoundNoSuggestion {
            asset: "ZZZZZZZZ".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("\"ZZZZZZZZ\""));
        assert!(!msg.contains("Did you mean"));
    }

    #[test]
    fn test_display_unavailable() {
        let err = CliError::Unavailable("connection refused".into());
        assert!(err.to_string().contains("Unable to reach"));
        assert!(err.to_string().contains("connection refused"));
    }

    #[test]
    fn test_display_timeout() {
        let err = CliError::Timeout("Timed out waiting for events".into());
        assert!(err.to_string().contains("Timed out waiting"));
        assert!(!err.to_string().contains("Unable to reach"));
    }

    #[test]
    fn test_display_rate_limited() {
        assert!(CliError::RateLimited.to_string().contains("Rate limited"));
    }

    // ── Error envelope ──────────────────────────────────────────────

    #[test]
    fn test_error_envelope_json() {
        let envelope = ErrorEnvelope::new("something went wrong");
        let json = envelope.to_json();
        assert_eq!(
            json,
            r#"{"error":"something went wrong","category":"internal","exit_code":1}"#
        );
    }

    #[test]
    fn test_error_envelope_from_cli_error() {
        let err = CliError::RateLimited;
        let envelope = ErrorEnvelope::from_cli_error(&err);
        let json = envelope.to_json();
        assert!(json.contains("\"error\""));
        assert!(json.contains("Rate limited"));
    }

    #[test]
    fn test_error_envelope_parses_back() {
        let envelope = ErrorEnvelope::new("test error");
        let json = envelope.to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("should be valid JSON");
        assert_eq!(parsed["error"], "test error");
    }

    #[test]
    fn test_error_envelope_contains_stable_fields() {
        let envelope = ErrorEnvelope::new("test");
        let json = envelope.to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = parsed.as_object().unwrap();
        assert_eq!(obj.len(), 3);
        assert!(obj.contains_key("error"));
        assert!(obj.contains_key("category"));
        assert!(obj.contains_key("exit_code"));
    }

    #[test]
    fn test_error_envelope_cli_error_all_variants() {
        let variants: Vec<CliError> = vec![
            CliError::Internal(anyhow::anyhow!("test")),
            CliError::Configuration("bad config".into()),
            CliError::AuthRequired,
            CliError::InvalidAuth("bad key".into()),
            CliError::OwsWalletNotFound {
                wallet: "missing-wallet".into(),
            },
            CliError::OwsNoChainAccount {
                wallet: "wallet-without-account".into(),
                caip2: "hl:testnet".into(),
            },
            CliError::RateLimited,
            CliError::Unavailable("timeout".into()),
            CliError::Timeout("deadline exceeded".into()),
            CliError::Unsupported("bad".into()),
            CliError::AssetNotFound {
                asset: "BT".into(),
                suggestions: "BTC".into(),
            },
            CliError::AssetNotFoundNoSuggestion { asset: "ZZ".into() },
            CliError::StaleData("old".into()),
            CliError::PartialResults("half".into()),
        ];

        for err in &variants {
            let envelope = ErrorEnvelope::from_cli_error(err);
            let json = envelope.to_json();
            let parsed: serde_json::Value = serde_json::from_str(&json)
                .unwrap_or_else(|_| panic!("should be valid JSON for {:?}", err));
            assert!(
                parsed.get("error").is_some(),
                "missing 'error' field for {:?}",
                err
            );
            assert_eq!(parsed["category"], err.category());
            assert_eq!(parsed["exit_code"], err.exit_code());
            let error_msg = parsed["error"].as_str().unwrap();
            assert!(!error_msg.is_empty(), "empty error for {:?}", err);
        }
    }

    // ── hypersdk error mapping ──────────────────────────────────────

    #[test]
    fn test_hypersdk_network_error_maps_to_unavailable() {
        let err = CliError::from(hypersdk::hypercore::Error::Timeout);
        assert_eq!(err.exit_code(), 12);
        assert!(matches!(err, CliError::Unavailable(_)));
    }

    #[test]
    fn test_hypersdk_api_rate_limit_maps_to_rate_limited() {
        let err = CliError::from(hypersdk::hypercore::Error::Api(
            "Rate limit exceeded".into(),
        ));
        assert_eq!(err.exit_code(), 11);
        assert!(matches!(err, CliError::RateLimited));
    }

    #[test]
    fn test_hypersdk_api_too_many_requests_maps_to_rate_limited() {
        let err = CliError::from(hypersdk::hypercore::Error::Api("Too many requests".into()));
        assert_eq!(err.exit_code(), 11);
        assert!(matches!(err, CliError::RateLimited));
    }

    #[test]
    fn test_hypersdk_api_http_429_maps_to_rate_limited() {
        let err = CliError::from(hypersdk::hypercore::Error::Api(
            "HTTP 429 Too Many Requests".into(),
        ));
        assert_eq!(err.exit_code(), 11);
        assert!(matches!(err, CliError::RateLimited));
    }

    #[test]
    fn test_hypersdk_api_auth_error_maps_to_invalid_auth() {
        let err = CliError::from(hypersdk::hypercore::Error::Api(
            "Invalid key provided".into(),
        ));
        assert_eq!(err.exit_code(), 10);
        assert!(matches!(err, CliError::InvalidAuth(_)));
    }

    #[test]
    fn test_hypersdk_api_unauthorized_maps_to_invalid_auth() {
        let err = CliError::from(hypersdk::hypercore::Error::Api(
            "Unauthorized access".into(),
        ));
        assert_eq!(err.exit_code(), 10);
        assert!(matches!(err, CliError::InvalidAuth(_)));
    }

    #[test]
    fn test_hypersdk_api_generic_maps_to_internal() {
        let err = CliError::from(hypersdk::hypercore::Error::Api(
            "Insufficient margin".into(),
        ));
        assert_eq!(err.exit_code(), 1);
        assert!(matches!(err, CliError::Internal(_)));
    }

    #[test]
    fn test_hypersdk_invalid_address_maps_to_unsupported() {
        let err = CliError::from(hypersdk::hypercore::Error::InvalidAddress(
            "not a valid hex".into(),
        ));
        assert_eq!(err.exit_code(), 13);
        assert!(matches!(err, CliError::Unsupported(_)));
    }

    #[test]
    fn test_hypersdk_invalid_order_maps_to_unsupported() {
        let err = CliError::from(hypersdk::hypercore::Error::InvalidOrder {
            message: "Size below minimum".into(),
        });
        assert_eq!(err.exit_code(), 13);
        assert!(matches!(err, CliError::Unsupported(_)));
    }

    #[test]
    fn test_hypersdk_json_error_maps_to_internal() {
        // Create a json error via serde
        let bad_json: Result<serde_json::Value, _> = serde_json::from_str("{invalid}");
        let json_err = bad_json.unwrap_err();
        let err = CliError::from(hypersdk::hypercore::Error::Json(json_err));
        assert_eq!(err.exit_code(), 1);
        assert!(matches!(err, CliError::Internal(_)));
    }

    // ── anyhow conversion ───────────────────────────────────────────

    #[test]
    fn test_anyhow_into_cli_error() {
        let err = CliError::from(anyhow::anyhow!("something unexpected"));
        assert_eq!(err.exit_code(), 1);
        assert!(err.to_string().contains("something unexpected"));
    }

    #[test]
    fn test_from_anyhow_preserves_cli_error_exit_code() {
        let err: anyhow::Error = CliError::AssetNotFound {
            asset: "BT".into(),
            suggestions: "BTC, BNT, ETH".into(),
        }
        .into();

        let cli_err = from_anyhow(err);
        assert_eq!(cli_err.exit_code(), 13);
        assert!(matches!(cli_err, CliError::AssetNotFound { .. }));
    }
}
