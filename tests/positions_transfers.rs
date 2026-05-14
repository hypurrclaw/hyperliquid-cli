mod support;

use predicates::prelude::*;
use serde_json::Value;
use support::{
    API_OVERRIDE_ENV, IsolatedHome, PRIVATE_KEY_ENV, TEST_ACCOUNT_PASSPHRASE, VALID_PRIVATE_KEY,
    expected_address,
};
use wiremock::matchers::{body_partial_json, body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const VALID_ADDRESS: &str = "0x0000000000000000000000000000000000000001";
const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";

fn clearinghouse_state(withdrawable: &str) -> serde_json::Value {
    serde_json::json!({
        "marginSummary": {
            "accountValue": "10000",
            "totalNtlPos": "4000",
            "totalRawUsd": "10000",
            "totalMarginUsed": "1000"
        },
        "crossMarginSummary": {
            "accountValue": "10000",
            "totalNtlPos": "4000",
            "totalRawUsd": "10000",
            "totalMarginUsed": "1000"
        },
        "crossMaintenanceMarginUsed": "100",
        "withdrawable": withdrawable,
        "assetPositions": [
            {
                "type": "oneWay",
                "position": {
                    "coin": "BTC",
                    "szi": "0.1",
                    "leverage": {"type": "cross", "value": 5},
                    "entryPx": "50000",
                    "positionValue": "5100",
                    "unrealizedPnl": "100",
                    "returnOnEquity": "0.1",
                    "liquidationPx": null,
                    "marginUsed": "1000",
                    "maxLeverage": 50,
                    "cumFunding": {"allTime": "0", "sinceOpen": "0", "sinceChange": "0"}
                }
            },
            {
                "type": "oneWay",
                "position": {
                    "coin": "ETH",
                    "szi": "-1",
                    "leverage": {"type": "isolated", "value": 3, "rawUsd": "1000"},
                    "entryPx": "3200",
                    "positionValue": "3000",
                    "unrealizedPnl": "-200",
                    "returnOnEquity": "-0.2",
                    "liquidationPx": "6000",
                    "marginUsed": "1000",
                    "maxLeverage": 50,
                    "cumFunding": {"allTime": "0", "sinceOpen": "0", "sinceChange": "0"}
                }
            }
        ],
        "time": 1700000000000_u64
    })
}

async fn mock_positions_transfer_server(withdrawable: &str, spot_usdc_total: &str) -> MockServer {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "allMids"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "BTC": "51000",
            "ETH": "3000"
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "meta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "universe": [
                {
                    "name": "BTC",
                    "szDecimals": 5,
                    "maxLeverage": 50,
                    "onlyIsolated": false,
                    "marginMode": null,
                    "growthMode": "disabled"
                },
                {
                    "name": "ETH",
                    "szDecimals": 4,
                    "maxLeverage": 50,
                    "onlyIsolated": false,
                    "marginMode": null,
                    "growthMode": "disabled"
                }
            ],
            "collateralToken": 0
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "spotMeta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "universe": [],
            "tokens": [
                {
                    "name": "USDC",
                    "index": 0,
                    "tokenId": "0x00000000000000000000000000000000",
                    "szDecimals": 6,
                    "weiDecimals": 6,
                    "evmContract": null
                }
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "perpDexs"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "clearinghouseState"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(clearinghouse_state(withdrawable)))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "spotClearinghouseState"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "balances": [
                {
                    "coin": "USDC",
                    "token": 0,
                    "hold": "0",
                    "total": spot_usdc_total,
                    "entryNtl": "0"
                },
                {
                    "coin": "+100",
                    "hold": "0.0",
                    "total": "0.0",
                    "entryNtl": "0.0"
                }
            ]
        })))
        .mount(&server)
        .await;

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

async fn mock_spot_token_transfer_server() -> MockServer {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "allMids"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "BTC": "50000"
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "spotMeta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "universe": [],
            "tokens": [
                {
                    "name": "USDC",
                    "index": 0,
                    "tokenId": "0x00000000000000000000000000000000",
                    "szDecimals": 6,
                    "weiDecimals": 6,
                    "evmContract": null,
                    "fullName": null,
                    "deployerTradingFeeShare": "0",
                    "evmExtraWeiDecimals": 0
                },
                {
                    "name": "HYPE",
                    "index": 150,
                    "tokenId": "0x11111111111111111111111111111111",
                    "szDecimals": 2,
                    "weiDecimals": 8,
                    "evmContract": null,
                    "fullName": null,
                    "deployerTradingFeeShare": "0",
                    "evmExtraWeiDecimals": 0
                }
            ]
        })))
        .mount(&server)
        .await;

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

