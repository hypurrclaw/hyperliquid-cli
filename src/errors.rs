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
//! | 1    | `ExchangeRejected`  | Exchange rejected the action         |
//! | 2    | `Configuration`     | Invalid config or network setting    |
//! | 2    | `MissingInput`      | Required input missing, can't prompt |
//! | 2    | (clap usage)        | Invalid arguments (handled by clap)  |
//! | 10   | `AuthRequired`      | Missing wallet / private key         |
//! | 10   | `InvalidAuth`       | Bad key format / expired session     |
//! | 11   | `RateLimited*`      | API rate-limit response              |
//! | 12   | `Unavailable`       | Network / API unreachable            |
//! | 12   | `ApiStatus`         | API answered with a failure status   |
//! | 12   | `Timeout`           | Operation exceeded a requested bound |
//! | 12   | `UnknownOutcome`    | Mutation sent, result unknown        |
//! | 13   | `Unsupported`       | Invalid asset, bad parameter         |
//! | 13   | `AssetNotFound`     | Unknown asset (one close match)      |
//! | 14   | `StaleData`         | Cached data expired                  |
//! | 15   | `PartialResults`    | Some items failed in a batch         |

use std::time::Duration;

use thiserror::Error;

use crate::response_sanitization::{labelled_untrusted_text, sanitize_untrusted_text};

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

    /// The exchange answered and rejected the action (exit 1).
    ///
    /// The message is already labelled when it carries remote text.
    #[error("{0}")]
    ExchangeRejected(String),

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

    /// OWS wallet not found in vault (exit 10).
    #[error("OWS wallet '{wallet}' was not found")]
    OwsWalletNotFound { wallet: String },

    /// OWS wallet has no Hyperliquid or EVM account for signer resolution (exit 10).
    #[error("OWS wallet '{wallet}' has no Hyperliquid ({caip2}) or EVM (eip155:1) account")]
    OwsNoChainAccount { wallet: String, caip2: String },

    /// Hyperliquid API returned a rate-limit response, retry timing unknown (exit 11).
    #[error("Too many requests to the Hyperliquid API. Try again shortly.")]
    RateLimited,

    /// Hyperliquid API returned a rate-limit response (exit 11).
    ///
    /// `retry_after` is the `Retry-After` header in seconds when present.
    /// `retryable` is true only for reads, which are safe to resend.
    #[error("Too many requests to the Hyperliquid API. {}", rate_limit_hint(.retry_after))]
    RateLimitedAfter {
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
        /// Remote response text, already sanitized and labelled.
        remote: Option<String>,
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

    /// Cached data is stale (exit 14).
    #[error("Stale data: {0}")]
    StaleData(String),

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
    if status >= 500 {
        "The Hyperliquid API failed to process the request."
    } else {
        "The Hyperliquid API rejected the request."
    }
}

// ── Exit code mapping ───────────────────────────────────────────────────

impl CliError {
    /// Map error variant to the corresponding exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Internal(_) | Self::ExchangeRejected(_) => 1,
            Self::Configuration(_) | Self::MissingInput(_) => 2,
            Self::AuthRequired
            | Self::InvalidAuth(_)
            | Self::OwsWalletNotFound { .. }
            | Self::OwsNoChainAccount { .. } => 10,
            Self::RateLimited | Self::RateLimitedAfter { .. } => 11,
            Self::Unavailable(_)
            | Self::ApiStatus { .. }
            | Self::Timeout(_)
            | Self::UnknownOutcome(_) => 12,
            Self::Unsupported(_)
            | Self::AssetNotFound { .. }
            | Self::AssetNotFoundNoSuggestion { .. } => 13,
            Self::StaleData(_) => 14,
            Self::PartialResults(_) => 15,
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
        let remote = (!body.trim().is_empty()).then(|| labelled_untrusted_text(body));
        Self::ApiStatus {
            status: status.as_u16(),
            phrase: status.canonical_reason().unwrap_or_default().to_string(),
            remote,
        }
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
}

