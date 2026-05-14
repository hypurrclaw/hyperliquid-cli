//! First-use owner: #41/#42 payload and file-input contract freeze.
//!
//! These tests own unsafe local-file rejection and secret-redaction contracts
//! before registry migration starts routing more inputs through shared metadata.

mod support;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

use support::{IsolatedHome, TEST_ACCOUNT_PASSPHRASE, VALID_PRIVATE_KEY};

#[test]
fn payload_file_rejects_absolute_path_before_rendering() {
    let env = IsolatedHome::new();
    let payload = env.tmp_path().join("payload.json");
    std::fs::write(&payload, r#"{"token":"should-not-render"}"#).unwrap();

    env.command()
        .args([
            "--format",
            "pretty",
            "--dry-run",
            "--payload-file",
            payload.to_str().unwrap(),
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
        .stderr(predicate::str::contains(
            "payload path must be relative to the current working directory",
        ))
        .stderr(predicate::str::contains("should-not-render").not());
}

#[test]
fn orders_file_rejects_stdin_for_batch_create() {
    let env = IsolatedHome::new();

    env.command()
        .args([
            "--format",
            "pretty",
            "--dry-run",
            "orders",
            "batch-create",
            "--orders-file",
            "-",
            "--testnet",
        ])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains(
            "orders file path cannot read from stdin",
        ));
}

#[test]
fn orders_file_rejects_invalid_utf8_without_echoing_bytes() {
    let env = IsolatedHome::new();
    let cwd = tempfile::tempdir().unwrap();
    std::fs::write(cwd.path().join("orders.json"), [0xff, b's', b'e', b'c']).unwrap();

    env.command()
        .current_dir(cwd.path())
        .args([
            "--format",
            "pretty",
            "--dry-run",
            "orders",
            "batch-create",
            "--orders-file",
            "orders.json",
            "--testnet",
        ])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains(
            "orders file must be valid UTF-8 JSON",
        ))
        .stderr(predicate::str::contains("sec").not());
}

#[cfg(unix)]
#[test]
fn orders_file_rejects_symlink_before_reading() {
    let env = IsolatedHome::new();
    let cwd = tempfile::tempdir().unwrap();
    std::fs::write(cwd.path().join("orders.json"), r#"{"orders":[]}"#).unwrap();
    std::os::unix::fs::symlink("orders.json", cwd.path().join("orders-link.json")).unwrap();

    env.command()
        .current_dir(cwd.path())
        .args([
            "--format",
            "pretty",
            "--dry-run",
            "orders",
            "batch-create",
            "--orders-file",
            "orders-link.json",
            "--testnet",
        ])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains(
            "orders file path cannot be a symlink",
        ));
}

#[test]
fn payload_json_rejects_terminal_controls_and_oversized_strings() {
    let env = IsolatedHome::new();
    let control_payload = r#"{"note":"bad\u001b"}"#;

    env.command()
        .args([
            "--format",
            "pretty",
            "--dry-run",
            "--payload-json",
            control_payload,
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
        .stderr(predicate::str::contains(
            "payload JSON cannot contain terminal control characters",
        ));

    let oversized_payload = format!(r#"{{"note":"{}"}}"#, "x".repeat(70_000));
    env.command()
        .arg("--format")
        .arg("pretty")
        .arg("--dry-run")
        .arg("--payload-json")
        .arg(oversized_payload)
        .args([
            "orders", "create", "--coin", "BTC", "--side", "buy", "--price", "50000", "--size",
            "0.001",
        ])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains(
            "payload JSON contains an oversized string",
        ));
}

#[test]
fn update_rejects_raw_payload_inputs_without_network() {
    let env = IsolatedHome::new();

    env.command()
        .args([
            "--format",
            "pretty",
            "--dry-run",
            "--payload-json",
            "{}",
            "update",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "payload input is not supported for update",
        ));
}

#[test]
fn raw_payload_is_rejected_before_local_or_read_only_commands_can_ignore_it() {
    let env = IsolatedHome::new();

    for args in [
        vec!["--format", "json", "--payload-json", "{}", "schema"],
        vec!["--format", "json", "--payload-json", "{}", "wallet", "list"],
        vec!["--format", "json", "--payload-json", "{}", "perps", "list"],
    ] {
        let output = env
            .command()
            .args(args)
            .assert()
            .failure()
            .code(13)
            .get_output()
            .stdout
            .clone();
        let json: Value = serde_json::from_slice(&output).unwrap();
        assert!(
            json["error"]
                .as_str()
                .unwrap()
                .contains("payload input is not supported"),
            "{json}"
        );
    }
}

#[test]
fn prompt_required_commands_fail_closed_in_machine_context_without_confirmation_bypass() {
    let env = IsolatedHome::new();

    for args in [
        vec!["--format", "json", "wallet", "delete", "qa-wallet"],
        vec![
            "--format", "json", "orders", "create", "--coin", "BTC", "--side", "buy", "--price",
            "50000", "--size", "0.001",
        ],
        vec![
            "--format",
            "json",
            "transfer",
            "send",
            "--to",
            "0x0000000000000000000000000000000000000001",
            "--amount",
            "1",
        ],
    ] {
        let output = env
            .command()
            .args(args)
            .assert()
            .failure()
            .code(13)
            .get_output()
            .stdout
            .clone();
        let json: Value = serde_json::from_slice(&output).unwrap();
        assert!(
            json["error"]
                .as_str()
                .unwrap()
                .contains("requires confirmation in machine-readable contexts"),
            "{json}"
        );
    }
}

#[test]
fn prompt_required_commands_fail_closed_for_agent_env_even_with_explicit_pretty_format() {
    let env = IsolatedHome::new();

    env.command()
        .env("HYPERLIQUID_AGENT", "1")
        .args(["--format", "pretty", "wallet", "delete", "qa-wallet"])
        .assert()
        .failure()
        .code(13)
        .stderr(predicate::str::contains(
            "requires confirmation in machine-readable contexts",
        ));
}

#[test]
fn wallet_export_secret_stdout_is_not_enabled_by_yes_in_machine_context() {
    let env = IsolatedHome::new();

    let output = env
        .command()
        .args(["--format", "json", "wallet", "export", "qa-wallet", "--yes"])
        .assert()
        .failure()
        .code(13)
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("requires confirmation in machine-readable contexts"),
        "{json}"
    );
}

#[test]
fn secret_prompt_commands_fail_closed_in_machine_context() {
    let env = IsolatedHome::new();

    for args in [
        vec!["--format", "json", "account", "add"],
        vec![
            "--format",
            "json",
            "account",
            "add",
            VALID_PRIVATE_KEY,
            "--alias",
            "argv-secret",
        ],
        vec![
            "--format",
            "json",
            "--dry-run",
            "account",
            "add",
            VALID_PRIVATE_KEY,
            "--alias",
            "argv-secret",
        ],
        vec!["--format", "json", "wallet", "import"],
        vec!["--format", "json", "wallet", "import", VALID_PRIVATE_KEY],
        vec![
            "--format",
            "json",
            "--dry-run",
            "wallet",
            "import",
            VALID_PRIVATE_KEY,
        ],
        vec!["--format", "json", "wallet", "import-mnemonic"],
        vec![
            "--format",
            "json",
            "wallet",
            "import-mnemonic",
            "test test test test test test test test test test test junk",
        ],
        vec![
            "--format",
            "json",
            "--dry-run",
            "wallet",
            "import-mnemonic",
            "test test test test test test test test test test test junk",
        ],
    ] {
        let output = env
            .command()
            .args(args)
            .assert()
            .failure()
            .code(13)
            .get_output()
            .stdout
            .clone();
        let json: Value = serde_json::from_slice(&output).unwrap();
        let error = json["error"].as_str().unwrap();
        assert!(
            error.contains("requires confirmation in machine-readable contexts")
                || error.contains("argv secret input is not supported"),
            "{json}"
        );
    }
}

#[test]
fn prompt_required_dry_runs_remain_machine_readable_previews() {
    let env = IsolatedHome::new();

    let output = env
        .command()
        .args([
            "--format",
            "json",
            "--dry-run",
            "wallet",
            "delete",
            "qa-wallet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["command"], "wallet delete");
    assert_eq!(json["dry_run"], true);
}

#[test]
fn registry_file_inputs_are_schema_marked_and_policy_covered() {
    assert_schema_input_kind("orders batch-create", "orders_file");
}

#[test]
fn transfer_recipient_address_fields_reject_stored_account_aliases() {
    let env = IsolatedHome::new();
    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "account",
            "add",
            VALID_PRIVATE_KEY,
            "--alias",
            "recipient-alias",
        ])
        .assert()
        .success();

    for args in [
        vec![
            "--format",
            "json",
            "--dry-run",
            "transfer",
            "send",
            "--to",
            "recipient-alias",
            "--amount",
            "1",
        ],
        vec![
            "--format",
            "json",
            "--dry-run",
            "transfer",
            "spot-send",
            "--to",
            "recipient-alias",
            "--token",
            "HYPE",
            "--amount",
            "1",
        ],
        vec![
            "--format",
            "json",
            "--dry-run",
            "transfer",
            "send-asset",
            "--to",
            "recipient-alias",
            "--source",
            "spot",
            "--dest",
            "perp",
            "--token",
            "USDC",
            "--amount",
            "1",
        ],
        vec![
            "--format",
            "json",
            "--dry-run",
            "transfer",
            "withdraw",
            "--to",
            "recipient-alias",
            "--amount",
            "1",
        ],
    ] {
        let output = env
            .account_command(TEST_ACCOUNT_PASSPHRASE)
            .args(args)
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let json: Value = serde_json::from_slice(&output).unwrap();
        assert!(
            json["error"]
                .as_str()
                .unwrap()
                .to_ascii_lowercase()
                .contains("address"),
            "{json}"
        );
    }
}

#[test]
fn staking_and_vault_protocol_object_address_fields_reject_stored_account_aliases() {
    let env = IsolatedHome::new();
    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "account",
            "add",
            VALID_PRIVATE_KEY,
            "--alias",
            "object-alias",
        ])
        .assert()
        .success();

    for args in [
        vec![
            "--format",
            "json",
            "--dry-run",
            "staking",
            "delegate",
            "--validator",
            "object-alias",
            "--amount",
            "1",
            "--testnet",
        ],
        vec![
            "--format",
            "json",
            "--dry-run",
            "staking",
            "link",
            "initiate",
            "--user",
            "object-alias",
            "--testnet",
        ],
        vec![
            "--format",
            "json",
            "--dry-run",
            "vault",
            "deposit",
            "--vault",
            "object-alias",
            "--amount",
            "1",
            "--testnet",
        ],
        vec![
            "--format",
            "json",
            "--dry-run",
            "vault",
            "withdraw",
            "--vault",
            "object-alias",
            "--amount",
            "1",
            "--testnet",
        ],
    ] {
        let output = env
            .account_command(TEST_ACCOUNT_PASSPHRASE)
            .args(args)
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let json: Value = serde_json::from_slice(&output).unwrap();
        assert!(
            json["error"]
                .as_str()
                .unwrap()
                .to_ascii_lowercase()
                .contains("address"),
            "{json}"
        );
    }
}

#[test]
fn position_margin_precision_is_validated_before_auth_or_network() {
    let env = IsolatedHome::new();

    env.command()
        .args([
            "positions",
            "update-margin",
            "--coin",
            "BTC",
            "--amount",
            "0.0000001",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "amount supports at most 6 decimal places",
        ))
        .stderr(predicate::str::contains("Authentication required").not());
}

fn assert_schema_input_kind(command: &str, arg: &str) {
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["--format", "json", "schema"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let schemas: Vec<Value> = serde_json::from_slice(&output).unwrap();
    let command_prefix = format!("hyperliquid {command}");
    let schema = schemas
        .iter()
        .find(|schema| {
            schema["command"].as_str().is_some_and(|name| {
                name == command_prefix || name.starts_with(&format!("{command_prefix} "))
            })
        })
        .unwrap_or_else(|| panic!("missing schema for {command}"));
    assert_eq!(
        schema["json_schema"]["properties"][arg]["input_kind"], "file_path",
        "{command} {arg} must stay covered by shared file policy tests"
    );
}
