//! Helpers for presenting untrusted protocol/API text to agents and humans.

/// Label applied when raw protocol, exchange, or user-provided text is surfaced in an error.
pub const UNTRUSTED_LABEL: &str = "[untrusted remote data]";

/// Maximum characters kept from one untrusted text value.
pub const MAX_UNTRUSTED_CHARS: usize = 300;

/// Sanitize untrusted text while preserving enough content for debugging.
///
/// JSON serialization already escapes strings, but error text is also shown in
/// terminals and later consumed by agents. This removes ANSI/control sequences,
/// bidirectional controls, and zero-width or other format characters that can
/// alter presentation or hide content, collapses whitespace, and caps the
/// result at [`MAX_UNTRUSTED_CHARS`] characters with a trailing `…`.
#[must_use]
pub fn sanitize_untrusted_text(input: &str) -> String {
    let without_ansi = strip_ansi_sequences(input);
    let cleaned = without_ansi
        .chars()
        .filter(|ch| !is_format_char(*ch))
        .map(|ch| match ch {
            '\n' | '\r' | '\t' => ' ',
            ch if ch.is_control() => ' ',
            ch => ch,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    truncate_chars(cleaned, MAX_UNTRUSTED_CHARS)
}

fn truncate_chars(text: String, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((byte_index, _)) => {
            let mut truncated = text;
            truncated.truncate(byte_index);
            truncated.push('…');
            truncated
        }
        None => text,
    }
}

/// Unicode general category `Cf` (format) characters, including every bidi
/// control and zero-width character.
fn is_format_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0001}'
            | '\u{E0020}'..='\u{E007F}'
    )
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

#[cfg(test)]
mod format_tests {
    use super::*;

    #[test]
    fn strips_bidi_controls() {
        let text = "pay \u{202E}evil\u{202C} \u{2066}now\u{2069}";
        assert_eq!(sanitize_untrusted_text(text), "pay evil now");
    }

    #[test]
    fn strips_zero_width_and_format_characters() {
        let text = "in\u{200B}sti\u{200D}gate\u{FEFF} \u{2060}run\u{00AD}ner \u{E0041}tag";
        assert_eq!(sanitize_untrusted_text(text), "instigate runner tag");
    }

    #[test]
    fn caps_length_with_ellipsis() {
        let long = "a".repeat(MAX_UNTRUSTED_CHARS + 50);
        let out = sanitize_untrusted_text(&long);
        assert_eq!(out.chars().count(), MAX_UNTRUSTED_CHARS + 1);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn leaves_text_at_the_cap_untouched() {
        let exact = "b".repeat(MAX_UNTRUSTED_CHARS);
        assert_eq!(sanitize_untrusted_text(&exact), exact);
    }

    #[test]
    fn truncates_on_char_boundaries() {
        let long = "é".repeat(MAX_UNTRUSTED_CHARS + 5);
        let out = sanitize_untrusted_text(&long);
        assert_eq!(out.chars().count(), MAX_UNTRUSTED_CHARS + 1);
    }

    #[test]
    fn keeps_ordinary_unicode_text() {
        assert_eq!(sanitize_untrusted_text("ethér 日本語 ✓"), "ethér 日本語 ✓");
    }
}