async fn exchange_body(server: &MockServer) -> Value {
    let requests = server.received_requests().await.unwrap();
    let exchange_request = requests
        .iter()
        .find(|request| request.url.path() == "/exchange")
        .expect("expected /exchange request");
    serde_json::from_slice(&exchange_request.body).unwrap()
}

#[tokio::test]
async fn positions_list_shows_positions_with_pnl_coloring() {
    let env = IsolatedHome::new();
    let server = mock_positions_transfer_server("1000", "1000").await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["positions", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BTC"))
        .stdout(predicate::str::contains("Size"))
        .stdout(predicate::str::contains("Entry"))
        .stdout(predicate::str::contains("Mark"))
        .stdout(predicate::str::contains("Unrealized PnL"))
        .stdout(predicate::str::contains("\u{1b}[32m100\u{1b}[0m"))
        .stdout(predicate::str::contains("\u{1b}[31m-200\u{1b}[0m"));
}

#[tokio::test]
async fn positions_list_table_and_json_keep_pnl_uncolored() {
    let env = IsolatedHome::new();
    let server = mock_positions_transfer_server("1000", "1000").await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["--format", "table", "positions", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("100"))
        .stdout(predicate::str::contains("-200"))
        .stdout(predicate::str::contains("\u{1b}[").not());

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["--format", "json", "positions", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"unrealized_pnl\""))
        .stdout(predicate::str::contains("\"100\""))
        .stdout(predicate::str::contains("\"-200\""))
        .stdout(predicate::str::contains("\u{1b}[").not());
}

#[tokio::test]
async fn transfer_confirmation_prompts_are_yellow_only_in_pretty() {
    let env = IsolatedHome::new();
    let server = mock_positions_transfer_server("1000", "1000").await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("n\n")
        .args(["transfer", "spot-to-perp", "--amount", "100", "--testnet"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "\u{1b}[33mTransfer 100 USDC from spot to perp? [y/N] \u{1b}[0m",
        ));

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("n\n")
        .args([
            "--format",
            "json",
            "transfer",
            "spot-to-perp",
            "--amount",
            "100",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "Transfer 100 USDC from spot to perp? [y/N]",
        ))
        .stderr(predicate::str::contains("\u{1b}[").not());
}

#[tokio::test]
async fn positions_update_leverage_validates_and_outputs_confirmation() {
    let env = IsolatedHome::new();
    let server = mock_positions_transfer_server("1000", "1000").await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "positions",
            "update-leverage",
            "--coin",
            "BTC",
            "--leverage",
            "5",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("BTC"))
        .stdout(predicate::str::contains("5"))
        .stdout(predicate::str::contains("updated"));

    let body = exchange_body(&server).await;
    assert_eq!(body["action"]["type"], "updateLeverage");
    assert_eq!(body["action"]["asset"], 0);
    assert_eq!(body["action"]["isCross"], true);
    assert_eq!(body["action"]["leverage"], 5);
    assert!(body["signature"].is_object());
}

#[test]
fn positions_update_leverage_rejects_zero() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "positions",
            "update-leverage",
            "--coin",
            "BTC",
            "--leverage",
            "0",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid value"));
}

#[tokio::test]
async fn positions_update_margin_outputs_confirmation() {
    let env = IsolatedHome::new();
    let server = mock_positions_transfer_server("1000", "1000").await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "positions",
            "update-margin",
            "--coin",
            "BTC",
            "--amount",
            "1000",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("BTC"))
        .stdout(predicate::str::contains("1000"))
        .stdout(predicate::str::contains("updated"));

    let body = exchange_body(&server).await;
    assert_eq!(body["action"]["type"], "updateIsolatedMargin");
    assert_eq!(body["action"]["asset"], 0);
    assert_eq!(body["action"]["isBuy"], true);
    assert_eq!(body["action"]["ntli"], 1_000_000_000_u64);
    assert!(body["signature"].is_object());
}

