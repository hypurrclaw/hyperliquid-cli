//! Human error output and process exit.

use super::{CliError, ErrorEnvelope};
use crate::output::colors;

/// Where an error goes: the JSON envelope on stdout, or a human block on stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorOutput {
    Json,
    Human,
}

/// Render a human-readable error block (no trailing newline).
///
/// Rich output starts with a red `✗`; plain output starts with `Error:`.
/// Detail lines are indented two spaces. A recovery block follows after a blank
/// line: the instruction, each command indented four spaces, then any note.
#[must_use]
pub fn render_human_error(err: &CliError, rich: bool) -> String {
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
            if let Some(when) = command.when {
                out.push_str(&format!("\n{when}:"));
            }
            out.push_str(&format!("\n    {}", command.command));
        }
        if let Some(note) = recovery.note {
            out.push_str(&format!("\n{note}"));
        }
    }
    out
}

/// Print the error envelope to **stdout**.
///
/// [`CliError::PartialResults`] prints nothing: the command that raised it
/// already wrote its own single JSON document with per-item results.
pub fn print_json_error(err: &CliError) {
    if matches!(err, CliError::PartialResults(_)) {
        return;
    }
    println!("{}", ErrorEnvelope::from_cli_error(err).to_json());
}

/// Print the human error block to **stderr** (`✗ ...` when stderr supports
/// styling, `Error: ...` otherwise).
pub fn print_human_error(err: &CliError) {
    eprintln!("{}", render_human_error(err, colors::stderr_enabled()));
}

/// Terminate the process with the correct exit code after printing the error.
pub fn exit_with_error(err: CliError, output: ErrorOutput) -> ! {
    match output {
        ErrorOutput::Json => print_json_error(&err),
        ErrorOutput::Human => print_human_error(&err),
    }
    std::process::exit(err.exit_code())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_uses_error_prefix() {
        let text = render_human_error(&CliError::rate_limited(), false);
        assert_eq!(
            text,
            "Error: Too many requests to the Hyperliquid API. Try again shortly."
        );
    }

    #[test]
    fn rich_uses_red_glyph() {
        let text = render_human_error(&CliError::rate_limited(), true);
        assert!(
            text.starts_with("\u{1b}[31m✗\u{1b}[0m Too many requests"),
            "{text}"
        );
        assert!(!text.contains("Error:"));
    }

    #[test]
    fn recovery_block_for_auth() {
        let text = render_human_error(&CliError::AuthRequired, false);
        assert_eq!(
            text,
            "Error: Authentication required.\n\nSet up a wallet:\n    hyperliquid setup\nOr set HYPERLIQUID_PRIVATE_KEY for this run."
        );
    }

    #[test]
    fn recovery_block_for_unknown_outcome() {
        let text = render_human_error(&CliError::unknown_outcome("The request timed out"), false);
        assert_eq!(
            text,
            "Error: Can't confirm whether the exchange applied this action. The request timed out.\n\nCheck open orders and recent fills:\n    hyperliquid orders open\n    hyperliquid account fills"
        );
    }

    #[test]
    fn recovery_block_for_confirmation_required() {
        let err = CliError::ConfirmationRequired {
            headline: "Can't run `orders create` without confirmation.".into(),
            preview: Some("hyperliquid orders create --dry-run".into()),
            approve: Some("hyperliquid orders create --yes".into()),
        };
        assert_eq!(
            render_human_error(&err, false),
            "Error: Can't run `orders create` without confirmation.\n\nPreview it:\n    hyperliquid orders create --dry-run\nOnly after a human approves:\n    hyperliquid orders create --yes"
        );
    }

    #[test]
    fn status_goes_in_a_detail_line() {
        let err = CliError::api_status(reqwest::StatusCode::SERVICE_UNAVAILABLE, "");
        assert_eq!(
            render_human_error(&err, false),
            "Error: The Hyperliquid API failed to process the request.\n  HTTP 503 Service Unavailable"
        );
    }
}
