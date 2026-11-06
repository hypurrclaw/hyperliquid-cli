use std::time::{Duration, Instant};

use hyperliquid_cli::assets::{
    AssetMetadata, AssetQuery, AssetResolver, CachedMetadata, PerpAsset, SpotAsset,
    parse_asset_query, suggestions,
};
use hyperliquid_cli::errors::CliError;

fn perp(name: &str, index: usize, dex: Option<&str>) -> PerpAsset {
    PerpAsset {
        name: name.to_string(),
        index,
        dex: dex.map(ToString::to_string),
        sz_decimals: 8,
        collateral: "USDC".to_string(),
    }
}

fn purr_usdc() -> SpotAsset {
    SpotAsset {
        symbol: "PURR/USDC".to_string(),
        index: 10_000,
        base: "PURR".to_string(),
        quote: "USDC".to_string(),
        base_sz_decimals: 8,
    }
}

fn fixture_metadata() -> AssetMetadata {
    AssetMetadata::from_assets(
        vec![
            perp("BTC", 0, None),
            perp("ETH", 1, None),
            perp("BLUR", 2, None),
            perp("BONK", 3, None),
            perp("TOKEN", 110_000, Some("dex")),
        ],
        vec![purr_usdc()],
    )
}

#[test]
fn parses_perp_symbol_format() {
    assert_eq!(
        parse_asset_query("BTC"),
        AssetQuery::Perp("BTC".to_string())
    );
}

#[test]
fn parses_spot_pair_format() {
    assert_eq!(
        parse_asset_query("PURR/USDC"),
        AssetQuery::Spot("PURR/USDC".to_string())
    );
}

#[test]
fn parses_hip3_dex_token_format() {
    assert_eq!(
        parse_asset_query("dex:TOKEN"),
        AssetQuery::Hip3 {
            dex: "dex".to_string(),
            token: "TOKEN".to_string()
        }
    );
}

#[test]
fn parses_outcome_notation_format() {
    assert_eq!(
        parse_asset_query("#10"),
        AssetQuery::Outcome("#10".to_string())
    );
    assert_eq!(
        parse_asset_query("+11"),
        AssetQuery::Outcome("+11".to_string())
    );
}

#[test]
fn resolves_btc_and_eth_to_perp_asset_indices() {
    let resolver = AssetResolver::new(fixture_metadata());

    assert_eq!(resolver.resolve_perp("BTC").unwrap(), perp("BTC", 0, None));
    assert_eq!(resolver.resolve_perp("eth").unwrap(), perp("ETH", 1, None));
}

#[test]
fn resolves_purr_usdc_to_spot_asset_index() {
    let resolver = AssetResolver::new(fixture_metadata());

    assert_eq!(resolver.resolve_spot("purr/usdc").unwrap(), purr_usdc());
}

#[test]
fn resolves_dex_token_to_hip3_asset_index() {
    let resolver = AssetResolver::new(fixture_metadata());

    assert_eq!(
        resolver.resolve_perp("dex:TOKEN").unwrap(),
        perp("TOKEN", 110_000, Some("dex"))
    );
}

#[test]
fn fuzzy_matching_returns_at_most_one_suggestion() {
    let names = ["BTC", "ETH", "BLUR", "BONK", "PURR/USDC"];
    let matches = suggestions("BT", names);

    assert_eq!(matches, vec!["BTC".to_string()]);
}

#[test]
fn fuzzy_matching_does_not_suggest_loose_matches_for_long_placeholder_names() {
    let names = ["BTC", "ETH", "BLUR", "BONK"];
    let matches = suggestions("INVALIDCOIN", names);

    assert!(matches.is_empty());
}

#[test]
fn fuzzy_matching_ignores_repeated_no_signal_inputs() {
    let names = ["ZEC", "BTC", "ETH"];
    let matches = suggestions("ZZZZZZZZ", names);

    assert!(matches.is_empty());
}

#[test]
fn invalid_asset_with_close_matches_exits_13() {
    let resolver = AssetResolver::new(fixture_metadata());
    let err = CliError::from(resolver.resolve_perp("BT").unwrap_err());

    assert_eq!(err.exit_code(), 13);
    match err {
        CliError::AssetNotFound { asset, suggestions } => {
            assert_eq!(asset, "BT");
            assert!(suggestions.contains("BTC"));
        }
        other => panic!("expected asset suggestions, got {other:?}"),
    }
}

#[test]
fn invalid_asset_without_close_matches_exits_13_without_suggestions() {
    let resolver = AssetResolver::new(fixture_metadata());
    let err = CliError::from(resolver.resolve_perp("ZZZZZZZZ").unwrap_err());

    assert_eq!(err.exit_code(), 13);
    assert!(matches!(err, CliError::AssetNotFoundNoSuggestion { .. }));
}

#[test]
fn outcome_notation_reports_outcome_aware_command_guidance_for_generic_resolver() {
    let resolver = AssetResolver::new(fixture_metadata());
    let err = CliError::from(resolver.resolve_perp("#10").unwrap_err());

    assert_eq!(err.exit_code(), 13);
    assert!(err.to_string().contains("outcome-aware command path"));
    assert!(err.to_string().contains("outcomes get #10"));
}

#[test]
fn cached_metadata_is_fresh_for_sixty_seconds() {
    let now = Instant::now();
    let cached = CachedMetadata::new(fixture_metadata(), now);

    assert!(cached.is_fresh_at(now + Duration::from_secs(59)));
    assert!(cached.is_fresh_at(now + Duration::from_secs(60)));
    assert!(!cached.is_fresh_at(now + Duration::from_secs(61)));
}
