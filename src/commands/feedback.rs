//! Structured feedback submission command.

use std::io::Read;
use std::time::{Duration, Instant};

use clap::{ArgGroup, Args};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::errors::{self, CliError};
use crate::output::{self, OutputFormat, TableData};
use crate::response_sanitization::labelled_untrusted_text;

const BUILD_ENV_FEEDBACK_URL: &str = "HYPERLIQUID_FEEDBACK_URL";
const MAX_SCENARIO_BYTES: usize = 16 * 1024;
const MAX_TAGS: usize = 10;
const MAX_TAG_BYTES: usize = 64;
const MAX_CONTACT_BYTES: usize = 256;

#[derive(Args, Debug, Clone)]
#[command(group(
    ArgGroup::new("scenario")
        .required(true)
        .args(["scenario_json", "scenario_file"])
))]
pub struct FeedbackArgs {
    /// Structured scenario JSON object to submit. May include agent_address, signer_address, or wallet_address for feedback rate-limit attribution.
    #[arg(
        long = "scenario-json",
        value_name = "JSON",
        conflicts_with = "scenario_file"
    )]
    pub scenario_json: Option<String>,

    /// Path to a structured scenario JSON object, or '-' to read from stdin. May include agent_address, signer_address, or wallet_address.
    #[arg(
        long = "scenario-file",
        value_name = "PATH|-",
        conflicts_with = "scenario_json"
    )]
    pub scenario_file: Option<String>,

    /// Optional contact handle or email to include with the feedback.
    #[arg(long, value_name = "CONTACT")]
    pub contact: Option<String>,

    /// Optional comma-separated labels such as bug,docs,agent.
    #[arg(long, value_name = "TAG", value_delimiter = ',')]
    pub tags: Vec<String>,

    /// Feedback API endpoint. Defaults to runtime or build-time HYPERLIQUID_FEEDBACK_URL.
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct FeedbackRequest<'a> {
    source: &'static str,
    version: &'static str,
    scenario: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    contact: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct FeedbackResponse {
    status: Option<String>,
    id: Option<String>,
    message: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedbackOutput {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl TableData for FeedbackOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Status", "Id", "Message"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.status.clone(),
            self.id.clone().unwrap_or_default(),
            self.message.clone().unwrap_or_default(),
        ]]
    }

    fn to_json_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

pub async fn submit(args: &FeedbackArgs, format: OutputFormat) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    let endpoint = feedback_endpoint(args)?;
    let scenario = read_scenario(args)?;
    let contact = validate_contact(args.contact.as_deref())?;
    let tags = validate_tags(&args.tags)?;

    let request = FeedbackRequest {
        source: "hyperliquid-cli",
        version: env!("CARGO_PKG_VERSION"),
        scenario,
        contact: contact.as_deref(),
        tags,
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|err| CliError::Unavailable(format!("feedback client setup failed: {err}")))?;

    let response = client
        .post(endpoint)
        .header(reqwest::header::USER_AGENT, feedback_user_agent())
        .json(&request)
        .send()
        .await
        .map_err(|err| CliError::Unavailable(format!("feedback submission failed: {err}")))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| CliError::Unavailable(format!("failed to read feedback response: {err}")))?;

    if !status.is_success() {
        if errors::http_response_indicates_rate_limit(status.as_u16(), &body) {
            return Err(CliError::RateLimited.into());
        }
        return Err(CliError::Unavailable(format!(
            "feedback service returned HTTP {status}. {}",
            labelled_untrusted_text(&body)
        ))
        .into());
    }

    let output = feedback_output_from_body(&body)?;
    output::print_data(&output, format, start.elapsed());
    Ok(())
}