#[tokio::test]
async fn transfer_spot_to_perp_prompts_and_confirms() {
    let env = IsolatedHome::new();
    let server = mock_positions_transfer_server("1000", "1000").await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("y\n")
        .args(["transfer", "spot-to-perp", "--amount", "100", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("spot-to-perp"))
        .stdout(predicate::str::contains("100"))
        .stderr(predicate::str::contains(
            "Transfer 100 USDC from spot to perp? [y/N]",
        ));

    let body = exchange_body(&server).await;
    assert_eq!(body["action"]["type"], "usdClassTransfer");
    assert_eq!(body["action"]["amount"], "100");
    assert_eq!(body["action"]["toPerp"], true);
    assert!(body["action"]["nonce"].is_u64());
    assert!(body["signature"].is_object());
}

#[tokio::test]
async fn transfer_perp_to_spot_prompts_and_confirms() {
    let env = IsolatedHome::new();
    let server = mock_positions_transfer_server("1000", "1000").await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("y\n")
        .args(["transfer", "perp-to-spot", "--amount", "100", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("perp-to-spot"))
        .stderr(predicate::str::contains(
            "Transfer 100 USDC from perp to spot? [y/N]",
        ));

    let body = exchange_body(&server).await;
    assert_eq!(body["action"]["type"], "usdClassTransfer");
    assert_eq!(body["action"]["amount"], "100");
    assert_eq!(body["action"]["toPerp"], false);
    assert!(body["action"]["nonce"].is_u64());
    assert!(body["signature"].is_object());
}

#[tokio::test]
async fn transfer_send_prompts_and_confirms() {
    let env = IsolatedHome::new();
    let server = mock_positions_transfer_server("1000", "1000").await;
    let signer = expected_address(VALID_PRIVATE_KEY);

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("yes\n")
        .args([
            "transfer",
            "send",
            "--to",
            VALID_ADDRESS,
            "--amount",
            "100",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("send"))
        .stdout(predicate::str::contains(VALID_ADDRESS))
        .stderr(predicate::str::contains("Network: Testnet"))
        .stderr(predicate::str::contains(format!("Signer: {signer}")))
        .stderr(predicate::str::contains(format!(
            "Acting context: {signer}"
        )))
        .stderr(predicate::str::contains(format!(
            "Recipient: {VALID_ADDRESS}"
        )))
        .stderr(predicate::str::contains(
            "Destination class: hyperliquid_user_address",
        ))
        .stderr(predicate::str::contains("Asset: USDC"))
        .stderr(predicate::str::contains("Amount: 100"))
        .stderr(predicate::str::contains(
            "Fee/cap: not_estimated_exchange_default",
        ))
        .stderr(predicate::str::contains("Reversibility: irreversible"))
        .stderr(predicate::str::contains(format!(
            "Send 100 USDC to {VALID_ADDRESS}? [y/N]"
        )));

    let body = exchange_body(&server).await;
    assert_eq!(body["action"]["type"], "usdSend");
    assert_eq!(
        body["action"]["destination"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase(),
        VALID_ADDRESS.to_ascii_lowercase()
    );
    assert_eq!(body["action"]["amount"], "100");
    assert!(body["action"]["time"].is_u64());
    assert!(body["signature"].is_object());
}

#[tokio::test]
async fn transfer_spot_send_dry_run_includes_token_and_destination() {
    let env = IsolatedHome::new();
    let server = mock_spot_token_transfer_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format",
            "json",
            "--dry-run",
            "transfer",
            "spot-send",
            "--to",
            VALID_ADDRESS,
            "--token",
            "HYPE",
            "--amount",
            "1.23",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["command"], "transfer spot-send");
    assert_eq!(json["would_execute"], "spot_send");
    assert_eq!(json["args"]["to"], VALID_ADDRESS);
    assert_eq!(json["args"]["token"], "HYPE");
    assert_eq!(json["args"]["token_index"], 150);
    assert_eq!(json["args"]["amount"], "1.23");
}

#[tokio::test]
async fn transfer_send_asset_dry_run_normalizes_asset_targets() {
    let env = IsolatedHome::new();
    let server = mock_spot_token_transfer_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format",
            "json",
            "--dry-run",
            "transfer",
            "send-asset",
            "--to",
            VALID_ADDRESS,
            "--source",
            "spot",
            "--dest",
            "dex:test",
            "--token",
            "USDC",
            "--amount",
            "4.5",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["command"], "transfer send-asset");
    assert_eq!(json["would_execute"], "send_asset");
    assert_eq!(json["args"]["source"], "spot");
    assert_eq!(json["args"]["source_wire"], "spot");
    assert_eq!(json["args"]["dest"], "dex:test");
    assert_eq!(json["args"]["dest_wire"], "test");
    assert_eq!(json["args"]["token"], "USDC");
    assert_eq!(json["args"]["amount"], "4.5");
}

#[test]
fn transfer_send_asset_invalid_target_exits_before_auth() {
    let env = IsolatedHome::new();

    env.command()
        .args([
            "transfer",
            "send-asset",
            "--to",
            VALID_ADDRESS,
            "--source",
            "alias:foo",
            "--dest",
            "spot",
            "--token",
            "USDC",
            "--amount",
            "1",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "asset target must be perp, spot, or dex:<NAME>",
        ))
        .stderr(predicate::str::contains("Authentication required").not());
}

#[tokio::test]
async fn transfer_spot_send_submits_expected_exchange_payload() {
    let env = IsolatedHome::new();
    let server = mock_spot_token_transfer_server().await;
    let signer = expected_address(VALID_PRIVATE_KEY);

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("yes\n")
        .args([
            "transfer",
            "spot-send",
            "--to",
            VALID_ADDRESS,
            "--token",
            "HYPE",
            "--amount",
            "1.23",
            "--testnet",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Network: Testnet"))
        .stderr(predicate::str::contains(format!("Signer: {signer}")))
        .stderr(predicate::str::contains(format!(
            "Acting context: {signer}"
        )))
        .stderr(predicate::str::contains(format!(
            "Recipient: {VALID_ADDRESS}"
        )))
        .stderr(predicate::str::contains(
            "Destination class: hyperliquid_spot_user_address",
        ))
        .stderr(predicate::str::contains("Asset: HYPE"))
        .stderr(predicate::str::contains("Amount: 1.23"))
        .stderr(predicate::str::contains(
            "Fee/cap: not_estimated_exchange_default",
        ))
        .stderr(predicate::str::contains("Reversibility: irreversible"))
        .stderr(predicate::str::contains(format!(
            "Send 1.23 HYPE spot token to {VALID_ADDRESS}? [y/N]"
        )));

    let body = exchange_body(&server).await;
    assert_eq!(body["action"]["type"], "spotSend");
    assert_eq!(
        body["action"]["destination"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase(),
        VALID_ADDRESS.to_ascii_lowercase()
    );
    assert_eq!(body["action"]["token"], "HYPE");
    assert_eq!(body["action"]["amount"], "1.23");
    assert!(body["action"]["time"].is_u64());
    assert!(body["signature"].is_object());
}

#[tokio::test]
async fn transfer_send_asset_submits_expected_exchange_payload() {
    let env = IsolatedHome::new();
    let server = mock_spot_token_transfer_server().await;
    let signer = expected_address(VALID_PRIVATE_KEY);

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("yes\n")
        .args([
            "transfer",
            "send-asset",
            "--to",
            VALID_ADDRESS,
            "--source",
            "spot",
            "--dest",
            "dex:test",
            "--token",
            "USDC",
            "--amount",
            "4.5",
            "--testnet",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("Network: Testnet"))
        .stderr(predicate::str::contains(format!("Signer: {signer}")))
        .stderr(predicate::str::contains(format!(
            "Acting context: {signer}"
        )))
        .stderr(predicate::str::contains(format!(
            "Recipient: {VALID_ADDRESS}"
        )))
        .stderr(predicate::str::contains(
            "Destination class: hyperliquid_asset_context",
        ))
        .stderr(predicate::str::contains("Asset: USDC spot->dex:test"))
        .stderr(predicate::str::contains("Amount: 4.5"))
        .stderr(predicate::str::contains(
            "Fee/cap: not_estimated_exchange_default",
        ))
        .stderr(predicate::str::contains("Reversibility: irreversible"))
        .stderr(predicate::str::contains(format!(
            "Send 4.5 USDC from spot to dex:test for {VALID_ADDRESS}? [y/N]"
        )));

    let body = exchange_body(&server).await;
    assert_eq!(body["action"]["type"], "sendAsset");
    assert_eq!(
        body["action"]["destination"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase(),
        VALID_ADDRESS.to_ascii_lowercase()
    );
    assert_eq!(body["action"]["sourceDex"], "spot");
    assert_eq!(body["action"]["destinationDex"], "test");
    assert_eq!(body["action"]["token"], "USDC");
    assert_eq!(body["action"]["amount"], "4.5");
    assert_eq!(body["action"]["fromSubAccount"], "");
    assert!(body["action"]["nonce"].is_u64());
    assert!(body["signature"].is_object());
}

#[tokio::test]
async fn transfer_withdraw_prompts_and_confirms() {
    let env = IsolatedHome::new();
    let server = mock_positions_transfer_server("1000", "1000").await;
    let signer = expected_address(VALID_PRIVATE_KEY);

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .write_stdin("y\n")
        .args([
            "transfer",
            "withdraw",
            "--to",
            VALID_ADDRESS,
            "--amount",
            "100",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("withdraw"))
        .stdout(predicate::str::contains(VALID_ADDRESS))
        .stderr(predicate::str::contains("Network: Testnet"))
        .stderr(predicate::str::contains(format!("Signer: {signer}")))
        .stderr(predicate::str::contains(format!(
            "Acting context: {signer}"
        )))
        .stderr(predicate::str::contains(format!(
            "Recipient: {VALID_ADDRESS}"
        )))
        .stderr(predicate::str::contains(
            "Destination class: arbitrum_address",
        ))
        .stderr(predicate::str::contains("Asset: USDC"))
        .stderr(predicate::str::contains("Amount: 100"))
        .stderr(predicate::str::contains(
            "Fee/cap: not_estimated_bridge_or_exchange_fee",
        ))
        .stderr(predicate::str::contains("Reversibility: irreversible"))
        .stderr(predicate::str::contains(format!(
            "Withdraw 100 USDC to Arbitrum address {VALID_ADDRESS}? [y/N]"
        )));

    let body = exchange_body(&server).await;
    assert_eq!(body["action"]["type"], "withdraw3");
    assert_eq!(
        body["action"]["destination"]
            .as_str()
            .unwrap()
            .to_ascii_lowercase(),
        VALID_ADDRESS.to_ascii_lowercase()
    );
    assert_eq!(body["action"]["amount"], "100");
    assert!(body["action"]["time"].is_u64());
    assert!(body["signature"].is_object());
}

#[tokio::test]
async fn transfer_insufficient_balance_exits_13_with_context() {
    let env = IsolatedHome::new();
    let server = mock_positions_transfer_server("1000", "5").await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["transfer", "spot-to-perp", "--amount", "100", "--testnet"])
        .assert()
        .code(13)
        .stderr(predicate::str::contains("insufficient balance"))
        .stderr(predicate::str::contains("available spot USDC"));
}

#[test]
fn transfer_invalid_address_exits_2() {
    let env = IsolatedHome::new();

    env.command()
        .args([
            "transfer",
            "send",
            "--to",
            "INVALID",
            "--amount",
            "100",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "address must be a 0x-prefixed 40-byte hex string",
        ));
}

#[test]
fn transfer_send_zero_address_exits_2_before_prompt_or_auth() {
    let env = IsolatedHome::new();

    env.command()
        .args([
            "transfer",
            "send",
            "--to",
            ZERO_ADDRESS,
            "--amount",
            "100",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "address must not be the zero address",
        ))
        .stderr(predicate::str::contains("Send 100 USDC").not())
        .stderr(predicate::str::contains("Authentication required").not());
}

#[test]
fn transfer_send_recipient_does_not_resolve_stored_account_alias() {
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

    let output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "--format",
            "json",
            "--dry-run",
            "transfer",
            "send",
            "--to",
            "recipient-alias",
            "--amount",
            "1",
        ])
        .assert()
        .failure()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert!(json["error"].as_str().unwrap().contains("0x-prefixed"));
}

#[test]
fn transfer_withdraw_zero_address_exits_2_before_prompt_or_auth() {
    let env = IsolatedHome::new();

    env.command()
        .args([
            "transfer",
            "withdraw",
            "--to",
            ZERO_ADDRESS,
            "--amount",
            "100",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "address must not be the zero address",
        ))
        .stderr(predicate::str::contains("Withdraw 100 USDC").not())
        .stderr(predicate::str::contains("Authentication required").not());
}

#[tokio::test]
async fn update_leverage_resolves_asset_before_exchange_action() {
    let env = IsolatedHome::new();
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "allMids"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "BTC": "51000"
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "meta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "universe": [
                {
                    "name": "BTC",
                    "szDecimals": 5,
                    "maxLeverage": 50,
                    "onlyIsolated": false,
                    "marginMode": null,
                    "growthMode": "disabled"
                }
            ],
            "collateralToken": 0
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "spotMeta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "universe": [],
            "tokens": []
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "perpDexs"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_string_contains("\"type\":\"updateLeverage\""))
        .and(body_string_contains("\"asset\":0"))
        .and(body_string_contains("\"leverage\":5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": {"type": "default"}
        })))
        .expect(1)
        .mount(&server)
        .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "positions",
            "update-leverage",
            "--coin",
            "BTC",
            "--leverage",
            "5",
            "--testnet",
        ])
        .assert()
        .success();
}
