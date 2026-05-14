mod support;

use predicates::prelude::*;
use serde_json::Value;
use support::{
    API_OVERRIDE_ENV, IsolatedHome, PRIVATE_KEY_ENV, TEST_ACCOUNT_PASSPHRASE, VALID_PRIVATE_KEY,
    expected_address, fixture_order_success_response, mount_all_mids,
    mount_common_public_endpoints, mount_successful_exchange_actions,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";
const SUBACCOUNT_ADDRESS: &str = "0x0000000000000000000000000000000000000001";
const SUBACCOUNT_KEY: &str = "0x000000000000000000000000000000000000000000000000000000000000000a";

async fn mock_subaccount_exchange_server() -> MockServer {
    let server = MockServer::start().await;
    mount_all_mids(&server, "50000", "3000").await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {"type": "default"}
        })))
        .mount(&server)
        .await;

    server
}

fn exchange_body(requests: &[wiremock::Request]) -> Value {
    let exchange_request = requests
        .iter()
        .find(|request| request.url.path() == "/exchange")
        .expect("expected /exchange request");
    serde_json::from_slice(&exchange_request.body).unwrap()
}

#[test]
fn subaccount_create_dry_run_includes_signing_context() {
    let env = IsolatedHome::new();
    let output = env
        .command()
        .args([
            "--format",
            "json",
            "--dry-run",
            "subaccount",
            "create",
            "--name",
            "market-maker-1",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["command"], "subaccount create");
    assert_eq!(json["would_execute"], "create_subaccount");
    assert_eq!(json["args"]["name"], "market-maker-1");
    assert_eq!(json["args"]["network"], "Mainnet");
    assert_eq!(json["args"]["reversibility"], "irreversible");
    assert!(json["signer"].is_null());
    assert!(json["acting_as"].is_null());
    assert!(json["vault_address"].is_null());
}

#[test]
fn subaccount_transfer_dry_run_converts_usdc_to_wire_units() {
    let env = IsolatedHome::new();
    let output = env
        .command()
        .args([
            "--format",
            "json",
            "--dry-run",
            "subaccount",
            "transfer",
            "--subaccount",
            ZERO_ADDRESS,
            "--amount",
            "10",
            "--direction",
            "deposit",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["command"], "subaccount transfer");
    assert_eq!(json["would_execute"], "subaccount_usdc_transfer");
    assert_eq!(json["args"]["subaccount"], ZERO_ADDRESS);
    assert_eq!(json["args"]["amount"], "10");
    assert_eq!(json["args"]["direction"], "deposit");
    assert_eq!(json["args"]["is_deposit"], true);
    assert_eq!(json["args"]["usd"], 10_000_000);
    assert_eq!(json["args"]["asset"], "USDC");
    assert_eq!(json["args"]["network"], "Mainnet");
    assert_eq!(json["args"]["reversibility"], "partially_reversible");
    assert!(json["signer"].is_null());
    assert!(json["acting_as"].is_null());
    assert!(json["vault_address"].is_null());
}

#[test]
fn subaccount_spot_transfer_dry_run_preserves_master_and_subaccount_contexts() {
    let env = IsolatedHome::new();
    let token = "PURR:0xc4bf3f870c0e9465323c0b6ed28096c2";
    let output = env
        .command()
        .args([
            "--format",
            "json",
            "--dry-run",
            "subaccount",
            "spot-transfer",
            "--subaccount",
            SUBACCOUNT_ADDRESS,
            "--token",
            token,
            "--amount",
            "1.2300",
            "--direction",
            "withdraw",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["command"], "subaccount spot-transfer");
    assert_eq!(json["would_execute"], "subaccount_spot_transfer");
    assert_eq!(json["args"]["subaccount"], SUBACCOUNT_ADDRESS);
    assert_eq!(json["args"]["token"], token);
    assert_eq!(json["args"]["amount"], "1.2300");
    assert_eq!(json["args"]["direction"], "withdraw");
    assert_eq!(json["args"]["is_deposit"], false);
    assert_eq!(json["args"]["asset"], token);
    assert_eq!(json["args"]["network"], "Mainnet");
    assert_eq!(json["args"]["reversibility"], "partially_reversible");
    assert!(json["signer"].is_null());
    assert!(json["acting_as"].is_null());
    assert!(json["vault_address"].is_null());
}

#[tokio::test]
async fn subaccount_create_submits_sdk_matching_action_shape() {
    let env = IsolatedHome::new();
    let server = mock_subaccount_exchange_server().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "subaccount",
            "create",
            "--name",
            "market-maker-1",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("market-maker-1"));

    let requests = server.received_requests().await.unwrap();
    let body = exchange_body(&requests);
    assert_eq!(body["action"]["type"], "createSubAccount");
    assert_eq!(body["action"]["name"], "market-maker-1");
    assert!(body["nonce"].is_u64());
    assert!(body["signature"].is_object());
    assert!(body["vaultAddress"].is_null());
}

#[tokio::test]
async fn subaccount_transfer_resolves_alias_and_uses_usdc_integer_units() {
    let env = IsolatedHome::new();
    let server = mock_subaccount_exchange_server().await;
    let subaccount_address = expected_address(SUBACCOUNT_KEY);

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "account",
            "add",
            SUBACCOUNT_KEY,
            "--alias",
            "market-maker-1",
            "--type",
            "subaccount",
        ])
        .assert()
        .success();

    let output = env
        .account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("yes\n")
        .args([
            "--format",
            "json",
            "subaccount",
            "transfer",
            "--subaccount",
            "market-maker-1",
            "--amount",
            "10.25",
            "--direction",
            "deposit",
            "-y",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    let row = &json[0];
    let signer_address = expected_address(VALID_PRIVATE_KEY);
    assert_eq!(row["signer"].as_str(), Some(signer_address.as_str()));
    assert_eq!(row["acting_as"].as_str(), Some(signer_address.as_str()));
    assert_eq!(row["network"], "Testnet");
    assert_eq!(row["reversibility"], "partially_reversible");
    assert_eq!(row["direction"], "deposit");
    assert_eq!(row["amount"], "10.25");
    assert_eq!(row["usd"], 10_250_000);
    assert_eq!(row["token"], "USDC");
    assert_eq!(
        row["subaccount"].as_str().unwrap().to_ascii_lowercase(),
        subaccount_address.to_ascii_lowercase()
    );

    let requests = server.received_requests().await.unwrap();
    let body = exchange_body(&requests);
    assert_eq!(body["action"]["type"], "subAccountTransfer");
    assert_eq!(
        body["action"]["subAccountUser"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase(),
        subaccount_address.to_ascii_lowercase()
    );
    assert_eq!(body["action"]["isDeposit"], true);
    assert_eq!(body["action"]["usd"], 10_250_000);
    assert!(body["vaultAddress"].is_null());
}

#[tokio::test]
async fn subaccount_transfer_yes_skips_confirmation_prompt() {
    let env = IsolatedHome::new();
    let server = mock_subaccount_exchange_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "subaccount",
            "transfer",
            "--subaccount",
            SUBACCOUNT_ADDRESS,
            "--amount",
            "10.25",
            "--direction",
            "deposit",
            "-y",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json[0]["status"], "confirmed");
}

#[tokio::test]
async fn subaccount_spot_transfer_submits_expected_exchange_payload() {
    let env = IsolatedHome::new();
    let server = mock_subaccount_exchange_server().await;
    let token = "PURR:0xc4bf3f870c0e9465323c0b6ed28096c2";

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("yes\n")
        .args([
            "--format",
            "json",
            "subaccount",
            "spot-transfer",
            "--subaccount",
            SUBACCOUNT_ADDRESS,
            "--token",
            token,
            "--amount",
            "1.2300",
            "--direction",
            "deposit",
            "-y",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    let row = &json[0];
    let signer_address = expected_address(VALID_PRIVATE_KEY);
    assert_eq!(row["signer"].as_str(), Some(signer_address.as_str()));
    assert_eq!(row["acting_as"].as_str(), Some(signer_address.as_str()));
    assert_eq!(row["network"], "Testnet");
    assert_eq!(row["reversibility"], "partially_reversible");
    assert_eq!(row["direction"], "deposit");
    assert_eq!(row["amount"], "1.23");
    assert_eq!(row["token"], token);
    assert_eq!(row["subaccount"], SUBACCOUNT_ADDRESS);

    let requests = server.received_requests().await.unwrap();
    let body = exchange_body(&requests);
    assert_eq!(body["action"]["type"], "subAccountSpotTransfer");
    assert_eq!(
        body["action"]["subAccountUser"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase(),
        SUBACCOUNT_ADDRESS.to_ascii_lowercase()
    );
    assert_eq!(body["action"]["isDeposit"], true);
    assert_eq!(body["action"]["token"], token);
    assert_eq!(body["action"]["amount"], "1.23");
    assert!(body["nonce"].is_u64());
    assert!(body["signature"].is_object());
    assert!(body["vaultAddress"].is_null());
}

#[tokio::test]
async fn subaccount_spot_transfer_yes_skips_confirmation_prompt() {
    let env = IsolatedHome::new();
    let server = mock_subaccount_exchange_server().await;
    let token = "PURR:0xc4bf3f870c0e9465323c0b6ed28096c2";

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "subaccount",
            "spot-transfer",
            "--subaccount",
            SUBACCOUNT_ADDRESS,
            "--token",
            token,
            "--amount",
            "1.2300",
            "--direction",
            "deposit",
            "-y",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json[0]["status"], "confirmed");
}

#[test]
fn subaccount_transfer_rejects_more_than_six_usdc_decimals() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "--dry-run",
            "subaccount",
            "transfer",
            "--subaccount",
            SUBACCOUNT_ADDRESS,
            "--amount",
            "0.0000001",
            "--direction",
            "deposit",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("at most 6 decimal places"));
}

#[test]
fn subaccount_transfer_rejects_short_hex_address_with_precise_wording() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "--dry-run",
            "subaccount",
            "transfer",
            "--subaccount",
            "0x123",
            "--amount",
            "0.000001",
            "--direction",
            "deposit",
        ])
        .assert()
        .code(13)
        .stderr(predicate::str::contains("40-hex-character"))
        .stderr(predicate::str::contains("20-byte"));
}

