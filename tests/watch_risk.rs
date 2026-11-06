mod support;

use predicates::prelude::*;
use serde_json::Value;
use support::{IsolatedHome, mount_account_state, mount_all_mids};
use wiremock::MockServer;

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

#[test]
fn watch_risk_rejects_zero_idle_timeout_like_subscribe() {
    IsolatedHome::new()
        .command()
        .args([
            "watch",
            "risk",
            "--user",
            "0x0000000000000000000000000000000000000001",
            "--idle-timeout-ms",
            "0",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--idle-timeout-ms"));
}

async fn watch_risk_lines(global_args: &[&str]) -> Vec<Value> {
    let server = MockServer::start().await;
    mount_account_state(&server, vec![], vec![], "9000", "0", 10).await;
    mount_all_mids(&server, "50000", "3000").await;

    let output = IsolatedHome::new()
        .command_with_server(&server)
        .args(global_args)
        .args([
            "watch",
            "risk",
            "--user",
            "0x0000000000000000000000000000000000000001",
            "--only",
            "position_lifecycle",
            "--max-events",
            "1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn watch_risk_json_emits_full_alert_without_projection_flags() {
    let lines = watch_risk_lines(&["--format", "json"]).await;

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["kind"], "alert");
    assert_eq!(lines[0]["rule"], "position_lifecycle");
    assert_eq!(lines[0]["coin"], "BTC");
    assert!(lines[0].get("payload").is_some());
}

#[tokio::test]
async fn watch_risk_json_honors_select() {
    let lines = watch_risk_lines(&["--format", "json", "--select", "rule,coin"]).await;

    assert_eq!(lines.len(), 1);
    assert_eq!(
        lines[0],
        serde_json::json!({"rule": "position_lifecycle", "coin": "BTC"})
    );
}

#[tokio::test]
async fn watch_risk_json_honors_results_only_as_a_harmless_flag() {
    let lines = watch_risk_lines(&["--format", "json", "--results-only"]).await;

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["rule"], "position_lifecycle");
}
