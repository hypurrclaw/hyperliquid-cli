//! Shared prompts. Questions go to stderr; answers come from stdin.
//!
//! Every prompt checks [`crate::mode::can_prompt`] first and fails with
//! [`CliError::MissingInput`] naming the option that supplies the answer, so a
//! non-interactive run never blocks or guesses. End of input is also missing
//! input: an empty stdin never accepts a default.

use std::io::{self, BufRead, Write};

use crate::errors::CliError;
use crate::mode;

/// Ask a yes/no question. Only `y` or `yes` (any case) confirm.
///
/// `bypass` names the option that answers the question in non-interactive
/// runs, such as `--yes`.
pub fn confirm(question: &str, bypass: &str) -> Result<bool, CliError> {
    let answer = read_line(&format!("{question} [y/N]: "), bypass)?;
    Ok(is_yes(&answer))
}

/// Ask for one line of text and return it without its line ending.
///
/// `input` names the option or argument that supplies the value without a
/// prompt.
pub fn read_line(label: &str, input: &str) -> Result<String, CliError> {
    ensure_can_prompt(input)?;
    let mut stderr = io::stderr().lock();
    write!(stderr, "{label}").and_then(|()| stderr.flush()).map_err(io_error)?;
    drop(stderr);

    let mut line = String::new();
    let read = io::stdin().lock().read_line(&mut line).map_err(io_error)?;
    if read == 0 {
        eprintln!();
        return Err(CliError::MissingInput(format!(
            "Can't continue without {input}. Stdin ended before an answer was given."
        )));
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

/// Fail with a missing-input error when this run can't prompt.
pub fn ensure_can_prompt(input: &str) -> Result<(), CliError> {
    match mode::prompt_blocker() {
        None => Ok(()),
        Some(reason) => Err(CliError::MissingInput(format!(
            "Can't prompt for {input} because of {reason}. Pass {input} instead."
        ))),
    }
}

/// `y` or `yes`, case-insensitive, surrounding whitespace ignored.
pub fn is_yes(input: &str) -> bool {
    matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn io_error(err: io::Error) -> CliError {
    CliError::Internal(anyhow::anyhow!("couldn't use the terminal for a prompt: {err}"))
}

#[cfg(test)]
mod tests {
    use super::is_yes;

    #[test]
    fn only_explicit_yes_confirms() {
        assert!(is_yes("y"));
        assert!(is_yes(" YES \n"));
        assert!(!is_yes(""));
        assert!(!is_yes("yep"));
        assert!(!is_yes("n"));
    }
}
