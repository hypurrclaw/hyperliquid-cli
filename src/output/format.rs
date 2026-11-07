//! Shared formatters for counts, durations, and timestamps in human output.
//!
//! JSON keeps raw numbers; these helpers shape pretty and table cells and
//! notices only.

use std::time::Duration;

use chrono::{DateTime, SecondsFormat};

/// Group digits in thousands: `1424` -> `1,424`.
#[must_use]
pub fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Count with a correctly pluralized noun: `plural(1, "perp", "perps")` ->
/// `1 perp`, `plural(1424, "mid", "mids")` -> `1,424 mids`.
///
/// Accepts any integer type that converts to `u64`; negative values read as 0.
#[must_use]
pub fn plural<N: TryInto<u64>>(count: N, singular: &str, plural: &str) -> String {
    let count = count.try_into().unwrap_or(0);
    let noun = if count == 1 { singular } else { plural };
    format!("{} {noun}", thousands(count))
}

/// Pick the singular or plural noun for `count` without formatting the number.
pub fn noun<'a, N: TryInto<u64>>(count: N, singular: &'a str, plural: &'a str) -> &'a str {
    let count = count.try_into().unwrap_or(0);
    if count == 1 { singular } else { plural }
}
/// Compact duration: `42ms`, `3.4s`, `2m 10s`, `1h 5m`.
#[must_use]
pub fn duration(value: Duration) -> String {
    let millis = value.as_millis();
    if millis < 1_000 {
        return format!("{millis}ms");
    }
    if millis < 59_950 {
        return format!("{:.1}s", value.as_secs_f64());
    }
    let total_seconds = u64::try_from((millis + 500) / 1_000).unwrap_or(u64::MAX);
    let (minutes, seconds) = (total_seconds / 60, total_seconds % 60);
    if minutes < 60 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{}h {}m", minutes / 60, minutes % 60)
    }
}

/// Epoch milliseconds as RFC 3339 UTC with millisecond precision:
/// `2026-11-06T10:00:00.085Z`. Out-of-range values come back as the raw number.
#[must_use]
pub fn epoch_ms_to_iso<N: TryInto<i64> + Copy + std::fmt::Display>(millis: N) -> String {
    millis
        .try_into()
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .map_or_else(
            || millis.to_string(),
            |time| time.to_rfc3339_opts(SecondsFormat::Millis, true),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_424), "1,424");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(u64::MAX), "18,446,744,073,709,551,615");
    }

    #[test]
    fn plural_picks_noun_and_groups() {
        assert_eq!(plural(0usize, "perp", "perps"), "0 perps");
        assert_eq!(plural(1usize, "perp", "perps"), "1 perp");
        assert_eq!(plural(2u32, "perp", "perps"), "2 perps");
        assert_eq!(plural(1424u64, "mid", "mids"), "1,424 mids");
        assert_eq!(plural(-3i64, "mid", "mids"), "0 mids");
    }

    #[test]
    fn duration_is_compact() {
        assert_eq!(duration(Duration::from_millis(0)), "0ms");
        assert_eq!(duration(Duration::from_millis(42)), "42ms");
        assert_eq!(duration(Duration::from_millis(999)), "999ms");
        assert_eq!(duration(Duration::from_millis(1_000)), "1.0s");
        assert_eq!(duration(Duration::from_millis(3_400)), "3.4s");
        assert_eq!(duration(Duration::from_millis(59_949)), "59.9s");
        assert_eq!(duration(Duration::from_millis(59_950)), "1m 0s");
        assert_eq!(duration(Duration::from_secs(130)), "2m 10s");
        assert_eq!(duration(Duration::from_secs(3_900)), "1h 5m");
    }

    #[test]
    fn epoch_ms_formats_rfc3339_utc() {
        assert_eq!(epoch_ms_to_iso(0u64), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            epoch_ms_to_iso(1_793_959_200_085_u64),
            "2026-11-06T10:00:00.085Z"
        );
        assert_eq!(
            epoch_ms_to_iso(1_793_959_200_085_i64),
            "2026-11-06T10:00:00.085Z"
        );
        assert_eq!(epoch_ms_to_iso(u64::MAX), u64::MAX.to_string());
    }
}
