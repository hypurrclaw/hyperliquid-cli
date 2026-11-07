//! A resolved market: what an asset name or protocol id refers to.

use crate::errors::CliError;

use super::asset_id::{self, OutcomeNotation};

/// Perpetual market, including HIP-3 markets when `dex` is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerpAsset {
    pub name: String,
    pub index: usize,
    pub dex: Option<String>,
    pub sz_decimals: u32,
    pub collateral: String,
}

impl PerpAsset {
    /// `name` for default-dex markets, `dex:name` for HIP-3 markets.
    #[must_use]
    pub fn display_name(&self) -> String {
        match &self.dex {
            Some(dex) => format!("{dex}:{}", self.name),
            None => self.name.clone(),
        }
    }
}

/// Spot market pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotAsset {
    pub symbol: String,
    pub index: usize,
    pub base: String,
    pub quote: String,
    pub base_sz_decimals: u32,
}

impl SpotAsset {
    /// Coin key info and websocket endpoints use for this pair.
    ///
    /// Only PURR/USDC is addressed by name; every other pair by `@<spot index>`.
    #[must_use]
    pub fn info_coin(&self) -> String {
        if self.base.eq_ignore_ascii_case("PURR")
            && self.quote.eq_ignore_ascii_case("USDC")
            && self.index == asset_id::spot_asset_index(0)
        {
            self.symbol.clone()
        } else {
            asset_id::spot_coin_key(self.index)
        }
    }
}

/// Outcome market side, held as the notation the user typed (`#10` / `+10`).
///
/// The notation is decoded on demand so a malformed one is reported by the
/// command that needs the encoding, not by every resolver caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeAsset {
    notation: String,
}

impl OutcomeAsset {
    #[must_use]
    pub fn new(notation: impl Into<String>) -> Self {
        Self {
            notation: notation.into(),
        }
    }

    #[must_use]
    pub fn notation(&self) -> &str {
        &self.notation
    }

    pub fn parse(&self) -> Result<OutcomeNotation, CliError> {
        asset_id::parse_outcome_notation(&self.notation)
    }

    pub fn asset_index(&self) -> Result<usize, CliError> {
        let asset_id = asset_id::outcome_asset_id(self.parse()?.encoding)?;
        usize::try_from(asset_id).map_err(|_| {
            CliError::Unsupported("outcome asset id is outside supported bounds".to_string())
        })
    }
}

/// What an asset name resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Market {
    Perp(PerpAsset),
    Spot(SpotAsset),
    Outcome(OutcomeAsset),
}

impl Market {
    /// Protocol asset id used in exchange actions.
    pub fn asset_index(&self) -> Result<usize, CliError> {
        match self {
            Self::Perp(perp) => Ok(perp.index),
            Self::Spot(spot) => Ok(spot.index),
            Self::Outcome(outcome) => outcome.asset_index(),
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::{PerpAsset, SpotAsset};

    pub(crate) fn perp(name: &str, index: usize) -> PerpAsset {
        PerpAsset {
            name: name.to_string(),
            index,
            dex: None,
            sz_decimals: 8,
            collateral: "USDC".to_string(),
        }
    }

    pub(crate) fn hip3_perp(dex: &str, name: &str, index: usize) -> PerpAsset {
        PerpAsset {
            dex: Some(dex.to_string()),
            ..perp(name, index)
        }
    }

    pub(crate) fn spot(symbol: &str, index: usize, base: &str, quote: &str) -> SpotAsset {
        SpotAsset {
            symbol: symbol.to_string(),
            index,
            base: base.to_string(),
            quote: quote.to_string(),
            base_sz_decimals: 8,
        }
    }
}
