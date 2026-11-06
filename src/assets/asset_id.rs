//! Protocol asset-id layout: perp index, spot `10_000 + index`, HIP-3
//! `100_000 + dex_slot * 10_000 + index`, outcome `100_000_000 + outcome * 10 + side`.
//!
//! See Hyperliquid docs: for-developers/api/asset-ids. The HIP-3 formula must
//! mirror hypersdk's.

use crate::errors::CliError;

pub const SPOT_ASSET_ID_OFFSET: u64 = 10_000;
pub const HIP3_ASSET_ID_OFFSET: u64 = 100_000;
pub const HIP3_DEX_SLOT_SIZE: u64 = 10_000;
pub const OUTCOME_ASSET_ID_OFFSET: u64 = 100_000_000;

/// Outcome markets are binary: side 0 is "Yes", side 1 is "No".
pub const MAX_OUTCOME_SIDE: u64 = 1;

const SPOT_OFFSET: usize = SPOT_ASSET_ID_OFFSET as usize;
const HIP3_OFFSET: usize = HIP3_ASSET_ID_OFFSET as usize;
const HIP3_SLOT_SIZE: usize = HIP3_DEX_SLOT_SIZE as usize;

/// Which id range a protocol asset id falls in, with the fields the range encodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetIdLayout {
    Perp { index: u64 },
    Spot { spot_index: u64 },
    Hip3Perp { dex_slot: u64, index: u64 },
    Outcome { encoding: u64 },
}

#[must_use]
pub fn classify(asset_id: u64) -> AssetIdLayout {
    if asset_id >= OUTCOME_ASSET_ID_OFFSET {
        AssetIdLayout::Outcome {
            encoding: asset_id - OUTCOME_ASSET_ID_OFFSET,
        }
    } else if asset_id >= HIP3_ASSET_ID_OFFSET {
        let encoded = asset_id - HIP3_ASSET_ID_OFFSET;
        AssetIdLayout::Hip3Perp {
            dex_slot: encoded / HIP3_DEX_SLOT_SIZE,
            index: encoded % HIP3_DEX_SLOT_SIZE,
        }
    } else if asset_id >= SPOT_ASSET_ID_OFFSET {
        AssetIdLayout::Spot {
            spot_index: asset_id - SPOT_ASSET_ID_OFFSET,
        }
    } else {
        AssetIdLayout::Perp { index: asset_id }
    }
}

/// Protocol asset id of the spot universe entry `spot_index`.
#[must_use]
pub fn spot_asset_index(spot_index: usize) -> usize {
    SPOT_OFFSET + spot_index
}

/// Spot universe index of a protocol spot asset id (the `N` in `@N`).
#[must_use]
pub fn spot_internal_id(asset_index: usize) -> usize {
    asset_index.saturating_sub(SPOT_OFFSET)
}

/// Info-endpoint coin key for a spot asset: `@N`.
#[must_use]
pub fn spot_coin_key(asset_index: usize) -> String {
    spot_internal_coin_key(spot_internal_id(asset_index))
}

/// Info-endpoint coin key for spot universe index `internal_id`: `@N`.
#[must_use]
pub fn spot_internal_coin_key(internal_id: usize) -> String {
    format!("@{internal_id}")
}

/// Parse an info-endpoint spot coin key (`@N`) into its universe index.
#[must_use]
pub fn parse_spot_coin_key(input: &str) -> Option<usize> {
    input
        .trim()
        .strip_prefix('@')
        .and_then(|value| value.parse::<usize>().ok())
}

/// Protocol asset id of market `index` on HIP-3 DEX slot `dex_slot`.
#[must_use]
pub fn hip3_asset_index(dex_slot: usize, index: usize) -> usize {
    HIP3_OFFSET + dex_slot * HIP3_SLOT_SIZE + index
}

/// An outcome side decoded from `#<encoding>` / `+<encoding>` notation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeNotation {
    pub outcome: u64,
    pub side: u64,
    pub encoding: u64,
}

#[must_use]
pub fn split_outcome_encoding(encoding: u64) -> (u64, u64) {
    (encoding / 10, encoding % 10)
}

