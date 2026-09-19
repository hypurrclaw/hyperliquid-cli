mod support;

use predicates::prelude::*;
use serde_json::Value;
use support::IsolatedHome;

#[test]
fn schema_orders_bracket_supports_dry_run() {
    let output = IsolatedHome::new()
        .command()
        .args(["--format", "json", "schema", "orders", "bracket"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["command"], "hyperliquid orders bracket");
    assert_eq!(json["dry_run"], "optional");
    assert_eq!(json["confirmation"], "prompt");
}

#[test]
fn bracket_requires_size_or_amount() {
    IsolatedHome::new()
        .command()
        .args([
            "--dry-run",
            "--testnet",
            "orders",
            "bracket",
            "--coin",
            "ETH",
            "--side",
            "buy",
            "--take-profit",
            "+10%",
            "--stop-loss",
            "-5%",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--size").or(predicate::str::contains("--amount")))
        .stderr(predicate::str::contains(
            "hyperliquid --format json --dry-run orders bracket --coin ETH --side buy --size 0.1 --take-profit +10% --stop-loss -5%",
        ));
}

#[test]
fn buy_help_is_registered() {
    IsolatedHome::new()
        .command()
        .args(["buy", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--coin"))
        .stdout(predicate::str::contains("--size"))
        .stdout(predicate::str::contains("Examples:"))
        .stdout(predicate::str::contains("--dry-run"));
}

#[test]
fn orders_bracket_help_includes_examples() {
    IsolatedHome::new()
        .command()
        .args(["orders", "bracket", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Examples:"))
        .stdout(predicate::str::contains("--dry-run"));
}
