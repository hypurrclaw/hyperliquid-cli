//! Parsing of the asset names users type.

/// Parsed user asset input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetQuery {
    /// Default perpetual market symbol, e.g. `BTC`.
    Perp(String),
    /// Spot trading pair, e.g. `PURR/USDC`.
    Spot(String),
    /// HIP-3 DEX-qualified perpetual symbol, e.g. `dex:TOKEN`.
    Hip3 { dex: String, token: String },
    /// Outcome market notation, e.g. `#10` or `+10`.
    Outcome(String),
}

/// Parse the asset formats supported by the CLI.
///
/// Formats:
/// - `BTC` => default perpetual market
/// - `PURR/USDC` => spot pair
/// - `dex:TOKEN` => HIP-3 DEX perpetual market
/// - `#10` / `+10` => outcome market notation
#[must_use]
pub fn parse_asset_query(input: &str) -> AssetQuery {
    let trimmed = input.trim();

    if trimmed.starts_with('#') || trimmed.starts_with('+') {
        return AssetQuery::Outcome(trimmed.to_string());
    }

    if let Some((dex, token)) = trimmed.split_once(':') {
        return AssetQuery::Hip3 {
            dex: dex.trim().to_string(),
            token: token.trim().to_string(),
        };
    }

    if trimmed.contains('/') {
        AssetQuery::Spot(trimmed.to_string())
    } else {
        AssetQuery::Perp(trimmed.to_string())
    }
}

/// Apply a `--dex` flag to a coin: `--dex dex COIN` means `dex:COIN`.
#[must_use]
pub fn qualify_dex_asset(dex: Option<&str>, coin: &str) -> String {
    match dex {
        Some(dex) if !coin.contains(':') => format!("{dex}:{coin}"),
        _ => coin.to_string(),
    }
}

pub(crate) fn eq_asset_name(left: &str, right: &str) -> bool {
    normalize_asset_name(left) == normalize_asset_name(right)
}

pub(crate) fn normalize_asset_name(input: &str) -> String {
    input.trim().to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dex_flag_qualifies_only_unqualified_coins() {
        assert_eq!(qualify_dex_asset(Some("xyz"), "GOLD"), "xyz:GOLD");
        assert_eq!(qualify_dex_asset(Some("xyz"), "abc:GOLD"), "abc:GOLD");
        assert_eq!(qualify_dex_asset(None, "BTC"), "BTC");
    }

    #[test]
    fn every_input_shape_parses_to_one_query_kind() {
        assert_eq!(parse_asset_query(" btc "), AssetQuery::Perp("btc".into()));
        assert_eq!(
            parse_asset_query("PURR/USDC"),
            AssetQuery::Spot("PURR/USDC".into())
        );
        assert_eq!(
            parse_asset_query("dex : TOKEN"),
            AssetQuery::Hip3 {
                dex: "dex".into(),
                token: "TOKEN".into()
            }
        );
        assert_eq!(parse_asset_query("+11"), AssetQuery::Outcome("+11".into()));
    }
}