/// Recovery guidance rendered under an error: an instruction ending in `:`,
/// the commands indented four spaces, then an optional closing note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recovery {
    pub instruction: &'static str,
    pub commands: &'static [&'static str],
    pub note: Option<&'static str>,
}

impl CliError {
    /// Recovery commands for this error, when one is known.
    #[must_use]
    pub fn recovery(&self) -> Option<Recovery> {
        match self {
            Self::AuthRequired => Some(Recovery {
                instruction: "Set up a wallet:",
                commands: &["hyperliquid setup"],
                note: Some("Or set HYPERLIQUID_PRIVATE_KEY for this run."),
            }),
            Self::UnknownOutcome(_) => Some(Recovery {
                instruction: "Check open orders and recent fills:",
                commands: &["hyperliquid orders open", "hyperliquid account fills"],
                note: None,
            }),
            _ => None,
        }
    }

    /// Stable snake_case reason code for JSON error envelopes.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Internal(_) => "internal_error",
            Self::ExchangeRejected(_) => "exchange_rejected",
            Self::Configuration(_) => "invalid_config",
            Self::MissingInput(message) if is_confirmation_block(message) => {
                "confirmation_required"
            }
            Self::MissingInput(_) => "missing_input",
            Self::AuthRequired => "auth_required",
            Self::InvalidAuth(_) => "invalid_auth",
            Self::OwsWalletNotFound { .. } => "wallet_not_found",
            Self::OwsNoChainAccount { .. } => "wallet_account_missing",
            Self::RateLimited | Self::RateLimitedAfter { .. } => "rate_limited",
            Self::Unavailable(_) => "network_error",
            Self::ApiStatus { .. } => "api_error",
            Self::Timeout(_) => "timeout",
            Self::UnknownOutcome(_) => "unknown_outcome",
            Self::Unsupported(message) if is_confirmation_block(message) => "confirmation_required",
            Self::Unsupported(_) => "unsupported_input",
            Self::AssetNotFound { .. } | Self::AssetNotFoundNoSuggestion { .. } => {
                "asset_not_found"
            }
            Self::StaleData(_) => "stale_data",
            Self::PartialResults(_) => "partial_failure",
        }
    }

    /// Whether resending is known to be safe and useful. `None` when unknown.
    #[must_use]
    pub fn retryable(&self) -> Option<bool> {
        match self {
            Self::UnknownOutcome(_) => Some(false),
            Self::RateLimitedAfter {
                retryable: true, ..
            } => Some(true),
            Self::ApiStatus { status, .. } if *status >= 500 => Some(true),
            _ => None,
        }
    }

    /// Seconds to wait before retrying, from `Retry-After`.
    #[must_use]
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            Self::RateLimitedAfter { retry_after, .. } => *retry_after,
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

/// True when a message is a missing-confirmation block written by this CLI.
fn is_confirmation_block(message: &str) -> bool {
    message.contains("requires confirmation") || message.contains("without confirmation")
}

// ── Network error copy ──────────────────────────────────────────────────

/// Map a `reqwest` transport failure to `Couldn't reach <host>. <cause>.`
///
/// The cause comes from walking the error's `source()` chain (refused,
/// timeout, DNS, TLS, reset). An active `HTTPS_PROXY`/`HTTP_PROXY` is named.
/// `timeout` is the request timeout that was in force, when known.
#[must_use]
pub fn network_error(err: &reqwest::Error, timeout: Option<Duration>) -> CliError {
    let target = err
        .url()
        .and_then(host_label)
        .unwrap_or_else(|| "the Hyperliquid API".to_string());
    let mut message = format!(
        "Couldn't reach {target}. {}.",
        transport_cause(err, timeout)
    );
    if let Some((name, host)) = active_proxy(err.url()) {
        message.push_str(&format!(" The request used {name} ({host})."));
    }
    CliError::Unavailable(message)
}

