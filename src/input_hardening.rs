//! Shared validation for agent-supplied identifiers and artifact paths.

use std::fs;
use std::io;
use std::path::{Component, Path};
use std::sync::LazyLock;

use regex_lite::Regex;

use crate::errors::CliError;

const DEFAULT_MAX_JSON_FILE_BYTES: u64 = 1024 * 1024;
const MAX_JSON_DEPTH: usize = 64;
const MAX_JSON_KEYS: usize = 4096;
const MAX_JSON_STRING_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct FilePolicy {
    label: &'static str,
    allow_stdin: bool,
    max_bytes: u64,
}

impl FilePolicy {
    #[must_use]
    pub const fn payload() -> Self {
        Self {
            label: "payload",
            allow_stdin: true,
            max_bytes: DEFAULT_MAX_JSON_FILE_BYTES,
        }
    }

    #[must_use]
    pub const fn json_artifact(label: &'static str) -> Self {
        Self {
            label,
            allow_stdin: false,
            max_bytes: DEFAULT_MAX_JSON_FILE_BYTES,
        }
    }
}

pub fn validate_resource_id(label: &str, value: &str) -> Result<(), CliError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(CliError::Configuration(format!("{label} cannot be empty")));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(CliError::Configuration(format!(
            "{label} cannot contain control characters"
        )));
    }
    if trimmed.contains('?') || trimmed.contains('#') {
        return Err(CliError::Configuration(format!(
            "{label} cannot contain embedded query or fragment markers"
        )));
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("%2e") || lower.contains("%2f") || lower.contains("%5c") {
        return Err(CliError::Configuration(format!(
            "{label} cannot contain percent-encoded path traversal"
        )));
    }
    if trimmed.contains("../") || trimmed.contains("..\\") {
        return Err(CliError::Configuration(format!(
            "{label} cannot contain path traversal"
        )));
    }
    Ok(())
}

pub fn validate_input_path(path: &str) -> Result<(), CliError> {
    validate_file_path(path, FilePolicy::payload())
}

pub fn validate_file_path(path: &str, policy: FilePolicy) -> Result<(), CliError> {
    if path == "-" {
        if policy.allow_stdin {
            return Ok(());
        }
        return Err(CliError::Configuration(format!(
            "{} path cannot read from stdin",
            policy.label
        )));
    }
    validate_resource_id(&format!("{} path", policy.label), path)?;
    let path = Path::new(path);
    if path.is_absolute() {
        return Err(CliError::Configuration(format!(
            "{} path must be relative to the current working directory",
            policy.label
        )));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(CliError::Configuration(format!(
            "{} path cannot contain parent directory traversal",
            policy.label
        )));
    }
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(CliError::Configuration(format!(
                "{} path cannot be a symlink",
                policy.label
            )));
        }
        if metadata.len() > policy.max_bytes {
            return Err(CliError::Configuration(format!(
                "{} exceeds {} byte limit",
                file_subject(policy.label),
                policy.max_bytes
            )));
        }
    }
    Ok(())
}

pub fn read_json_file(path: &Path, policy: FilePolicy) -> Result<serde_json::Value, CliError> {
    let path_string = path.to_string_lossy();
    validate_file_path(&path_string, policy)?;
    let text = if path_string == "-" {
        read_stdin_line(policy)?
    } else {
        let bytes = fs::read(path).map_err(|err| {
            CliError::Configuration(format!(
                "failed to read {}: {err}",
                file_subject(policy.label)
            ))
        })?;
        if bytes.len() as u64 > policy.max_bytes {
            return Err(CliError::Configuration(format!(
                "{} exceeds {} byte limit",
                file_subject(policy.label),
                policy.max_bytes
            )));
        }
        String::from_utf8(bytes).map_err(|_| {
            CliError::Configuration(format!(
                "{} must be valid UTF-8 JSON",
                file_subject(policy.label)
            ))
        })?
    };
    parse_json_text(&text, policy.label)
}

pub fn parse_json_text(raw: &str, label: &str) -> Result<serde_json::Value, CliError> {
    reject_terminal_control_content(raw, label)?;
    let value = serde_json::from_str(raw)
        .map_err(|err| CliError::Configuration(format!("invalid {label} JSON: {err}")))?;
    validate_json_limits(&value, label)?;
    Ok(value)
}

