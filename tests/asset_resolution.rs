use std::time::{Duration, Instant};

use hyperliquid_cli::commands::{
    AssetMetadata, AssetQuery, AssetResolver, CachedMetadata, PerpAsset, ResolvedAsset, SpotAsset,
    parse_asset_query, suggestions,
};
use hyperliquid_cli::errors::CliError;

fn fixture_metadata() -> AssetMetadata {
    AssetMetadata::from_assets(
        vec![
            PerpAsset::default_dex("BTC", 0),
            PerpAsset::default_dex("ETH", 1),
            PerpAsset::default_dex("BLUR", 2),
            PerpAsset::default_dex("BONK", 3),
            PerpAsset::hip3("dex", "TOKEN", 110_000),
        ],
        vec![SpotAsset::new("PURR/USDC", 10_000, "PURR", "USDC")],
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

    assert_eq!(
        resolver.resolve_perp("BTC").unwrap(),
        ResolvedAsset::Perp {
            name: "BTC".to_string(),
            index: 0,
            dex: None,
            sz_decimals: 8,
            collateral: "USDC".to_string()
        }
    );
    assert_eq!(
        resolver.resolve_perp("eth").unwrap(),
        ResolvedAsset::Perp {
            name: "ETH".to_string(),
            index: 1,
            dex: None,
            sz_decimals: 8,
            collateral: "USDC".to_string()
        }
    );
}

#[test]
fn resolves_purr_usdc_to_spot_asset_index() {
    let resolver = AssetResolver::new(fixture_metadata());

    assert_eq!(
        resolver.resolve_spot("purr/usdc").unwrap(),
        ResolvedAsset::Spot {
            symbol: "PURR/USDC".to_string(),
            index: 10_000,
            base: "PURR".to_string(),
            quote: "USDC".to_string(),
            base_sz_decimals: 8
        }
    );
}

#[test]
fn resolves_dex_token_to_hip3_asset_index() {
    let resolver = AssetResolver::new(fixture_metadata());

    assert_eq!(
        resolver.resolve_perp("dex:TOKEN").unwrap(),
        ResolvedAsset::Perp {
            name: "TOKEN".to_string(),
            index: 110_000,
            dex: Some("dex".to_string()),
            sz_decimals: 8,
            collateral: "USDC".to_string()
        }
    );
}

#[test]
fn fuzzy_matching_returns_top_three_suggestions() {
    let names = ["BTC", "ETH", "BLUR", "BONK", "PURR/USDC"];
    let matches = suggestions("BT", names);

    assert!(matches.len() <= 3);
    assert_eq!(matches[0], "BTC");
    assert!(matches.contains(&"BLUR".to_string()) || matches.contains(&"BONK".to_string()));
}

#[test]
fn fuzzy_matching_can_suggest_for_long_placeholder_coin_names() {
    let names = ["BTC", "ETH", "BLUR", "BONK"];
    let matches = suggestions("INVALIDCOIN", names);

    assert!(matches.len() <= 3);
    assert!(matches.contains(&"BTC".to_string()));
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
    let err = resolver.resolve_perp("BT").unwrap_err();

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
    let err = resolver.resolve_perp("ZZZZZZZZ").unwrap_err();

    assert_eq!(err.exit_code(), 13);
    assert!(matches!(err, CliError::AssetNotFoundNoSuggestion { .. }));
}

#[test]
fn outcome_notation_reports_outcome_aware_command_guidance_for_generic_resolver() {
    let resolver = AssetResolver::new(fixture_metadata());
    let err = resolver.resolve("#10").unwrap_err();

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
