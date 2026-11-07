//! Positive decimal amounts scaled to integer wire units.
//!
//! Callers keep their own error text. The scaling rule is the one Hyperliquid
//! wire formats share: reject non-positive amounts, reject a fractional scaled
//! unit, then fit the integer into the wire type.

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use crate::errors::CliError;

pub(crate) fn scaled_u64(
    amount: Decimal,
    scale: u64,
    non_positive: CliError,
    too_fine: CliError,
    too_large: CliError,
) -> Result<u64, CliError> {
    if amount <= Decimal::ZERO {
        return Err(non_positive);
    }
    let scaled = amount * Decimal::from(scale);
    if scaled.fract() != Decimal::ZERO {
        return Err(too_fine);
    }
    scaled.to_u64().ok_or(too_large)
}

pub(crate) fn scaled_i64(
    amount: Decimal,
    scale: u64,
    non_positive: CliError,
    too_fine: CliError,
    too_large: CliError,
) -> Result<i64, CliError> {
    if amount <= Decimal::ZERO {
        return Err(non_positive);
    }
    let scaled = amount * Decimal::from(scale);
    if scaled.fract() != Decimal::ZERO {
        return Err(too_fine);
    }
    scaled.to_i64().ok_or(too_large)
}
