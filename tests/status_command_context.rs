use hyperliquid_cli::command_context::{
    CommandContext, CommandOutputContext, CommandTransportPolicy,
};
use hyperliquid_cli::commands::status::StatusOutput;
use hyperliquid_cli::output::{self, OutputFormat};

fn sample_status_output() -> StatusOutput {
    StatusOutput {
        network: "mainnet".to_string(),
        api_url: "https://api.hyperliquid.xyz".to_string(),
        health: "healthy".to_string(),
        latency_ms: 42,
        perps_count: 100,
        mids_count: 100,
        rate_limit_status: "ok".to_string(),
        rate_limit_note: "no rate-limit response observed".to_string(),
    }
}

#[test]
fn status_context_json_projection_does_not_leak_from_global_options() {
    output::set_json_options_with_limit(Some("health"), false, None);

    let context = CommandContext::new(
        "mainnet",
        "https://api.hyperliquid.xyz",
        CommandOutputContext::new(OutputFormat::Json, Some("network,latency_ms"), false, None),
        CommandTransportPolicy::CliProcess,
    );
    let rendered = context.render(&sample_status_output());
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json["network"], "mainnet");
    assert_eq!(json["latency_ms"], 42);
    assert!(json.get("health").is_none());

    output::set_json_options_with_limit(None, false, None);
}
