//! Terminal detection for output shaping.
//!
//! Human formats shape themselves by stream: bordered tables and indented JSON
//! on a terminal, tab-separated tables and compact JSON on a pipe.

#[cfg(test)]
thread_local! {
    static TEST_TTY: std::cell::Cell<Option<(bool, bool)>> = const { std::cell::Cell::new(None) };
}

/// Whether stdout is connected to a terminal.
#[cfg(not(test))]
#[must_use]
pub fn stdout_is_terminal() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdout())
}

/// Whether stderr is connected to a terminal.
#[cfg(not(test))]
#[must_use]
pub fn stderr_is_terminal() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stderr())
}

/// Unit tests are never terminals unless a test opts in with [`with_tty_for_test`].
#[cfg(test)]
#[must_use]
pub fn stdout_is_terminal() -> bool {
    TEST_TTY.with(|cell| cell.get()).is_some_and(|tty| tty.0)
}

/// Unit tests are never terminals unless a test opts in with [`with_tty_for_test`].
#[cfg(test)]
#[must_use]
pub fn stderr_is_terminal() -> bool {
    TEST_TTY.with(|cell| cell.get()).is_some_and(|tty| tty.1)
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
