// Integration tests for the error system.
//
// Verifies:
// - Exit codes 0, 1, 2, 10, 11, 12, 13, 14, 15
// - JSON error envelope format {"error": "..."}
// - Stderr vs stdout error routing
// - Clap usage errors exit with code 2
// - Colored error output for pretty/table mode
// - hypersdk error mapping

use assert_cmd::Command;
use predicates::prelude::*;
// ── Exit code 0 — Success ────────────────────────────────────────────────

#[test]
fn test_exit_code_0_success() {
    // No args = success (prints version info)
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .assert()
        .success()
        .code(0);
}

#[test]
fn test_exit_code_0_version() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("hyperliquid"));
}

#[test]
fn test_exit_code_0_help() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .code(0);
}

// ── Exit code 2 — Usage error (clap) ─────────────────────────────────────

#[test]
fn test_exit_code_2_usage_error() {
    // Clap handles invalid format values with exit code 2
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("--format")
        .arg("xml")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn test_exit_code_2_invalid_format() {
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("--format")
        .arg("yaml")
        .assert()
        .failure()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["category"], "usage");
    assert!(json["error"].as_str().unwrap().contains("invalid"));
}

#[test]
fn test_exit_code_2_missing_subcommand_arg() {
    // `perps get` without a coin argument should exit 2 (clap)
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("perps")
        .arg("get")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn test_json_usage_errors_are_structured() {
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["--format", "json", "wallet", "show", "--bogus"])
        .assert()
        .failure()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["category"], "usage");
    assert_eq!(json["exit_code"], 2);
    assert!(json["error"].as_str().unwrap().contains("--bogus"));
}

#[test]
fn test_non_tty_usage_errors_are_structured_json_by_default() {
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["wallet", "show", "--bogus"])
        .assert()
        .failure()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["category"], "usage");
    assert_eq!(json["exit_code"], 2);
    assert!(json["error"].as_str().unwrap().contains("--bogus"));
}

#[test]
fn test_explicit_pretty_usage_errors_stay_human_readable() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["--format", "pretty", "wallet", "show", "--bogus"])
        .assert()
        .failure()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("--bogus"));
}

#[test]
fn test_hyperliquid_agent_usage_errors_are_structured_json() {
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .env("HYPERLIQUID_AGENT", "1")
        .args(["wallet", "show", "--bogus"])
        .assert()
        .failure()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["category"], "usage");
    assert_eq!(json["exit_code"], 2);
}

// ── Pretty mode: errors on stderr ────────────────────────────────────────

// ── Table mode: errors on stderr ─────────────────────────────────────────

// ── Short flag for format ────────────────────────────────────────────────

// ── Clap usage errors have exit code 2 ───────────────────────────────────

#[test]
fn test_clap_usage_error_exit_2() {
    // Invalid global flag value should be exit 2
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("--format")
        .arg("invalid")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn test_clap_usage_error_has_error_message() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .arg("--format")
        .arg("invalid")
        .assert()
        .failure()
        .stdout(predicate::str::contains("error").or(predicate::str::contains("invalid")));
}

// ── No args does not error ───────────────────────────────────────────────

#[test]
fn test_no_args_success() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("hyperliquid-cli"));
}

#[test]
fn experimental_ows_live_signing_fails_closed_with_unsupported_exit() {
    let address = "0x0000000000000000000000000000000000000001";
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .args([
            "--format",
            "json",
            "--ows-signer",
            address,
            "orders",
            "cancel",
            "123",
        ])
        .assert()
        .code(13)
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["category"], "unsupported");
    assert_eq!(json["exit_code"], 13);
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("does not have a resolved wallet for live signing")
    );
}

#[test]
fn experimental_ows_dry_run_does_not_require_signing() {
    let address = "0x0000000000000000000000000000000000000001";
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .args([
            "--format",
            "json",
            "--dry-run",
            "--ows-signer",
            address,
            "orders",
            "cancel",
            "123",
        ])
        .assert()
        .success();
}

#[test]
fn experimental_ows_user_signed_action_fails_closed_before_submit() {
    let address = "0x0000000000000000000000000000000000000001";
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .args([
            "--format",
            "json",
            "--ows-signer",
            address,
            "account",
            "abstraction",
            "set",
            "--mode",
            "disabled",
            "--yes",
        ])
        .assert()
        .code(13)
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["category"], "unsupported");
    assert_eq!(json["exit_code"], 13);
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("does not have a resolved wallet for live signing")
    );
}

#[test]
fn experimental_ows_l1_action_fails_closed_before_submit() {
    let address = "0x0000000000000000000000000000000000000001";
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .args([
            "--format",
            "json",
            "--ows-signer",
            address,
            "api-wallet",
            "approve",
            "--agent-address",
            "0x0000000000000000000000000000000000000002",
            "--name",
            "ows-smoke",
        ])
        .assert()
        .code(13)
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["category"], "unsupported");
    assert_eq!(json["exit_code"], 13);
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("does not have a resolved wallet for live signing")
    );
}