/// Describe why a transport request failed, without a trailing period.
#[must_use]
pub fn transport_cause(err: &reqwest::Error, timeout: Option<Duration>) -> String {
    let mut chain = vec![err.to_string().to_lowercase()];
    let mut deepest = None;
    let mut source = std::error::Error::source(err);
    while let Some(inner) = source {
        deepest = Some(inner.to_string());
        chain.push(inner.to_string().to_lowercase());
        source = inner.source();
    }
    let chain = chain.join(" | ");
    let has = |needles: &[&str]| needles.iter().any(|needle| chain.contains(needle));

    if err.is_timeout() || has(&["timed out", "deadline has elapsed"]) {
        return match timeout {
            Some(limit) => format!("Timed out after {}", format_duration(limit)),
            None => "Timed out".to_string(),
        };
    }
    if has(&["connection refused"]) {
        return "Connection refused".to_string();
    }
    if has(&[
        "dns error",
        "failed to lookup address",
        "name or service not known",
        "nodename nor servname",
        "no such host",
        "name resolution",
    ]) {
        return "DNS lookup failed".to_string();
    }
    if has(&["connection reset", "broken pipe"]) {
        return "Connection reset".to_string();
    }
    if has(&["certificate", "tls", "ssl"]) {
        return "TLS handshake failed".to_string();
    }
    if has(&["network is unreachable", "no route to host"]) {
        return "Network is unreachable".to_string();
    }
    match deepest {
        Some(text) => {
            let text = sanitize_untrusted_text(&text);
            text.trim_end_matches('.').to_string()
        }
        None => "Connection failed".to_string(),
    }
}

fn format_duration(duration: Duration) -> String {
    if duration.subsec_millis() == 0 {
        format!("{}s", duration.as_secs())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn host_label(url: &reqwest::Url) -> Option<String> {
    let host = url.host_str()?;
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

/// The proxy variable that applies to this URL, with the proxy host only
/// (credentials in the proxy URL are never printed).
fn active_proxy(url: Option<&reqwest::Url>) -> Option<(&'static str, String)> {
    let https = url.is_none_or(|url| url.scheme() == "https");
    let names: &[&'static str] = if https {
        &["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
    } else {
        &["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
    };
    for name in names {
        let Ok(value) = std::env::var(name) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        let parsed = reqwest::Url::parse(value)
            .ok()
            .filter(|url| url.host_str().is_some())
            .or_else(|| reqwest::Url::parse(&format!("http://{value}")).ok());
        let host = parsed
            .as_ref()
            .and_then(host_label)
            .unwrap_or_else(|| "a proxy".to_string());
        return Some((name, host));
    }
    None
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
            hypersdk::hypercore::Error::Network(inner) => network_error(inner, None),
            hypersdk::hypercore::Error::Timeout => CliError::Unavailable(
                "Couldn't reach the Hyperliquid API. The request timed out.".to_string(),
            ),
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
                    CliError::InvalidAuth(labelled_untrusted_text(msg))
                } else {
                    CliError::Internal(anyhow::anyhow!(
                        "API error: {}",
                        labelled_untrusted_text(msg)
                    ))
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
            hypersdk::hypercore::Error::WebSocket(msg) => CliError::Unavailable(format!(
                "Couldn't keep the Hyperliquid WebSocket connection. {}",
                labelled_untrusted_text(msg)
            )),
            hypersdk::hypercore::Error::Json(_) => CliError::Internal(anyhow::anyhow!("{}", err)),
            hypersdk::hypercore::Error::Other(msg) => {
                CliError::Internal(anyhow::anyhow!("{}", labelled_untrusted_text(msg)))
            }
        }
    }
}

// ── JSON error envelope ─────────────────────────────────────────────────

/// A recovery command in the `next` list of an error envelope.
#[derive(serde::Serialize)]
pub struct NextCommand {
    pub command: String,
}

/// A JSON-serializable error envelope.
///
/// In JSON mode, errors are rendered to stdout with stable top-level fields so
/// agents can route failures without parsing prose. `error`, `category`, and
/// `exit_code` are the stable core; `reason` is a stable snake_case code and
/// the remaining keys are present only when they apply.
#[derive(serde::Serialize)]
#[allow(dead_code)]
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
    /// Create a new error envelope from a message string.
    #[allow(dead_code)]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            error: message.into(),
            category: "internal",
            exit_code: 1,
            reason: "internal_error",
            retryable: None,
            retry_after: None,
            next: Vec::new(),
            data: serde_json::Map::new(),
        }
    }

    /// Create from a [`CliError`], using its Display representation as the headline.
    pub fn from_cli_error(err: &CliError) -> Self {
        let mut data = serde_json::Map::new();
        if let Some(suggestion) = err.suggestion() {
            data.insert("suggestion".into(), suggestion.into());
        }
        if let CliError::ApiStatus { status, remote, .. } = err {
            data.insert("http_status".into(), (*status).into());
            if let Some(remote) = remote {
                data.insert("remote_message".into(), remote.as_str().into());
            }
        }
        let next = err
            .recovery()
            .map(|recovery| {
                recovery
                    .commands
                    .iter()
                    .map(|command| NextCommand {
                        command: (*command).to_string(),
                    })
                    .collect()
            })
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
    /// "similar ... exists" tip also fills `data.suggestion`.
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
            next: Vec::new(),
            data,
        }
    }

    /// Serialize to a JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"error":"serialization failed","category":"internal","exit_code":1,"reason":"internal_error"}"#.to_string()
        })
    }

    /// Pretty-print as JSON.
    #[allow(dead_code)]
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| {
            "{\n  \"error\": \"serialization failed\",\n  \"category\": \"internal\",\n  \"exit_code\": 1,\n  \"reason\": \"internal_error\"\n}"
                .to_string()
        })
    }
}