#[tokio::test]
async fn orders_create_dry_run_on_behalf_includes_vault_context() {
    let env = IsolatedHome::new();
    let server = MockServer::start().await;
    mount_common_public_endpoints(&server).await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format",
            "json",
            "--dry-run",
            "orders",
            "create",
            "--on-behalf-of",
            ZERO_ADDRESS,
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--type",
            "limit",
            "--price",
            "50000",
            "--size",
            "0.001",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["command"], "orders create");
    assert_eq!(json["args"]["on_behalf_of"], ZERO_ADDRESS);
    assert!(json["signer"].is_null());
    assert_eq!(json["acting_as"], ZERO_ADDRESS);
    assert_eq!(json["vault_address"], ZERO_ADDRESS);
}

#[tokio::test]
async fn orders_create_on_behalf_sets_vault_address_on_exchange_request() {
    let env = IsolatedHome::new();
    let server = MockServer::start().await;
    mount_common_public_endpoints(&server).await;
    mount_successful_exchange_actions(&server, fixture_order_success_response(12345)).await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--on-behalf-of",
            SUBACCOUNT_ADDRESS,
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--type",
            "limit",
            "--price",
            "50000",
            "--size",
            "0.001",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("12345"));

    let requests = server.received_requests().await.unwrap();
    let body = exchange_body(&requests);
    assert_eq!(body["action"]["type"], "order");
    assert_eq!(body["vaultAddress"], SUBACCOUNT_ADDRESS);
    assert!(body["signature"].is_object());
}

