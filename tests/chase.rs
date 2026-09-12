mod support;

use predicates::prelude::*;
use serde_json::Value;
use support::IsolatedHome;

#[test]
fn schema_orders_chase_supports_dry_run() {
    let output = IsolatedHome::new()
        .command()
        .args(["--format", "json", "schema", "orders", "chase"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["command"], "hyperliquid orders chase");
    assert_eq!(json["dry_run"], "optional");
    assert_eq!(json["confirmation"], "prompt");
}

#[test]
fn chase_requires_timeout_in_agent_mode() {
    IsolatedHome::new()
        .command()
        .env("HYPERLIQUID_AGENT", "1")
        .args([
            "--format",
            "json",
            "--dry-run",
            "--testnet",
            "orders",
            "chase",
            "--coin",
            "ETH",
            "--side",
            "buy",
            "--size",
            "0.5",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("--timeout 60s"))
        .stdout(predicate::str::contains(
            "hyperliquid --format json --dry-run orders chase --coin ETH --side buy --size 0.5 --timeout 60s",
        ));
}

#[test]
fn chase_help_mentions_offset_and_timeout() {
    IsolatedHome::new()
        .command()
        .args(["orders", "chase", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--offset"))
        .stdout(predicate::str::contains("--timeout"))
        .stdout(predicate::str::contains("--max-chase"))
        .stdout(predicate::str::contains("Examples:"))
        .stdout(predicate::str::contains("--dry-run"));
}
