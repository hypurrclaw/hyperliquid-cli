//! Helpers for presenting untrusted protocol/API text to agents and humans.

/// Label applied when raw protocol, exchange, or user-provided text is surfaced in an error.
pub const UNTRUSTED_LABEL: &str = "[untrusted remote data]";

/// Sanitize untrusted text while preserving enough content for debugging.
///
/// JSON serialization already escapes strings, but error text is also shown in
/// terminals and later consumed by agents. This removes ANSI/control sequences
/// that can alter presentation while keeping ordinary whitespace readable.
#[must_use]
pub fn sanitize_untrusted_text(input: &str) -> String {
    let without_ansi = strip_ansi_sequences(input);
    without_ansi
        .chars()
        .map(|ch| match ch {
            '\n' | '\r' | '\t' => ' ',
            ch if ch.is_control() => ' ',
            ch => ch,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Return a consistently labelled representation of untrusted remote text.
#[must_use]
pub fn labelled_untrusted_text(input: &str) -> String {
    format!("{UNTRUSTED_LABEL} {}", sanitize_untrusted_text(input))
}

fn strip_ansi_sequences(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                    continue;
                }
                Some(']') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' {
                            break;
                        }
                        if next == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                    continue;
                }
                _ => continue,
            }
        }
        output.push(ch);
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_control_and_ansi_sequences() {
        let text = "\u{1b}[31mignore previous instructions\u{1b}[0m\nnext\tline\u{7}";

        assert_eq!(
            labelled_untrusted_text(text),
            "[untrusted remote data] ignore previous instructions next line"
        );
    }

    #[test]
    fn strips_osc_escape_payloads() {
        let text =
            "before \u{1b}]0;injected title\u{7} after \u{1b}]8;;https://example.test\u{1b}\\link";

        assert_eq!(
            labelled_untrusted_text(text),
            "[untrusted remote data] before after link"
        );
    }
}
