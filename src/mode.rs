//! Whether this invocation may prompt, decided once in `main`.
//!
//! Commands ask the [`Mode`] they were handed. They never read
//! `HYPERLIQUID_AGENT`, `CI`, or TTY state for interaction decisions
//! themselves, so every command agrees.

use crate::errors::CliError;
use crate::output::terminal::{EnvFlag, OutputEnv, env_flag};

/// What decides whether this invocation may prompt, resolved once in `main`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mode {
    /// `--non-interactive` was passed.
    pub non_interactive_flag: bool,
    /// The effective output format is JSON (explicit, env, agent, or non-TTY default).
    pub machine_format: bool,
    /// Stdin carries a payload (`--payload-file -`), so it can't answer prompts.
    pub stdin_is_payload: bool,
    /// `HYPERLIQUID_AGENT` is on.
    pub agent: bool,
    /// The process runs under CI.
    pub ci: bool,
}

impl Mode {
    #[must_use]
    pub fn new(
        non_interactive_flag: bool,
        machine_format: bool,
        stdin_is_payload: bool,
        env: &OutputEnv,
    ) -> Self {
        Self {
            non_interactive_flag,
            machine_format,
            stdin_is_payload,
            agent: env.agent,
            ci: env.ci,
        }
    }

    /// Whether this invocation may ask the user a question.
    ///
    /// Prompts are off for `--non-interactive`, JSON output, a detected agent, CI,
    /// and when stdin is a payload. Piped stdin may still answer prompts in human
    /// mode; end-of-input is treated as missing input by [`crate::prompt`].
    #[must_use]
    pub fn can_prompt(&self) -> bool {
        self.prompt_blocker().is_none()
    }

    /// Why prompts are off, for error copy. `None` when prompts are allowed.
    #[must_use]
    pub fn prompt_blocker(&self) -> Option<&'static str> {
        if self.non_interactive_flag {
            Some("--non-interactive")
        } else if self.stdin_is_payload {
            Some("stdin is used as the payload")
        } else if self.machine_format {
            Some("JSON output")
        } else if self.agent {
            Some("HYPERLIQUID_AGENT")
        } else if self.ci {
            Some("CI")
        } else {
            None
        }
    }
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

    #[test]
    fn prompts_are_blocked_by_the_first_matching_reason() {
        assert!(Mode::default().can_prompt());
        assert_eq!(Mode::default().prompt_blocker(), None);

        let flagged = Mode {
            non_interactive_flag: true,
            machine_format: true,
            ..Mode::default()
        };
        assert_eq!(flagged.prompt_blocker(), Some("--non-interactive"));

        let payload = Mode {
            stdin_is_payload: true,
            machine_format: true,
            ..Mode::default()
        };
        assert_eq!(
            payload.prompt_blocker(),
            Some("stdin is used as the payload")
        );

        let json = Mode {
            machine_format: true,
            agent: true,
            ..Mode::default()
        };
        assert_eq!(json.prompt_blocker(), Some("JSON output"));

        let agent = Mode {
            agent: true,
            ci: true,
            ..Mode::default()
        };
        assert_eq!(agent.prompt_blocker(), Some("HYPERLIQUID_AGENT"));

        let ci = Mode {
            ci: true,
            ..Mode::default()
        };
        assert!(!ci.can_prompt());
        assert_eq!(ci.prompt_blocker(), Some("CI"));
    }
}