struct ClapText {
    headline: String,
    usage: Option<String>,
    suggestion: Option<String>,
}

fn parse_clap_text(raw: &str) -> ClapText {
    let text = raw.trim();
    let text = text.strip_prefix("error: ").unwrap_or(text);
    let mut kept = Vec::new();
    let mut usage = Vec::new();
    let mut suggestion = None;
    for paragraph in text.split("\n\n") {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        if paragraph.starts_with("Usage:") || paragraph.starts_with("For more information") {
            usage.push(paragraph);
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
    }
}

impl CliError {
    /// Stable high-level category for JSON error envelopes.
    pub fn category(&self) -> &'static str {
        match self {
            Self::Internal(_) | Self::ExchangeRejected(_) => "internal",
            Self::Configuration(_) => "configuration",
            Self::MissingInput(_) => "missing_input",
            Self::AuthRequired
            | Self::InvalidAuth(_)
            | Self::OwsWalletNotFound { .. }
            | Self::OwsNoChainAccount { .. } => "auth",
            Self::RateLimited | Self::RateLimitedAfter { .. } => "rate_limited",
            Self::Unavailable(_)
            | Self::ApiStatus { .. }
            | Self::Timeout(_)
            | Self::UnknownOutcome(_) => "unavailable",
            Self::Unsupported(_)
            | Self::AssetNotFound { .. }
            | Self::AssetNotFoundNoSuggestion { .. } => "unsupported",
            Self::StaleData(_) => "stale_data",
            Self::PartialResults(_) => "partial_results",
        }
    }
}

// ── Print helpers (format-aware routing) ────────────────────────────────

/// Render a human-readable error block (no trailing newline).
///
/// Rich output starts with a red `✗`; plain output starts with `Error:`.
/// Detail lines are indented two spaces. A recovery block follows after a blank
/// line: the instruction, each command indented four spaces, then any note.
#[must_use]
pub fn render_human_error(err: &CliError, rich: bool) -> String {
    use crate::output::colors;

    let headline = err.to_string();
    let mut out = if rich {
        format!("{} {headline}", colors::paint(colors::RED, "✗"))
    } else {
        format!("Error: {headline}")
    };
    if let Some(detail) = err.detail() {
        out.push_str(&format!("\n  {detail}"));
    }
    if let Some(recovery) = err.recovery() {
        out.push_str(&format!("\n\n{}", recovery.instruction));
        for command in recovery.commands {
            out.push_str(&format!("\n    {command}"));
        }
        if let Some(note) = recovery.note {
            out.push_str(&format!("\n{note}"));
        }
    }
    out
}