pub fn redact_sensitive_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .into_iter()
                .map(|(key, value)| {
                    if is_sensitive_key(&key) {
                        (key, serde_json::json!("[redacted]"))
                    } else {
                        (key, redact_sensitive_json(value))
                    }
                })
                .collect(),
        ),
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(redact_sensitive_json).collect())
        }
        serde_json::Value::String(value) if value.to_ascii_lowercase().starts_with("bearer ") => {
            serde_json::json!("[redacted]")
        }
        other => other,
    }
}

fn read_stdin_line(policy: FilePolicy) -> Result<String, CliError> {
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    if input.len() as u64 > policy.max_bytes {
        return Err(CliError::Configuration(format!(
            "{} input exceeds {} byte limit",
            policy.label, policy.max_bytes
        )));
    }
    Ok(input)
}

fn file_subject(label: &str) -> String {
    if label.ends_with("file") || label.ends_with("artifact") {
        label.to_string()
    } else {
        format!("{label} file")
    }
}

fn reject_terminal_control_content(raw: &str, label: &str) -> Result<(), CliError> {
    if raw
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(CliError::Configuration(format!(
            "{label} JSON cannot contain terminal control characters"
        )));
    }
    Ok(())
}

fn validate_json_limits(value: &serde_json::Value, label: &str) -> Result<(), CliError> {
    let mut key_count = 0;
    validate_json_limits_inner(value, label, 0, &mut key_count)
}

fn validate_json_limits_inner(
    value: &serde_json::Value,
    label: &str,
    depth: usize,
    key_count: &mut usize,
) -> Result<(), CliError> {
    if depth > MAX_JSON_DEPTH {
        return Err(CliError::Configuration(format!(
            "{label} JSON exceeds maximum nesting depth"
        )));
    }
    match value {
        serde_json::Value::Object(object) => {
            *key_count += object.len();
            if *key_count > MAX_JSON_KEYS {
                return Err(CliError::Configuration(format!(
                    "{label} JSON contains too many object keys"
                )));
            }
            for (key, value) in object {
                if key.len() > MAX_JSON_STRING_BYTES {
                    return Err(CliError::Configuration(format!(
                        "{label} JSON contains an oversized object key"
                    )));
                }
                validate_json_limits_inner(value, label, depth + 1, key_count)?;
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                validate_json_limits_inner(value, label, depth + 1, key_count)?;
            }
        }
        serde_json::Value::String(value) => {
            if value
                .chars()
                .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
            {
                return Err(CliError::Configuration(format!(
                    "{label} JSON cannot contain terminal control characters"
                )));
            }
            if value.len() > MAX_JSON_STRING_BYTES {
                return Err(CliError::Configuration(format!(
                    "{label} JSON contains an oversized string"
                )));
            }
        }
        _ => {}
    }
    Ok(())
}

static PRIVATE_KEY_HEX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b0x[0-9a-f]{64}\b").expect("private-key redaction regex must compile")
});
static BEARER_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(bearer\s+)[^\s,;"'\}\]]+"#)
        .expect("bearer-token redaction regex must compile")
});
static SECRET_FLAG_VALUE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(--(?:private-key|keystore-password|payload-json)(?:=|\s+))(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s,;\}\]]+)"#,
    )
    .expect("secret flag redaction regex must compile")
});
static SECRET_LABEL_VALUE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b((?:private[_-]?key|keystore[_-]?password|password|passphrase|mnemonic|seed|secret|api[_-]?key|access[_-]?token|refresh[_-]?token)\s*[:=]\s*)(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s,;\}\]]+)"#,
    )
    .expect("secret label redaction regex must compile")
});

