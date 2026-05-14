use hyperliquid_cli::command_context::{
    CommandContext, CommandOutputContext, CommandTransportPolicy,
};
use hyperliquid_cli::commands::account::{AbstractionOutput, AbstractionRow, FillRow, FillsOutput};
use hyperliquid_cli::output::{self, OutputFormat};
use rust_decimal::Decimal;

fn json_context(select: Option<&str>, max_results: Option<usize>) -> CommandContext<'static> {
    CommandContext::new(
        "mainnet",
        "https://api.hyperliquid.xyz",
        CommandOutputContext::new(OutputFormat::Json, select, false, max_results),
        CommandTransportPolicy::CliProcess,
    )
}

#[test]
fn account_fills_context_json_projection_does_not_leak_from_global_options() {
    output::set_json_options_with_limit(Some("fee"), false, None);

    let output = FillsOutput::new(vec![FillRow {
        coin: "BTC".to_string(),
        side: "buy".to_string(),
        price: Decimal::from(50_000),
        size: Decimal::new(1, 3),
        direction: "Open Long".to_string(),
        closed_pnl: Decimal::ZERO,
        fee: Decimal::new(10, 2),
        fee_token: "USDC".to_string(),
        oid: 42,
        time: 1_700_000_000_000,
        liquidity: "taker".to_string(),
        trade_id: 7,
        cloid: None,
    }]);
    let rendered = json_context(Some("coin,time"), None).render(&output);
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json[0]["coin"], "BTC");
    assert_eq!(json[0]["time"], 1_700_000_000_000_u64);
    assert!(json[0].get("fee").is_none());

    output::set_json_options_with_limit(None, false, None);
}

#[test]
fn account_abstraction_context_uses_explicit_projection() {
    output::set_json_options_with_limit(Some("raw_mode"), false, None);

    let output = AbstractionOutput {
        row: AbstractionRow {
            user: "0x0000000000000000000000000000000000000001".to_string(),
            raw_mode: "unifiedAccount".to_string(),
            normalized_mode: "unified-account".to_string(),
        },
    };
    let rendered = json_context(Some("user,normalized_mode"), None).render(&output);
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json["user"], "0x0000000000000000000000000000000000000001");
    assert_eq!(json["normalized_mode"], "unified-account");
    assert!(json.get("raw_mode").is_none());

    output::set_json_options_with_limit(None, false, None);
}