#[test]
fn acting_account_selector_help_distinguishes_transfer_recipients() {
    let env = IsolatedHome::new();

    env.command()
        .args(["subaccount", "transfer", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("acting-account selector"));

    env.command()
        .args(["orders", "create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Acting-account selector"))
        .stdout(predicate::str::contains("vaultAddress"));
}

#[test]
fn schema_describes_acting_account_selectors_and_raw_destinations() {
    let env = IsolatedHome::new();

    let subaccount_output = env
        .command()
        .args(["--format", "json", "schema", "subaccount", "transfer"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let subaccount_schema: Value = serde_json::from_slice(&subaccount_output).unwrap();
    let subaccount_description = subaccount_schema["args"]
        .as_array()
        .unwrap()
        .iter()
        .find(|arg| arg["id"] == "subaccount")
        .and_then(|arg| arg["description"].as_str())
        .unwrap();
    assert!(subaccount_description.contains("acting-account selector"));
    assert!(subaccount_description.contains("does not apply to transfer recipients"));
    let yes_arg = subaccount_schema["args"]
        .as_array()
        .unwrap()
        .iter()
        .find(|arg| arg["id"] == "yes")
        .cloned()
        .expect("schema should expose --yes for subaccount transfer");
    assert_eq!(yes_arg["long"], "yes");
    assert_eq!(yes_arg["short"], "y");
    assert_eq!(yes_arg["arg_type"], "boolean");

    let transfer_output = env
        .command()
        .args(["--format", "json", "schema", "transfer", "send"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let transfer_schema: Value = serde_json::from_slice(&transfer_output).unwrap();
    let to_description = transfer_schema["args"]
        .as_array()
        .unwrap()
        .iter()
        .find(|arg| arg["id"] == "to")
        .and_then(|arg| arg["description"].as_str())
        .unwrap();
    assert!(to_description.contains("Explicit destination address"));
    assert!(to_description.contains("Wallet aliases are not resolved"));
    let to_kind = transfer_schema["args"]
        .as_array()
        .unwrap()
        .iter()
        .find(|arg| arg["id"] == "to")
        .and_then(|arg| arg["input_kind"].as_str())
        .unwrap();
    assert_eq!(to_kind, "raw_destination_address");

    let subaccount_spot_output = env
        .command()
        .args(["--format", "json", "schema", "subaccount", "spot-transfer"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let subaccount_spot_schema: Value = serde_json::from_slice(&subaccount_spot_output).unwrap();
    let spot_yes_arg = subaccount_spot_schema["args"]
        .as_array()
        .unwrap()
        .iter()
        .find(|arg| arg["id"] == "yes")
        .cloned()
        .expect("schema should expose --yes for subaccount spot-transfer");
    assert_eq!(spot_yes_arg["long"], "yes");
    assert_eq!(spot_yes_arg["short"], "y");
    assert_eq!(spot_yes_arg["arg_type"], "boolean");
}