pub fn redact_sensitive_text(text: &str) -> String {
    let redacted = SECRET_FLAG_VALUE_RE.replace_all(text, "$1[redacted]");
    let redacted = SECRET_LABEL_VALUE_RE.replace_all(&redacted, "$1[redacted]");
    let redacted = PRIVATE_KEY_HEX_RE.replace_all(&redacted, "[redacted]");
    BEARER_TOKEN_RE
        .replace_all(&redacted, "$1[redacted]")
        .into_owned()
}

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    let compact = lower
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();

    const BROAD_NEEDLES: &[&str] = &[
        "privatekey",
        "secret",
        "password",
        "passphrase",
        "mnemonic",
        "seed",
        "credential",
        "credentials",
        "apikey",
        "authorization",
        "bearer",
    ];
    const EXACT_KEYS: &[&str] = &[
        "accesstoken",
        "refreshtoken",
        "idtoken",
        "bearertoken",
        "authtoken",
        "sessiontoken",
        "csrftoken",
        "xcsrftoken",
        "jwttoken",
        "apitoken",
        "oauthtoken",
        "personalaccesstoken",
        "signature",
    ];

    BROAD_NEEDLES.iter().any(|needle| compact.contains(needle))
        || EXACT_KEYS.contains(&compact.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_agent_hazard_identifiers() {
        for value in ["../x", "x%2ey", "coin?x=1", "coin#frag", "bad\ncoin"] {
            assert!(validate_resource_id("id", value).is_err(), "{value}");
        }
    }

    #[test]
    fn accepts_plain_identifiers_and_stdin_path() {
        validate_resource_id("id", "BTC").unwrap();
        validate_input_path("-").unwrap();
        validate_input_path("payloads/order.json").unwrap();
    }

    #[test]
    fn file_policy_rejects_stdin_when_not_allowed() {
        let err = validate_file_path("-", FilePolicy::json_artifact("orders file")).unwrap_err();
        assert!(err.to_string().contains("stdin"));
    }

    #[test]
    fn redacts_recursive_secret_keys_and_bearer_values() {
        let value = serde_json::json!({
            "api_key": "abc",
            "nested": {
                "access_token": "secret-token",
                "token": "USDC",
                "signature": "0xsig",
                "safe": "Bearer token",
                "next_token": "pagination-token",
                "token_id": "erc20-token-id",
                "transaction_signature": "public-chain-signature"
            }
        });
        let redacted = redact_sensitive_json(value);
        assert_eq!(redacted["api_key"], "[redacted]");
        assert_eq!(redacted["nested"]["access_token"], "[redacted]");
        assert_eq!(redacted["nested"]["token"], "USDC");
        assert_eq!(redacted["nested"]["signature"], "[redacted]");
        assert_eq!(redacted["nested"]["safe"], "[redacted]");
        assert_eq!(redacted["nested"]["next_token"], "pagination-token");
        assert_eq!(redacted["nested"]["token_id"], "erc20-token-id");
        assert_eq!(
            redacted["nested"]["transaction_signature"],
            "public-chain-signature"
        );
    }

    #[test]
    fn redacts_sensitive_patterns_from_plain_text() {
        let private_key = format!("0x{}", "a".repeat(64));
        let text = format!(
            "error --keystore-password hunter2 passphrase='open sesame' Authorization: Bearer abc.def private_key={private_key} token_id=123"
        );

        let redacted = redact_sensitive_text(&text);

        assert!(!redacted.contains("hunter2"));
        assert!(!redacted.contains("open sesame"));
        assert!(!redacted.contains("abc.def"));
        assert!(!redacted.contains(&private_key));
        assert!(redacted.contains("--keystore-password [redacted]"));
        assert!(redacted.contains("passphrase=[redacted]"));
        assert!(redacted.contains("Bearer [redacted]"));
        assert!(redacted.contains("private_key=[redacted]"));
        assert!(redacted.contains("token_id=123"));
    }

    #[test]
    fn parse_json_text_rejects_escaped_terminal_controls_and_oversized_strings() {
        let control_err = parse_json_text(r#"{"note":"bad\u001b"}"#, "payload").unwrap_err();
        assert!(
            control_err
                .to_string()
                .contains("terminal control characters")
        );

        let oversized = format!(r#"{{"note":"{}"}}"#, "x".repeat(MAX_JSON_STRING_BYTES + 1));
        let oversized_err = parse_json_text(&oversized, "payload").unwrap_err();
        assert!(oversized_err.to_string().contains("oversized string"));
    }
}
