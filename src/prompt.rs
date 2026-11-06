//! Shared prompts. Questions go to stderr; answers come from stdin.
//!
//! Every prompt checks [`Mode::can_prompt`] first and fails with
//! [`CliError::MissingInput`] naming the option that supplies the answer, so a
//! non-interactive run never blocks or guesses. End of input is also missing
//! input: an empty stdin never accepts a default.

use std::io::{self, BufRead, Write};

use crate::errors::CliError;
use crate::mode::Mode;
use crate::output::{colors, terminal};

/// Ask a yes/no question. Only `y` or `yes` (any case) confirm.
///
/// `bypass` names the option that answers the question in non-interactive
/// runs, such as `--yes`.
pub fn confirm(mode: &Mode, question: &str, bypass: &str) -> Result<bool, CliError> {
    let answer = read_line(mode, &format!("{question} [y/N]: "), bypass)?;
    Ok(is_yes(&answer))
}

/// Like [`confirm`], with the question styled as a warning on stderr.
pub fn confirm_warning(mode: &Mode, question: &str, bypass: &str) -> Result<bool, CliError> {
    confirm(mode, &warning(question), bypass)
}

/// Style `text` as a warning for a prompt or consent preview written to stderr.
#[must_use]
pub fn warning(text: &str) -> String {
    colors::stderr::yellow(text)
}

/// Ask for one line of text and return it without its line ending.
///
/// `input` names the option or argument that supplies the value without a
/// prompt.
pub fn read_line(mode: &Mode, label: &str, input: &str) -> Result<String, CliError> {
    ensure_can_prompt(mode, input)?;
    let mut stderr = io::stderr().lock();
    write!(stderr, "{label}")
        .and_then(|()| stderr.flush())
        .map_err(io_error)?;
    drop(stderr);

    let mut line = String::new();
    let read = io::stdin().lock().read_line(&mut line).map_err(io_error)?;
    if read == 0 {
        eprintln!();
        return Err(CliError::MissingInput(format!(
            "Can't continue without {input}. Stdin ended before an answer was given."
        )));
    }
    if !terminal::stdin_is_terminal() {
        // Piped answers aren't echoed, so end the prompt line ourselves.
        eprintln!();
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

/// Fail with a missing-input error when this run can't prompt.
pub fn ensure_can_prompt(mode: &Mode, input: &str) -> Result<(), CliError> {
    match mode.prompt_blocker() {
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

/// Ask the user to type `expected` exactly. Anything else, including an empty
/// line, declines. Use for destructive actions that can't be undone.
pub fn confirm_typed(
    mode: &Mode,
    label: &str,
    expected: &str,
    bypass: &str,
) -> Result<bool, CliError> {
    let answer = read_line(mode, label, bypass)?;
    Ok(answer.trim() == expected)
}

/// Read a secret without echoing it.
///
/// On a terminal the value is typed at a masked prompt, which needs a run that
/// can prompt. Piped stdin supplies the secret directly: the first line, without
/// its line ending. End of input is missing input.
pub fn read_secret(mode: &Mode, label: &str, input: &str) -> Result<String, CliError> {
    let value = if terminal::stdin_is_terminal() {
        ensure_can_prompt(mode, input)?;
        write_label(label)?;
        let value = rpassword::read_password().map_err(io_error)?;
        eprintln!();
        value
    } else {
        if mode.can_prompt() {
            write_label(label)?;
        }
        let mut line = String::new();
        let read = io::stdin().lock().read_line(&mut line).map_err(io_error)?;
        if read == 0 {
            if mode.can_prompt() {
                eprintln!();
            }
            return Err(CliError::MissingInput(format!(
                "Can't continue without {input}. Stdin ended before a value was given."
            )));
        }
        line.trim_end_matches(['\r', '\n']).to_string()
    };
    if value.trim().is_empty() {
        return Err(CliError::MissingInput(format!(
            "Can't continue without {input}. The value was empty."
        )));
    }
    Ok(value)
}

fn write_label(label: &str) -> Result<(), CliError> {
    let mut stderr = io::stderr().lock();
    write!(stderr, "{label}")
        .and_then(|()| stderr.flush())
        .map_err(io_error)
}

fn io_error(err: io::Error) -> CliError {
    CliError::Internal(anyhow::anyhow!(
        "couldn't use the terminal for a prompt: {err}"
    ))
}

#[cfg(test)]
mod tests {
    use super::{is_yes, warning};
    use crate::output::colors;

    #[test]
    fn only_explicit_yes_confirms() {
        assert!(is_yes("y"));
        assert!(is_yes(" YES \n"));
        assert!(!is_yes(""));
        assert!(!is_yes("yep"));
        assert!(!is_yes("n"));
    }

    #[test]
    fn warning_follows_stderr_color_only() {
        colors::with_streams_for_test(true, false, || assert_eq!(warning("q"), "q"));
        colors::with_streams_for_test(false, true, || {
            assert_eq!(warning("q"), colors::paint(colors::YELLOW, "q"));
        });
    }
}