/// Print a CLI error with proper routing based on output format.
///
/// - JSON mode: prints the error envelope to **stdout**
/// - Pretty and table modes: print the human error block to **stderr**
///   (`✗ ...` when stderr supports styling, `Error: ...` otherwise)
///
/// [`CliError::PartialResults`] in JSON mode prints nothing: the command that
/// raised it already wrote its own single JSON document with per-item results.
pub fn print_error(err: &CliError, format: crate::output::OutputFormat) {
    match format {
        crate::output::OutputFormat::Json => {
            if matches!(err, CliError::PartialResults(_)) {
                return;
            }
            let envelope = ErrorEnvelope::from_cli_error(err);
            println!("{}", envelope.to_json());
        }
        crate::output::OutputFormat::Pretty | crate::output::OutputFormat::Table => {
            let rich = crate::output::colors::stderr_enabled();
            eprintln!("{}", render_human_error(err, rich));
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
        assert_eq!(
            CliError::AuthRequired.to_string(),
            "Authentication required."
        );
    }

    #[test]
    fn test_display_asset_not_found_with_suggestion() {
        let err = CliError::AssetNotFound {
            asset: "BT".into(),
            suggestions: "BTC".into(),
        };
        assert_eq!(
            err.to_string(),
            "Couldn't find asset \"BT\". Did you mean BTC?"
        );
    }

    #[test]
    fn test_display_asset_not_found_no_suggestion() {
        let err = CliError::AssetNotFoundNoSuggestion {
            asset: "ZZZZZZZZ".into(),
        };
        assert_eq!(err.to_string(), "Couldn't find asset \"ZZZZZZZZ\".");
    }

    #[test]
    fn test_display_unavailable_is_complete_copy() {
        let err =
            CliError::Unavailable("Couldn't reach api.hyperliquid.xyz. Connection refused.".into());
        assert_eq!(
            err.to_string(),
            "Couldn't reach api.hyperliquid.xyz. Connection refused."
        );
    }

    #[test]
    fn test_display_timeout() {
        let err = CliError::Timeout("Timed out waiting for events".into());
        assert!(err.to_string().contains("Timed out waiting"));
    }

    #[test]
    fn test_display_rate_limited() {
        assert_eq!(
            CliError::RateLimited.to_string(),
            "Too many requests to the Hyperliquid API. Try again shortly."
        );
        let err = CliError::RateLimitedAfter {
            retry_after: Some(7),
            retryable: true,
        };
        assert_eq!(
            err.to_string(),
            "Too many requests to the Hyperliquid API. Try again in 7s."
        );
    }

    #[test]
    fn test_display_copy_has_no_banned_phrasing() {
        for err in [
            CliError::RateLimited,
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

    // ── Error envelope ──────────────────────────────────────────────

    #[test]
    fn test_error_envelope_json() {
        let envelope = ErrorEnvelope::new("something went wrong");
        let json = envelope.to_json();
        assert_eq!(
            json,
            r#"{"error":"something went wrong","category":"internal","exit_code":1,"reason":"internal_error"}"#
        );
    }

    #[test]
    fn test_error_envelope_from_cli_error() {
        let err = CliError::RateLimited;
        let envelope = ErrorEnvelope::from_cli_error(&err);
        let json = envelope.to_json();
        assert!(json.contains("\"error\""));
        assert!(json.contains("Too many requests"));
    }

    #[test]
    fn test_error_envelope_parses_back() {
        let envelope = ErrorEnvelope::new("test error");
        let json = envelope.to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("should be valid JSON");
        assert_eq!(parsed["error"], "test error");
    }

    #[test]
    fn test_error_envelope_minimal_fields_omit_optionals() {
        let envelope = ErrorEnvelope::new("test");
        let json = envelope.to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = parsed.as_object().unwrap();
        assert_eq!(obj.len(), 4);
        for key in ["error", "category", "exit_code", "reason"] {
            assert!(obj.contains_key(key), "{key}");
        }
    }

    fn envelope_value(err: &CliError) -> serde_json::Value {
        serde_json::from_str(&ErrorEnvelope::from_cli_error(err).to_json()).unwrap()
    }

    #[test]
    fn envelope_reasons_are_stable_per_variant() {
        let cases: Vec<(CliError, &str)> = vec![
            (CliError::Internal(anyhow::anyhow!("x")), "internal_error"),
            (CliError::ExchangeRejected("x".into()), "exchange_rejected"),
            (CliError::Configuration("x".into()), "invalid_config"),
            (CliError::MissingInput("x".into()), "missing_input"),
            (CliError::AuthRequired, "auth_required"),
            (CliError::InvalidAuth("x".into()), "invalid_auth"),
            (CliError::RateLimited, "rate_limited"),
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
            (CliError::StaleData("x".into()), "stale_data"),
            (CliError::PartialResults("x".into()), "partial_failure"),
        ];
        for (err, reason) in cases {
            assert_eq!(envelope_value(&err)["reason"], reason, "{err:?}");
        }
    }

    #[test]
    fn envelope_marks_confirmation_blocks() {
        let err = CliError::Unsupported(
            "orders create requires confirmation in machine-readable contexts".into(),
        );
        let value = envelope_value(&err);
        assert_eq!(value["reason"], "confirmation_required");
        assert_eq!(value["exit_code"], 13);
        assert_eq!(value["category"], "unsupported");

        let err = CliError::MissingInput("Can't create order without confirmation.".into());
        assert_eq!(envelope_value(&err)["reason"], "confirmation_required");
    }

    #[test]
    fn envelope_rate_limit_carries_retry_fields() {
        let read = envelope_value(&CliError::RateLimitedAfter {
            retry_after: Some(7),
            retryable: true,
        });
        assert_eq!(read["retry_after"], 7);
        assert_eq!(read["retryable"], true);
        assert_eq!(read["exit_code"], 11);

        let mutation = envelope_value(&CliError::RateLimitedAfter {
            retry_after: None,
            retryable: false,
        });
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

    #[test]
    fn human_error_plain_uses_error_prefix() {
        let text = render_human_error(&CliError::RateLimited, false);
        assert_eq!(
            text,
            "Error: Too many requests to the Hyperliquid API. Try again shortly."
        );
    }

    #[test]
    fn human_error_rich_uses_red_glyph() {
        let text = render_human_error(&CliError::RateLimited, true);
        assert!(
            text.starts_with("\u{1b}[31m✗\u{1b}[0m Too many requests"),
            "{text}"
        );
        assert!(!text.contains("Error:"));
    }

    #[test]
    fn human_error_recovery_block_for_auth() {
        let text = render_human_error(&CliError::AuthRequired, false);
        assert_eq!(
            text,
            "Error: Authentication required.\n\nSet up a wallet:\n    hyperliquid setup\nOr set HYPERLIQUID_PRIVATE_KEY for this run."
        );
    }

    #[test]
    fn human_error_recovery_block_for_unknown_outcome() {
        let text = render_human_error(&CliError::unknown_outcome("The request timed out"), false);
        assert_eq!(
            text,
            "Error: Can't confirm whether the exchange applied this action. The request timed out.\n\nCheck open orders and recent fills:\n    hyperliquid orders open\n    hyperliquid account fills"
        );
    }

    #[test]
    fn human_error_puts_status_in_a_detail_line() {
        let err = CliError::api_status(reqwest::StatusCode::SERVICE_UNAVAILABLE, "");
        assert_eq!(
            render_human_error(&err, false),
            "Error: The Hyperliquid API failed to process the request.\n  HTTP 503 Service Unavailable"
        );
    }

    #[test]
    fn test_error_envelope_cli_error_all_variants() {
        let variants: Vec<CliError> = vec![
            CliError::Internal(anyhow::anyhow!("test")),
            CliError::ExchangeRejected("rejected".into()),
            CliError::Configuration("bad config".into()),
            CliError::MissingInput("missing".into()),
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
            CliError::RateLimitedAfter {
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
            assert_eq!(parsed["reason"], err.reason());
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
