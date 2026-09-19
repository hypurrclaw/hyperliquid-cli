mod support;

use predicates::prelude::*;
use serde_json::Value;
use support::IsolatedHome;

#[test]
fn schema_watch_risk_is_read_only() {
    let output = IsolatedHome::new()
        .command()
        .args(["--format", "json", "schema", "watch", "risk"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["command"], "hyperliquid watch risk");
    assert_eq!(json["lifecycle"], "read_only");
    assert_eq!(json["auth_required"], false);
    assert_eq!(json["dry_run"], "not_supported");
}

#[test]
fn watch_risk_requires_bounds_in_agent_mode() {
    IsolatedHome::new()
        .command()
        .env("HYPERLIQUID_AGENT", "1")
        .args([
            "--format",
            "json",
            "watch",
            "risk",
            "--user",
            "0x0000000000000000000000000000000000000001",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("--max-events 20"))
        .stdout(predicate::str::contains(
            "hyperliquid --format json watch risk --user 0x0000000000000000000000000000000000000001 --max-events 20",
        ));
}

#[test]
fn watch_risk_help_includes_examples() {
    IsolatedHome::new()
        .command()
        .args(["watch", "risk", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Examples:"))
        .stdout(predicate::str::contains("--max-events 20"));
}
