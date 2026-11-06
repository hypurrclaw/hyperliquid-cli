//! Environment facts that shape output: which standard streams are terminals
//! and whether an agent or CI drives the run.
//!
//! Human formats shape themselves by stream: bordered tables and indented JSON
//! on a terminal, tab-separated tables and compact JSON on a pipe. Colors and
//! prompts read the same probes, so every module agrees on what a terminal is.

use std::io::IsTerminal;

#[derive(Clone, Copy)]
enum Stream {
    Stdin,
    Stdout,
    Stderr,
}

#[cfg(not(test))]
fn probe(stream: Stream) -> bool {
    match stream {
        Stream::Stdin => std::io::stdin().is_terminal(),
        Stream::Stdout => std::io::stdout().is_terminal(),
        Stream::Stderr => std::io::stderr().is_terminal(),
    }
}

#[cfg(test)]
thread_local! {
    static TEST_TTY: std::cell::Cell<Option<(bool, bool)>> = const { std::cell::Cell::new(None) };
}

/// Unit tests never see the developer's stdout or stderr as terminals unless a
/// test opts in with [`with_tty_for_test`].
#[cfg(test)]
fn probe(stream: Stream) -> bool {
    let tty = TEST_TTY.with(std::cell::Cell::get);
    match stream {
        Stream::Stdin => std::io::stdin().is_terminal(),
        Stream::Stdout => tty.is_some_and(|tty| tty.0),
        Stream::Stderr => tty.is_some_and(|tty| tty.1),
    }
}

/// Whether stdin is an interactive terminal.
#[must_use]
pub fn stdin_is_terminal() -> bool {
    probe(Stream::Stdin)
}

/// Whether stdout is connected to a terminal.
#[must_use]
pub fn stdout_is_terminal() -> bool {
    probe(Stream::Stdout)
}

/// Whether stderr is connected to a terminal.
#[must_use]
pub fn stderr_is_terminal() -> bool {
    probe(Stream::Stderr)
}

/// Run `f` with the given `(stdout, stderr)` terminal state on this thread.
#[cfg(test)]
pub fn with_tty_for_test<T>(stdout: bool, stderr: bool, f: impl FnOnce() -> T) -> T {
    struct Restore(Option<(bool, bool)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_TTY.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(TEST_TTY.with(|cell| cell.replace(Some((stdout, stderr)))));
    f()
}

/// A boolean environment variable read strictly: only
/// `1|true|yes|on|0|false|no|off` (case-insensitive) are values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvFlag {
    /// Unset or empty.
    Unset,
    On,
    Off,
    /// Set to something outside the accepted values.
    Invalid,
}

#[must_use]
pub fn parse_flag(raw: &str) -> EnvFlag {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" => EnvFlag::Unset,
        "1" | "true" | "yes" | "on" => EnvFlag::On,
        "0" | "false" | "no" | "off" => EnvFlag::Off,
        _ => EnvFlag::Invalid,
    }
}

#[must_use]
pub fn env_flag(name: &str) -> EnvFlag {
    std::env::var(name).map_or(EnvFlag::Unset, |raw| parse_flag(&raw))
}

/// `HYPERLIQUID_AGENT` is set to a true value. An invalid value counts as off;
/// startup validation rejects it before any command runs.
#[must_use]
pub fn agent_env() -> bool {
    env_flag("HYPERLIQUID_AGENT") == EnvFlag::On
}

/// The process runs under CI (`CI` set to anything except empty, `0`, or `false`).
#[must_use]
pub fn ci_env() -> bool {
    std::env::var("CI").is_ok_and(|value| {
        let value = value.trim().to_ascii_lowercase();
        !value.is_empty() && value != "0" && value != "false"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_accept_known_values_and_treat_empty_as_unset() {
        assert_eq!(parse_flag("TRUE"), EnvFlag::On);
        assert_eq!(parse_flag(" off "), EnvFlag::Off);
        assert_eq!(parse_flag(""), EnvFlag::Unset);
        assert_eq!(parse_flag("banana"), EnvFlag::Invalid);
    }
}
