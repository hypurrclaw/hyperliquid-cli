//! JSON error envelope.

use super::{CliError, NextCommand};

/// A JSON-serializable error envelope.
///
/// In JSON mode, errors are rendered to stdout with stable top-level fields so
/// agents can route failures without parsing prose. `error`, `category`, and
/// `exit_code` are the stable core; `reason` is a stable snake_case code and
/// the remaining keys are present only when they apply.
#[derive(serde::Serialize)]
pub struct ErrorEnvelope {
    pub error: String,
    pub category: &'static str,
    pub exit_code: i32,
    pub reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<NextCommand>,
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    pub data: serde_json::Map<String, serde_json::Value>,
}

impl ErrorEnvelope {
    /// Create from a [`CliError`], using its Display representation as the headline.
    pub fn from_cli_error(err: &CliError) -> Self {
        let mut data = serde_json::Map::new();
        if let Some(suggestion) = err.suggestion() {
            data.insert("suggestion".into(), suggestion.into());
        }
        if let CliError::ApiStatus { status, remote, .. } = err {
            data.insert("http_status".into(), (*status).into());
            if let Some(remote) = remote {
                data.insert("remote_message".into(), remote.to_string().into());
            }
        }
        let next = err
            .recovery()
            .map(|recovery| recovery.commands)
            .unwrap_or_default();
        Self {
            error: err.to_string(),
            category: err.category(),
            exit_code: err.exit_code(),
            reason: err.reason(),
            retryable: err.retryable(),
            retry_after: err.retry_after(),
            next,
            data,
        }
    }

    /// Create from raw clap usage-error text.
    ///
    /// `error` keeps the headline (and clap's tip line, if any). The `Usage:`
    /// block and the `For more information` line move to `data.usage`; a
    /// "similar ... exists" tip also fills `data.suggestion`; a `See all
    /// options:` pointer becomes the `next` help command.
    pub fn usage(message: impl Into<String>) -> Self {
        let raw = message.into();
        let parsed = parse_clap_text(&raw);
        let mut data = serde_json::Map::new();
        if let Some(usage) = parsed.usage {
            data.insert("usage".into(), usage.into());
        }
        if let Some(suggestion) = parsed.suggestion {
            data.insert("suggestion".into(), suggestion.into());
        }
        let reason = if parsed.headline.contains("unrecognized subcommand") {
            "unknown_command"
        } else {
            "invalid_arguments"
        };
        Self {
            error: parsed.headline,
            category: "usage",
            exit_code: 2,
            reason,
            retryable: None,
            retry_after: None,
            next: parsed
                .help_command
                .map(|command| vec![NextCommand::new(command)])
                .unwrap_or_default(),
            data,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"error":"serialization failed","category":"internal","exit_code":1,"reason":"internal_error"}"#.to_string()
        })
    }
}

struct ClapText {
    headline: String,
    usage: Option<String>,
    suggestion: Option<String>,
    help_command: Option<String>,
}

