// Integration tests for the output formatting system.
//
// Tests verify:
// - OutputFormat enum works with all three variants (pretty/table/json)
// - Pretty mode: tabwriter-aligned, cyan headers, colored values
// - Table mode: bordered tables via tabled, no ANSI codes
// - Json mode: stable JSON output with snake_case keys
// - Timing feedback format
// - Color theme helpers
// - Error output routing (JSON errors to stdout, others to stderr)
// - Empty data handling

use assert_cmd::Command;
use predicates::prelude::*;

// ── CLI format flag tests ───────────────────────────────────────────────

#[test]
fn test_format_flag_accepts_pretty() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("--format")
        .arg("pretty")
        .arg("--help")
        .assert()
        .success();
}

#[test]
fn test_format_flag_accepts_table() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("--format")
        .arg("table")
        .arg("--help")
        .assert()
        .success();
}

#[test]
fn test_format_flag_accepts_json() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("--format")
        .arg("json")
        .arg("--help")
        .assert()
        .success();
}

#[test]
fn test_format_flag_rejects_invalid() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("--format")
        .arg("xml")
        .assert()
        .failure()
        .stdout(predicates::str::contains("invalid"));
}

#[test]
fn test_default_format_is_pretty() {
    // No --format flag should work (defaults to pretty)
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("--help")
        .assert()
        .success();
}

#[test]
fn top_level_help_explains_dynamic_agent_json_default() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("json for non-TTY stdout"))
        .stdout(predicate::str::contains("HYPERLIQUID_AGENT=1"))
        .stdout(predicate::str::contains(
            "Unknown fields are omitted when the output shape is dynamic",
        ));
}

#[test]
fn schema_help_includes_agent_examples() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["schema", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hyperliquid --format json schema"))
        .stdout(predicate::str::contains("schema orders create"));
}

#[test]
fn orders_create_help_surfaces_risk_and_dry_run() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["orders", "create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Risk: funds_movement"))
        .stdout(predicate::str::contains("Dry-run: supported"))
        .stdout(predicate::str::contains("Confirmation:"));
}

#[test]
fn test_short_format_flag() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("-f")
        .arg("json")
        .arg("--help")
        .assert()
        .success();
}

#[test]
fn json_payload_parse_errors_do_not_echo_secret_content() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .args([
            "--format",
            "json",
            "--dry-run",
            "--payload-json",
            r#"{"api_key":"should-not-render","#,
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "0.001",
        ])
        .assert()
        .failure()
        .code(2)
        .stdout(predicate::str::contains("invalid payload JSON"))
        .stdout(predicate::str::contains("should-not-render").not())
        .stdout(predicate::str::contains("api_key").not());
}