pub fn parse_outcome_notation(raw: &str) -> Result<OutcomeNotation, CliError> {
    let trimmed = raw.trim();
    let Some(rest) = trimmed
        .strip_prefix('#')
        .or_else(|| trimmed.strip_prefix('+'))
    else {
        return Err(outcome_notation_error(raw));
    };
    if rest.is_empty() || !rest.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(outcome_notation_error(raw));
    }
    let encoding = rest
        .parse::<u64>()
        .map_err(|_| outcome_notation_error(raw))?;
    let (outcome, side) = split_outcome_encoding(encoding);
    if side > MAX_OUTCOME_SIDE {
        return Err(CliError::Unsupported(format!(
            "invalid outcome notation '{raw}': only binary outcome sides 0 and 1 are supported"
        )));
    }
    Ok(OutcomeNotation {
        outcome,
        side,
        encoding,
    })
}

pub fn outcome_encoding(outcome: u64, side: u64) -> Result<u64, CliError> {
    if side > MAX_OUTCOME_SIDE {
        return Err(CliError::Unsupported(
            "only binary outcome sides 0 and 1 are supported".to_string(),
        ));
    }
    outcome
        .checked_mul(10)
        .and_then(|value| value.checked_add(side))
        .ok_or_else(|| CliError::Unsupported("outcome encoding overflowed u64".to_string()))
}

pub fn outcome_asset_id(encoding: u64) -> Result<u64, CliError> {
    OUTCOME_ASSET_ID_OFFSET
        .checked_add(encoding)
        .ok_or_else(|| CliError::Unsupported("outcome asset id overflowed".to_string()))
}

/// Info-endpoint coin key for an outcome side: `#<encoding>`.
#[must_use]
pub fn outcome_coin(encoding: u64) -> String {
    format!("#{encoding}")
}

/// Outcome token name for an outcome side: `+<encoding>`.
#[must_use]
pub fn outcome_token(encoding: u64) -> String {
    format!("+{encoding}")
}

#[must_use]
pub fn outcome_notation_error(raw: &str) -> CliError {
    CliError::Unsupported(format!(
        "invalid outcome notation '{raw}': expected #<encoding> or +<encoding>, for example #10"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_protocol_id_ranges() {
        assert_eq!(classify(0), AssetIdLayout::Perp { index: 0 });
        assert_eq!(classify(9_999), AssetIdLayout::Perp { index: 9_999 });
        assert_eq!(classify(10_000), AssetIdLayout::Spot { spot_index: 0 });
        assert_eq!(classify(99_999), AssetIdLayout::Spot { spot_index: 89_999 });
        assert_eq!(
            classify(110_035),
            AssetIdLayout::Hip3Perp {
                dex_slot: 1,
                index: 35
            }
        );
        assert_eq!(
            classify(100_000_011),
            AssetIdLayout::Outcome { encoding: 11 }
        );
    }

    #[test]
    fn usize_helpers_round_trip_with_the_layout() {
        assert_eq!(spot_asset_index(5), 10_005);
        assert_eq!(spot_internal_id(10_005), 5);
        assert_eq!(spot_coin_key(10_005), "@5");
        assert_eq!(parse_spot_coin_key(" @5 "), Some(5));
        assert_eq!(parse_spot_coin_key("BTC"), None);
        assert_eq!(hip3_asset_index(1, 35), 110_035);
    }

    #[test]
    fn outcome_encoding_round_trips_through_notation() {
        let notation = parse_outcome_notation("#11").unwrap();
        assert_eq!(
            (notation.outcome, notation.side, notation.encoding),
            (1, 1, 11)
        );
        assert_eq!(outcome_encoding(1, 1).unwrap(), 11);
        assert_eq!(outcome_asset_id(11).unwrap(), 100_000_011);
        assert_eq!(parse_outcome_notation("+11").unwrap(), notation);
        assert_eq!(outcome_coin(11), "#11");
        assert_eq!(outcome_token(11), "+11");
    }

    #[test]
    fn outcome_notation_rejects_non_binary_sides_and_garbage() {
        assert!(parse_outcome_notation("#12").is_err());
        assert!(parse_outcome_notation("10").is_err());
        assert!(parse_outcome_notation("#").is_err());
        assert!(parse_outcome_notation("#1a").is_err());
        assert!(outcome_encoding(1, 2).is_err());
        assert!(outcome_encoding(u64::MAX, 0).is_err());
        assert!(outcome_asset_id(u64::MAX).is_err());
    }
}
