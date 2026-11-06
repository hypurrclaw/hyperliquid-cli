//! Invocation mode resolved once per process.
//!
//! Commands ask this module whether they may prompt and whether agent or CI
//! environments are active. They never read `HYPERLIQUID_AGENT`, `CI`, or TTY
//! state for interaction decisions themselves, so every command agrees.

use std::sync::OnceLock;

use crate::errors::CliError;
use crate::output::terminal::{EnvFlag, agent_env, ci_env, env_flag};

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

/// Parse a boolean environment variable strictly.
///
/// Unset or empty means `None`. Any value outside `1|true|yes|on|0|false|no|off`
/// (case-insensitive) is a configuration error naming the variable, so a typo
/// never silently falls back to the default.
pub fn env_bool(name: &str) -> Result<Option<bool>, CliError> {
    flag_value(name, env_flag(name))
}

fn flag_value(name: &str, flag: EnvFlag) -> Result<Option<bool>, CliError> {
    match flag {
        EnvFlag::Unset => Ok(None),
        EnvFlag::On => Ok(Some(true)),
        EnvFlag::Off => Ok(Some(false)),
        EnvFlag::Invalid => Err(CliError::Configuration(format!(
            "{name} must be one of 1, true, yes, on, 0, false, no, off"
        ))),
    }
}

/// Validate every boolean `HYPERLIQUID_*` mode variable once at startup.
pub fn validate_env() -> Result<(), CliError> {
    env_bool("HYPERLIQUID_AGENT")?;
    env_bool("HYPERLIQUID_NO_UPDATE_CHECK")?;
    Ok(())
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
    use crate::output::terminal::parse_flag;

    #[test]
    fn strict_bool_accepts_known_values_and_treats_empty_as_unset() {
        assert_eq!(flag_value("X", parse_flag("TRUE")).unwrap(), Some(true));
        assert_eq!(flag_value("X", parse_flag(" off ")).unwrap(), Some(false));
        assert_eq!(flag_value("X", parse_flag("")).unwrap(), None);
    }

    #[test]
    fn strict_bool_rejects_unknown_values_by_name() {
        let err = flag_value("HYPERLIQUID_AGENT", parse_flag("banana")).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("HYPERLIQUID_AGENT"));
    }
}
