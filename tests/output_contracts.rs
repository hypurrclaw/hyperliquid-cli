//! First-use owner: #42 command contract freeze.
//!
//! These tests own stable top-level JSON/error/output contracts that are not
//! specific to one command family.

mod contract_support;
mod support;

use assert_cmd::Command;
use serde_json::{Value, json};

use contract_support::assert_json_fixture;
use support::{API_OVERRIDE_ENV, IsolatedHome, TEST_ACCOUNT_PASSPHRASE, mock_market_server};

#[test]
fn json_error_envelope_matches_characterization_fixture() {
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["--format", "json", "wallet", "show", "--bogus"])
        .assert()
        .failure()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let error: Value = serde_json::from_slice(&output).unwrap();

    assert_json_fixture(
        "output_error_envelope.json",
        &json!({
            "characterization": true,
            "review_required_to_update": true,
            "error": error,
        }),
    );
}

#[tokio::test]
async fn results_only_and_select_output_matches_characterization_fixture() {
    let env = IsolatedHome::new();
    let server = mock_market_server().await;
    let mut command = env.account_command(TEST_ACCOUNT_PASSPHRASE);
    let output = command
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format",
            "json",
            "--results-only",
            "--max-results",
            "2",
            "--select",
            "name,max_leverage",
            "perps",
            "list",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: Value = serde_json::from_slice(&output).unwrap();

    assert_json_fixture(
        "output_projection.json",
        &json!({
            "characterization": true,
            "review_required_to_update": true,
            "output": value,
        }),
    );
}