fn feedback_endpoint(args: &FeedbackArgs) -> Result<String, CliError> {
    let endpoint = args
        .url
        .clone()
        .or_else(runtime_feedback_endpoint)
        .or_else(compiled_feedback_endpoint)
        .ok_or_else(|| {
            CliError::Configuration(format!(
                "feedback endpoint is not configured; pass --url or set {BUILD_ENV_FEEDBACK_URL} at runtime or build time"
            ))
        })?;

    let parsed = reqwest::Url::parse(&endpoint).map_err(|err| {
        CliError::Configuration(format!("feedback endpoint must be an absolute URL: {err}"))
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(CliError::Configuration(
            "feedback endpoint must use http or https".to_string(),
        ));
    }
    Ok(endpoint)
}

fn runtime_feedback_endpoint() -> Option<String> {
    std::env::var(BUILD_ENV_FEEDBACK_URL)
        .ok()
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
}

fn compiled_feedback_endpoint() -> Option<String> {
    option_env!("HYPERLIQUID_BUILD_FEEDBACK_URL")
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
}

fn read_scenario(args: &FeedbackArgs) -> Result<Value, CliError> {
    let raw = match (&args.scenario_json, &args.scenario_file) {
        (Some(json), None) => json.clone(),
        (None, Some(path)) if path == "-" => read_limited_scenario_json(
            std::io::stdin(),
            "stdin",
            "failed to read scenario JSON from stdin",
        )?,
        (None, Some(path)) => {
            let file = std::fs::File::open(path).map_err(|err| {
                CliError::Unsupported(format!("failed to open scenario JSON file {path}: {err}"))
            })?;
            read_limited_scenario_json(
                file,
                path,
                &format!("failed to read scenario JSON from {path}"),
            )?
        }
        _ => {
            return Err(CliError::Unsupported(
                "provide exactly one of --scenario-json or --scenario-file".to_string(),
            ));
        }
    };

    if raw.len() > MAX_SCENARIO_BYTES {
        return Err(CliError::Unsupported(format!(
            "scenario JSON must be at most {MAX_SCENARIO_BYTES} bytes"
        )));
    }

    let scenario = serde_json::from_str::<Value>(&raw)
        .map_err(|err| CliError::Unsupported(format!("scenario JSON is invalid: {err}")))?;
    if !scenario.is_object() {
        return Err(CliError::Unsupported(
            "scenario JSON must be an object describing the observed scenario".to_string(),
        ));
    }
    Ok(scenario)
}

fn read_limited_scenario_json(
    reader: impl Read,
    source: &str,
    error_prefix: &str,
) -> Result<String, CliError> {
    let mut input = String::new();
    reader
        .take((MAX_SCENARIO_BYTES as u64) + 1)
        .read_to_string(&mut input)
        .map_err(|err| CliError::Unsupported(format!("{error_prefix}: {err}")))?;

    if input.len() > MAX_SCENARIO_BYTES {
        return Err(CliError::Unsupported(format!(
            "scenario JSON from {source} must be at most {MAX_SCENARIO_BYTES} bytes"
        )));
    }

    Ok(input)
}

fn validate_contact(contact: Option<&str>) -> Result<Option<String>, CliError> {
    let Some(contact) = contact else {
        return Ok(None);
    };
    let normalized = contact.trim();
    if normalized.is_empty() {
        return Err(CliError::Unsupported(
            "contact must not be empty when provided".to_string(),
        ));
    }
    if normalized.len() > MAX_CONTACT_BYTES || contains_control(normalized) {
        return Err(CliError::Unsupported(format!(
            "contact must be at most {MAX_CONTACT_BYTES} bytes and contain no control characters"
        )));
    }
    Ok(Some(normalized.to_string()))
}

fn validate_tags(tags: &[String]) -> Result<Vec<String>, CliError> {
    if tags.len() > MAX_TAGS {
        return Err(CliError::Unsupported(format!(
            "at most {MAX_TAGS} feedback tags are supported"
        )));
    }

    tags.iter()
        .map(|tag| {
            let normalized = tag.trim().to_ascii_lowercase();
            if normalized.is_empty()
                || normalized.len() > MAX_TAG_BYTES
                || !normalized
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
            {
                return Err(CliError::Unsupported(format!(
                    "feedback tags must use letters, numbers, '-' or '_' and be at most {MAX_TAG_BYTES} bytes"
                )));
            }
            Ok(normalized)
        })
        .collect()
}

fn feedback_output_from_body(body: &str) -> Result<FeedbackOutput, CliError> {
    if body.trim().is_empty() {
        return Ok(FeedbackOutput {
            status: "accepted".to_string(),
            id: None,
            message: None,
        });
    }

    let response = serde_json::from_str::<FeedbackResponse>(body).map_err(|err| {
        CliError::Unavailable(format!(
            "feedback service returned invalid JSON. {} ({err})",
            labelled_untrusted_text(body)
        ))
    })?;

    Ok(FeedbackOutput {
        status: response.status.unwrap_or_else(|| "accepted".to_string()),
        id: response.id,
        message: response.message.or(response.error),
    })
}

fn feedback_user_agent() -> String {
    format!("hyperliquid-cli/{}", env!("CARGO_PKG_VERSION"))
}

fn contains_control(input: &str) -> bool {
    input.chars().any(|ch| ch.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_with_json(json: &str) -> FeedbackArgs {
        FeedbackArgs {
            scenario_json: Some(json.to_string()),
            scenario_file: None,
            contact: None,
            tags: Vec::new(),
            url: Some("http://127.0.0.1:8787/feedback".to_string()),
        }
    }

    #[test]
    fn accepts_structured_scenario_object() {
        let scenario = read_scenario(&args_with_json(
            r#"{"command":"orders create","expected":"dry-run preview","actual":"error"}"#,
        ))
        .unwrap();

        assert_eq!(scenario["command"], "orders create");
    }

    #[test]
    fn rejects_non_object_scenario_json() {
        let err = read_scenario(&args_with_json(r#"["not", "an", "object"]"#)).unwrap_err();

        assert!(
            matches!(err, CliError::Unsupported(message) if message.contains("must be an object"))
        );
    }

    #[test]
    fn normalizes_tags() {
        let tags = validate_tags(&["Bug".into(), "agent-ux".into()]).unwrap();

        assert_eq!(tags, vec!["bug", "agent-ux"]);
    }

    #[test]
    fn parses_success_response() {
        let output = feedback_output_from_body(r#"{"status":"accepted","id":"fb_123"}"#).unwrap();

        assert_eq!(output.status, "accepted");
        assert_eq!(output.id.as_deref(), Some("fb_123"));
    }

    #[test]
    fn parses_worker_error_field_as_message() {
        let output =
            feedback_output_from_body(r#"{"status":"error","error":"rate_limited"}"#).unwrap();

        assert_eq!(output.status, "error");
        assert_eq!(output.message.as_deref(), Some("rate_limited"));
    }

    #[test]
    fn feedback_endpoint_uses_runtime_env_when_url_arg_absent() {
        let _guard = env_guard();
        unsafe {
            std::env::set_var(
                BUILD_ENV_FEEDBACK_URL,
                "https://example.invalid/runtime-feedback",
            );
        }
        let mut args = args_with_json(r#"{"command":"mids"}"#);
        args.url = None;

        let endpoint = feedback_endpoint(&args).unwrap();

        assert_eq!(endpoint, "https://example.invalid/runtime-feedback");
    }

    #[test]
    fn feedback_endpoint_prefers_url_arg_over_runtime_env() {
        let _guard = env_guard();
        unsafe {
            std::env::set_var(
                BUILD_ENV_FEEDBACK_URL,
                "https://example.invalid/runtime-feedback",
            );
        }
        let mut args = args_with_json(r#"{"command":"mids"}"#);
        args.url = Some("https://example.invalid/explicit-feedback".to_string());

        let endpoint = feedback_endpoint(&args).unwrap();

        assert_eq!(endpoint, "https://example.invalid/explicit-feedback");
    }
}

#[cfg(test)]
fn env_guard() -> impl Drop {
    use std::sync::Mutex;
    use std::sync::OnceLock;
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let mutex = LOCK.get_or_init(|| Mutex::new(()));
    let guard = mutex.lock().unwrap();
    struct EnvRestore {
        _guard: std::sync::MutexGuard<'static, ()>,
    }
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            unsafe {
                std::env::remove_var(BUILD_ENV_FEEDBACK_URL);
            }
        }
    }
    EnvRestore { _guard: guard }
}
