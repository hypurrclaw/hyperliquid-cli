mod support;

use predicates::prelude::*;
use serde_json::Value;
use std::fs;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use support::{
    IsolatedHome, TEST_ACCOUNT_PASSPHRASE, expected_address, mock_all_mids_server,
    mock_all_mids_server_with_prices,
};

const IMPORT_KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000006";

fn assert_file_does_not_contain(path: &std::path::Path, needle: &str) {
    let bytes = fs::read(path).unwrap();
    let contents = String::from_utf8_lossy(&bytes);
    assert!(
        !contents.contains(needle),
        "{} must not contain sensitive material",
        path.display()
    );
}

fn assert_directory_does_not_contain(path: &std::path::Path, needle: &str) {
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                stack.push(path);
            } else {
                let bytes = fs::read(&path).unwrap();
                let contents = String::from_utf8_lossy(&bytes);
                assert!(
                    !contents.contains(needle),
                    "{} must not contain sensitive material",
                    path.display()
                );
            }
        }
    }
}

#[tokio::test]
async fn setup_create_wallet_saves_config_and_verifies_connection() {
    let env = IsolatedHome::new();
    let server = mock_all_mids_server().await;

    let output = env
        .account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .arg("setup")
        .write_stdin("1\nn\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Welcome to Hyperliquid CLI setup"))
        .stdout(predicate::str::contains("Create new wallet"))
        .stdout(predicate::str::contains("Config saved"))
        .stdout(predicate::str::contains("Test query succeeded"))
        .stdout(predicate::str::contains("Setup complete"))
        .get_output()
        .stdout
        .clone();

    let stdout = String::from_utf8(output).unwrap();
    let address = stdout
        .split_whitespace()
        .find(|part| part.starts_with("0x"))
        .expect("setup should print generated wallet address")
        .to_string();

    let config_path = env.config_file_path();
    let config = fs::read_to_string(&config_path).unwrap();
    assert!(config.contains("\"network\": \"mainnet\""));
    assert!(!config.contains("private_key"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .args(["wallet", "address"])
        .assert()
        .success()
        .stdout(predicate::str::contains(address));
}

#[tokio::test]
async fn setup_yes_creates_wallet_and_persists_default_builder_and_referral() {
    let env = IsolatedHome::new();
    let server = mock_all_mids_server().await;
    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_string_contains(r#""type":"approveBuilderFee""#))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": { "type": "default" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .env(
            "HYPERLIQUID_DEFAULT_BUILDER_ADDRESS",
            "0x00000000000000000000000000000000000000bb",
        )
        .env("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE", "0.001%")
        .env("HYPERLIQUID_DEFAULT_REFERRAL_CODE", "SETUPYES")
        .args(["setup", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Setup complete"))
        .stdout(predicate::str::contains("Default builder"))
        .stdout(predicate::str::contains("Default referral"))
        .stdout(predicate::str::contains("Builder approval submitted"))
        .stdout(predicate::str::contains("Choose an option").not())
        .stdout(predicate::str::contains("Default builder address").not())
        .stdout(predicate::str::contains("Default referral code").not());

    let config: Value = serde_json::from_str(&fs::read_to_string(env.config_file_path()).unwrap())
        .expect("setup -y should write valid config JSON");
    assert_eq!(config["network"], "mainnet");
    assert!(config["default_wallet_id"].as_str().is_some());
    assert_eq!(
        config["default_builder_address"],
        "0x00000000000000000000000000000000000000bb"
    );
    assert_eq!(config["default_builder_fee_rate"], "0.001%");
    assert_eq!(config["default_referral_code"], "SETUPYES");
}

#[tokio::test]
async fn setup_import_wallet_uses_hidden_prompt_path_and_stores_key_encrypted() {
    let env = IsolatedHome::new();
    let server = mock_all_mids_server().await;
    let address = expected_address(IMPORT_KEY);

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .arg("setup")
        .write_stdin(format!("2\ny\n\n\n{IMPORT_KEY}\n"))
        .assert()
        .success()
        .stdout(predicate::str::contains("Private key:"))
        .stdout(predicate::str::contains("Test query succeeded"))
        .stdout(predicate::str::contains(&address))
        .stdout(predicate::str::contains(IMPORT_KEY).not());

    let config_path = env.config_file_path();
    let config = fs::read_to_string(&config_path).unwrap();
    assert!(config.contains("\"network\": \"testnet\""));
    assert!(!config.contains("private_key"));

    let vault_path = env.ows_vault_path();
    assert_directory_does_not_contain(&vault_path, IMPORT_KEY);
    assert_directory_does_not_contain(&vault_path, IMPORT_KEY.trim_start_matches("0x"));
    assert!(
        !env.legacy_accounts_key_path().exists(),
        "raw encryption key must not be stored next to accounts.db"
    );

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .args(["wallet", "address"])
        .assert()
        .success()
        .stdout(predicate::str::is_match(format!("^{address}\n$")).unwrap());
}

#[tokio::test]
async fn setup_connection_failure_has_clear_error_and_nonzero_exit() {
    let env = IsolatedHome::new();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .respond_with(ResponseTemplate::new(503).set_body_string("maintenance"))
        .mount(&server)
        .await;

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .arg("setup")
        .write_stdin("1\nn\n")
        .assert()
        .code(12)
        .stderr(predicate::str::contains("Unable to reach Hyperliquid API"))
        .stderr(predicate::str::contains("Check your network connection"));

    assert!(
        env.config_file_candidates()
            .iter()
            .all(|path| !path.exists()),
        "failed verification must not leave config.json behind"
    );
    assert!(
        env.ows_vault_candidates().iter().all(|path| !path.exists()),
        "failed verification must not leave OWS vault behind"
    );
}

#[tokio::test]
async fn setup_selecting_testnet_verifies_against_testnet_endpoint() {
    let env = IsolatedHome::new();
    let mainnet_server = mock_all_mids_server_with_prices("50000.0", "3000.0").await;
    let testnet_server = mock_all_mids_server_with_prices("51000.0", "3100.0").await;

    env.account_command_with_mainnet_and_testnet(
        TEST_ACCOUNT_PASSPHRASE,
        &mainnet_server,
        &testnet_server,
    )
    .arg("setup")
    .write_stdin("1\ny\n")
    .assert()
    .success()
    .stdout(predicate::str::contains("Test query succeeded"));

    let mainnet_requests = mainnet_server.received_requests().await.unwrap();
    let testnet_requests = testnet_server.received_requests().await.unwrap();
    assert!(
        mainnet_requests.is_empty(),
        "testnet setup must not verify against the mainnet endpoint"
    );
    assert_eq!(
        testnet_requests.len(),
        1,
        "testnet setup must verify exactly once against the selected testnet endpoint"
    );
}

#[tokio::test]
async fn setup_selecting_mainnet_verifies_against_mainnet_endpoint() {
    let env = IsolatedHome::new();
    let mainnet_server = mock_all_mids_server_with_prices("50000.0", "3000.0").await;
    let testnet_server = mock_all_mids_server_with_prices("51000.0", "3100.0").await;

    env.account_command_with_mainnet_and_testnet(
        TEST_ACCOUNT_PASSPHRASE,
        &mainnet_server,
        &testnet_server,
    )
    .arg("setup")
    .write_stdin("1\nn\n")
    .assert()
    .success()
    .stdout(predicate::str::contains("Test query succeeded"));

    let mainnet_requests = mainnet_server.received_requests().await.unwrap();
    let testnet_requests = testnet_server.received_requests().await.unwrap();
    assert_eq!(
        mainnet_requests.len(),
        1,
        "mainnet setup must verify exactly once against the selected mainnet endpoint"
    );
    assert!(
        testnet_requests.is_empty(),
        "mainnet setup must not verify against the testnet endpoint"
    );
}
