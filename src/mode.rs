//! Invocation mode resolved once per process.
//!
//! Commands ask this module whether they may prompt and whether agent or CI
//! environments are active. They never read `HYPERLIQUID_AGENT`, `CI`, or TTY
//! state for interaction decisions themselves, so every command agrees.

use std::sync::OnceLock;

use crate::errors::CliError;

/// Inputs that only the entry point knows after argument parsing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModeInputs {
    /// `--non-interactive` was passed.
    pub non_interactive_flag: bool,
    /// The effective output format is JSON (explicit, env, agent, or non-TTY default).
    pub machine_format: bool,
    /// Stdin carries a payload (`--payload-file -`), so it can't answer prompts.
    pub stdin_is_payload: bool,
}

static INPUTS: OnceLock<ModeInputs> = OnceLock::new();

/// Record the parsed invocation inputs. Later calls are ignored.
pub fn init(inputs: ModeInputs) {
    let _ = INPUTS.set(inputs);
}

fn inputs() -> ModeInputs {
    INPUTS.get().copied().unwrap_or_default()
}

const TRUE_VALUES: [&str; 4] = ["1", "true", "yes", "on"];
const FALSE_VALUES: [&str; 4] = ["0", "false", "no", "off"];

/// Parse a boolean environment variable strictly.
///
/// Unset or empty means `None`. Any value outside `1|true|yes|on|0|false|no|off`
/// (case-insensitive) is a configuration error naming the variable, so a typo
/// never silently falls back to the default.
pub fn env_bool(name: &str) -> Result<Option<bool>, CliError> {
    let Ok(raw) = std::env::var(name) else {
        return Ok(None);
    };
    parse_bool_value(name, &raw)
}

fn parse_bool_value(name: &str, raw: &str) -> Result<Option<bool>, CliError> {
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Ok(None);
    }
    if TRUE_VALUES.contains(&value.as_str()) {
        return Ok(Some(true));
    }
    if FALSE_VALUES.contains(&value.as_str()) {
        return Ok(Some(false));
    }
    Err(CliError::Configuration(format!(
        "{name} must be one of 1, true, yes, on, 0, false, no, off"
    )))
}

/// Validate every boolean `HYPERLIQUID_*` mode variable once at startup.
pub fn validate_env() -> Result<(), CliError> {
    env_bool("HYPERLIQUID_AGENT")?;
    env_bool("HYPERLIQUID_NO_UPDATE_CHECK")?;
    Ok(())
}

/// `HYPERLIQUID_AGENT` is set to a true value.
pub fn agent_env() -> bool {
    env_bool("HYPERLIQUID_AGENT")
        .ok()
        .flatten()
        .unwrap_or(false)
}

/// The process runs under CI (`CI` set to anything except empty, `0`, or `false`).
pub fn ci_env() -> bool {
    std::env::var("CI").is_ok_and(|value| {
        let value = value.trim().to_ascii_lowercase();
        !value.is_empty() && value != "0" && value != "false"
    })
}

/// Whether this invocation may ask the user a question.
///
/// Prompts are off for `--non-interactive`, JSON output, a detected agent, CI,
/// and when stdin is a payload. Piped stdin may still answer prompts in human
/// mode; end-of-input is treated as missing input by [`crate::prompt`].
pub fn can_prompt() -> bool {
    let inputs = inputs();
    !inputs.non_interactive_flag
        && !inputs.machine_format
        && !inputs.stdin_is_payload
        && !agent_env()
        && !ci_env()
}

/// Why prompts are off, for error copy. `None` when prompts are allowed.
pub fn prompt_blocker() -> Option<&'static str> {
    let inputs = inputs();
    if inputs.non_interactive_flag {
        Some("--non-interactive")
    } else if inputs.stdin_is_payload {
        Some("stdin is used as the payload")
    } else if inputs.machine_format {
        Some("JSON output")
    } else if agent_env() {
        Some("HYPERLIQUID_AGENT")
    } else if ci_env() {
        Some("CI")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_bool_accepts_known_values_and_treats_empty_as_unset() {
        assert_eq!(parse_bool_value("X", "TRUE").unwrap(), Some(true));
        assert_eq!(parse_bool_value("X", " off ").unwrap(), Some(false));
        assert_eq!(parse_bool_value("X", "").unwrap(), None);
    }

    #[test]
    fn strict_bool_rejects_unknown_values_by_name() {
        let err = parse_bool_value("HYPERLIQUID_AGENT", "banana").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("HYPERLIQUID_AGENT"));
    }
}