fn parse_clap_text(raw: &str) -> ClapText {
    let text = raw.trim();
    let text = text.strip_prefix("error: ").unwrap_or(text);
    let mut kept = Vec::new();
    let mut usage = Vec::new();
    let mut suggestion = None;
    let mut help_command = None;
    for paragraph in text.split("\n\n") {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        if paragraph.starts_with("Usage:") || paragraph.starts_with("For more information") {
            usage.push(paragraph);
            continue;
        }
        if let Some(command) = paragraph.strip_prefix("See all options:") {
            help_command = Some(command.trim().to_string());
            continue;
        }
        if suggestion.is_none()
            && let Some(tip) = paragraph.strip_prefix("tip: a similar")
        {
            suggestion = tip.split('\'').nth(1).map(str::to_string);
        }
        kept.push(paragraph);
    }
    let headline = if kept.is_empty() {
        text.to_string()
    } else {
        kept.join("\n")
    };
    ClapText {
        headline,
        usage: (!usage.is_empty()).then(|| usage.join("\n\n")),
        suggestion,
        help_command,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response_sanitization::Untrusted;

    fn envelope_value(err: &CliError) -> serde_json::Value {
        serde_json::from_str(&ErrorEnvelope::from_cli_error(err).to_json()).unwrap()
    }

    #[test]
    fn minimal_envelope_has_only_the_four_core_keys() {
        let err = CliError::Internal(anyhow::anyhow!("something went wrong"));
        let json = ErrorEnvelope::from_cli_error(&err).to_json();
        assert_eq!(
            json,
            r#"{"error":"something went wrong","category":"internal","exit_code":1,"reason":"internal_error"}"#
        );
    }

    #[test]
    fn envelope_from_cli_error_carries_message() {
        let json = ErrorEnvelope::from_cli_error(&CliError::rate_limited()).to_json();
        assert!(json.contains("\"error\""));
        assert!(json.contains("Too many requests"));
    }

    #[test]
    fn envelope_reasons_are_stable_per_variant() {
        let cases: Vec<(CliError, &str)> = vec![
            (CliError::Internal(anyhow::anyhow!("x")), "internal_error"),
            (
                CliError::ExchangeRejected {
                    context: "order rejected",
                    remote: Untrusted::from_remote("x"),
                },
                "exchange_rejected",
            ),
            (CliError::Configuration("x".into()), "invalid_config"),
            (CliError::MissingInput("x".into()), "missing_input"),
            (CliError::AuthRequired, "auth_required"),
            (CliError::InvalidAuth("x".into()), "invalid_auth"),
            (CliError::rate_limited(), "rate_limited"),
            (CliError::Unavailable("x".into()), "network_error"),
            (
                CliError::api_status(reqwest::StatusCode::BAD_GATEWAY, ""),
                "api_error",
            ),
            (CliError::Timeout("x".into()), "timeout"),
            (CliError::unknown_outcome("x"), "unknown_outcome"),
            (CliError::Unsupported("x".into()), "unsupported_input"),
            (
                CliError::AssetNotFoundNoSuggestion { asset: "X".into() },
                "asset_not_found",
            ),
            (CliError::PartialResults("x".into()), "partial_failure"),
        ];
        for (err, reason) in cases {
            assert_eq!(envelope_value(&err)["reason"], reason, "{err:?}");
        }
    }

    #[test]
    fn envelope_rate_limit_carries_retry_fields() {
        let read = envelope_value(&CliError::RateLimited {
            retry_after: Some(7),
            retryable: true,
        });
        assert_eq!(read["retry_after"], 7);
        assert_eq!(read["retryable"], true);
        assert_eq!(read["exit_code"], 11);

        let mutation = envelope_value(&CliError::rate_limited());
        assert!(mutation.get("retry_after").is_none());
        assert!(mutation.get("retryable").is_none());
    }

    #[test]
    fn envelope_unknown_outcome_has_next_commands_and_not_retryable() {
        let value = envelope_value(&CliError::unknown_outcome(
            "The request timed out after 10s",
        ));
        assert_eq!(value["exit_code"], 12);
        assert_eq!(value["category"], "unavailable");
        assert_eq!(value["reason"], "unknown_outcome");
        assert_eq!(value["retryable"], false);
        assert_eq!(
            value["error"],
            "Can't confirm whether the exchange applied this action. The request timed out after 10s."
        );
        assert_eq!(
            value["next"],
            serde_json::json!([
                {"command": "hyperliquid orders open"},
                {"command": "hyperliquid account fills"}
            ])
        );
    }

    #[test]
    fn envelope_auth_required_points_at_setup() {
        let value = envelope_value(&CliError::AuthRequired);
        assert_eq!(value["next"][0]["command"], "hyperliquid setup");
        assert_eq!(value["exit_code"], 10);
    }

    #[test]
    fn envelope_confirmation_required_offers_preview_then_approval() {
        let value = envelope_value(&CliError::ConfirmationRequired {
            headline: "Can't run `orders create` without confirmation.".into(),
            preview: Some("hyperliquid orders create --dry-run".into()),
            approve: Some("hyperliquid orders create --yes".into()),
        });
        assert_eq!(value["reason"], "confirmation_required");
        assert_eq!(value["exit_code"], 13);
        assert_eq!(value["category"], "unsupported");
        assert_eq!(
            value["next"],
            serde_json::json!([
                {"command": "hyperliquid orders create --dry-run"},
                {
                    "command": "hyperliquid orders create --yes",
                    "when": "Only after a human approves"
                }
            ])
        );
    }

    #[test]
    fn envelope_asset_not_found_puts_suggestion_in_data_not_next() {
        let value = envelope_value(&CliError::AssetNotFound {
            asset: "BTCC".into(),
            suggestions: "BTC".into(),
        });
        assert_eq!(value["reason"], "asset_not_found");
        assert_eq!(value["data"]["suggestion"], "BTC");
        assert!(value.get("next").is_none());
        assert_eq!(
            value["error"],
            "Couldn't find asset \"BTCC\". Did you mean BTC?"
        );
    }

    #[test]
    fn envelope_api_status_keeps_status_out_of_the_headline() {
        let err = CliError::api_status(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            "\u{1b}[31mmaintenance\u{1b}[0m",
        );
        let value = envelope_value(&err);
        assert_eq!(
            value["error"],
            "The Hyperliquid API failed to process the request."
        );
        assert_eq!(value["reason"], "api_error");
        assert_eq!(value["data"]["http_status"], 503);
        assert_eq!(
            value["data"]["remote_message"],
            "[untrusted remote data] maintenance"
        );
        assert_eq!(value["exit_code"], 12);
    }

    #[test]
    fn envelope_usage_moves_usage_block_into_data() {
        let raw = "error: unexpected argument '--bogus' found\n\n  tip: a similar argument exists: '--format'\n\nUsage: hyperliquid wallet show [OPTIONS]\n\nFor more information, try '--help'.\n";
        let value: serde_json::Value =
            serde_json::from_str(&ErrorEnvelope::usage(raw).to_json()).unwrap();

        assert_eq!(
            value["error"],
            "unexpected argument '--bogus' found\ntip: a similar argument exists: '--format'"
        );
        assert_eq!(value["category"], "usage");
        assert_eq!(value["exit_code"], 2);
        assert_eq!(value["reason"], "invalid_arguments");
        assert_eq!(
            value["data"]["usage"],
            "Usage: hyperliquid wallet show [OPTIONS]\n\nFor more information, try '--help'."
        );
        assert_eq!(value["data"]["suggestion"], "--format");
    }

    #[test]
    fn envelope_usage_unknown_subcommand_reason() {
        let raw = "error: unrecognized subcommand 'ordres'\n\n  tip: a similar subcommand exists: 'orders'\n\nUsage: hyperliquid [OPTIONS] <COMMAND>\n\nFor more information, try '--help'.\n";
        let value: serde_json::Value =
            serde_json::from_str(&ErrorEnvelope::usage(raw).to_json()).unwrap();

        assert_eq!(value["reason"], "unknown_command");
        assert_eq!(value["data"]["suggestion"], "orders");
        assert!(!value["error"].as_str().unwrap().starts_with("error:"));
    }
}
