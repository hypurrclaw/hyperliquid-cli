mod support;

use predicates::prelude::*;
use serde_json::Value;
use support::{API_OVERRIDE_ENV, IsolatedHome, PRIVATE_KEY_ENV, VALID_PRIVATE_KEY, mount_all_mids};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const VALID_ADDRESS: &str = "0x0000000000000000000000000000000000000001";
const VAULT_ADDRESS: &str = "0x0000000000000000000000000000000000000002";

async fn mount_override_healthcheck(server: &MockServer) {
    mount_all_mids(server, "51000", "3000").await;
}

async fn mount_numeric_override_healthcheck(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "allMids"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "BTC": 51000.0,
            "ETH": 3000.0
        })))
        .mount(server)
        .await;
}

async fn mock_vault_server() -> MockServer {
    let server = MockServer::start().await;
    mount_numeric_override_healthcheck(&server).await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "vaultDetails",
            "vaultAddress": VAULT_ADDRESS
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "Test Vault",
            "vaultAddress": VAULT_ADDRESS,
            "leader": VALID_ADDRESS,
            "description": "market neutral test strategy",
            "portfolio": [
                ["allTime", {
                    "accountValueHistory": [[1700000000000_u64, 12345.67]],
                    "pnlHistory": [[1700000000000_u64, "234.56"]],
                    "vlm": "98765.43"
                }]
            ],
            "apr": 0.1234,
            "followerState": null,
            "leaderFraction": 0.1,
            "leaderCommission": 0.05,
            "followers": [],
            "maxDistributable": 1000.0,
            "maxWithdrawable": 500.0,
            "isClosed": false,
            "relationship": {"type": "normal"},
            "allowDeposits": true,
            "alwaysCloseOnWithdraw": false
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "clearinghouseState",
            "user": VAULT_ADDRESS
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "assetPositions": [
                {
                    "type": "oneWay",
                    "position": {
                        "coin": "BTC",
                        "szi": "0.25",
                        "entryPx": "50000",
                        "positionValue": "12750",
                        "unrealizedPnl": "250",
                        "returnOnEquity": "0.02",
                        "liquidationPx": null,
                        "maxLeverage": 50,
                        "marginUsed": "2550",
                        "cumFunding": {"allTime": "0", "sinceOpen": "0", "sinceChange": "0"},
                        "leverage": {"type": "cross", "value": 5}
                    }
                }
            ],
            "crossMarginSummary": {
                "accountValue": "15000",
                "totalMarginUsed": "2550",
                "totalNtlPos": "12750",
                "totalRawUsd": "15000"
            },
            "crossMaintenanceMarginUsed": "100",
            "marginSummary": {
                "accountValue": "15000",
                "totalMarginUsed": "2550",
                "totalNtlPos": "12750",
                "totalRawUsd": "15000"
            },
            "withdrawable": "1000",
            "time": 1700000000000_u64
        })))
        .mount(&server)
        .await;

    server
}

async fn mock_scientific_vault_server() -> MockServer {
    let server = MockServer::start().await;
    mount_numeric_override_healthcheck(&server).await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "vaultDetails",
            "vaultAddress": VAULT_ADDRESS
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "Scientific Vault",
            "vaultAddress": VAULT_ADDRESS,
            "leader": VALID_ADDRESS,
            "description": "scientific notation fields",
            "portfolio": [
                ["allTime", {
                    "accountValueHistory": [[1700000000000_u64, 12345.67]],
                    "pnlHistory": [[1700000000000_u64, "234.56"]],
                    "vlm": "98765.43"
                }]
            ],
            "apr": -5.262868504855727e-3,
            "leaderFraction": 7.445690056409042e-7,
            "leaderCommission": 0.0,
            "followers": [],
            "maxDistributable": 0.0,
            "maxWithdrawable": 0.0,
            "isClosed": false,
            "relationship": {"type": "normal"},
            "allowDeposits": true,
            "alwaysCloseOnWithdraw": false
        })))
        .mount(&server)
        .await;

    server
}

async fn mock_vault_summaries_server() -> MockServer {
    let server = MockServer::start().await;
    mount_override_healthcheck(&server).await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "vaultSummaries"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "name": "Alpha Carry",
                "vaultAddress": "0x0000000000000000000000000000000000000011",
                "leader": "0x00000000000000000000000000000000000000aa",
                "tvl": "1000.5",
                "apr": "0.12",
                "userDeposit": "42.5",
                "isClosed": false,
                "relationship": {"type": "normal"},
                "createTimeMillis": 1700000000000_i64
            },
            {
                "name": "Beta Yield",
                "vaultAddress": "0x0000000000000000000000000000000000000022",
                "leader": "0x00000000000000000000000000000000000000bb",
                "tvl": "2500",
                "apr": "0.05",
                "isClosed": false,
                "relationship": {"type": "parent", "data": {"childAddresses": []}},
                "createTimeMillis": 1710000000000_i64
            },
            {
                "name": "Gamma Hedge",
                "vaultAddress": "0x0000000000000000000000000000000000000033",
                "leader": "0x00000000000000000000000000000000000000cc",
                "tvl": "10",
                "apr": "0.25",
                "isClosed": false,
                "relationship": {"type": "child"},
                "createTimeMillis": 1720000000000_i64
            }
        ])))
        .mount(&server)
        .await;

    server
}

async fn mock_vault_action_server() -> MockServer {
    let server = MockServer::start().await;
    mount_override_healthcheck(&server).await;
    Mock::given(method("POST"))
        .and(path("/exchange"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": { "type": "default" }
        })))
        .mount(&server)
        .await;
    server
}

async fn mock_borrowlend_server() -> MockServer {
    let server = MockServer::start().await;
    mount_override_healthcheck(&server).await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "spotMeta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "universe": [],
            "tokens": [
                {
                    "name": "USDC",
                    "szDecimals": 8,
                    "weiDecimals": 8,
                    "index": 0,
                    "tokenId": "0x00000000000000000000000000000000",
                    "isCanonical": true,
                    "evmContract": null,
                    "fullName": null
                },
                {
                    "name": "HYPE",
                    "szDecimals": 8,
                    "weiDecimals": 8,
                    "index": 150,
                    "tokenId": "0x00000000000000000000000000000001",
                    "isCanonical": true,
                    "evmContract": null,
                    "fullName": null
                }
            ]
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "allBorrowLendReserveStates"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            [
                0,
                {
                    "borrowYearlyRate": "0.05",
                    "supplyYearlyRate": "0.0142",
                    "balance": "28175848.640569929",
                    "utilization": "0.31617754",
                    "oraclePx": "1.0",
                    "ltv": "0.0",
                    "totalSupplied": "41203454.1833538637",
                    "totalBorrowed": "13027606.7840966005"
                }
            ],
            [
                150,
                {
                    "borrowYearlyRate": "0.05",
                    "supplyYearlyRate": "0.0",
                    "balance": "1354673.0887382801",
                    "utilization": "0.0",
                    "oraclePx": "39.917",
                    "ltv": "0.5",
                    "totalSupplied": "1354673.0887382801",
                    "totalBorrowed": "0.0"
                }
            ]
        ])))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "borrowLendReserveState",
            "token": 0
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "borrowYearlyRate": "0.05",
            "supplyYearlyRate": "0.0142",
            "balance": "28175848.640569929",
            "utilization": "0.31617754",
            "oraclePx": "1.0",
            "ltv": "0.0",
            "totalSupplied": "41203454.1833538637",
            "totalBorrowed": "13027606.7840966005"
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "spotClearinghouseState",
            "user": VALID_ADDRESS
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "balances": [
                {
                    "coin": "USDC",
                    "token": 0,
                    "hold": "0",
                    "total": "42.5",
                    "entryNtl": "42.5"
                },
                {
                    "coin": "HYPE",
                    "token": 150,
                    "hold": "1.25",
                    "total": "-3.75",
                    "entryNtl": "-3.75"
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
            "response": { "type": "default" }
        })))
        .mount(&server)
        .await;

    server
}

async fn mock_borrowlend_unavailable_server() -> MockServer {
    let server = MockServer::start().await;
    mount_override_healthcheck(&server).await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "spotMeta"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "universe": [],
            "tokens": [
                {
                    "name": "USDC",
                    "szDecimals": 8,
                    "weiDecimals": 8,
                    "index": 0,
                    "tokenId": "0x00000000000000000000000000000000",
                    "isCanonical": true,
                    "evmContract": null,
                    "fullName": null
                }
            ]
        })))
        .mount(&server)
        .await;

    for request_type in ["allBorrowLendReserveStates", "borrowLendReserveState"] {
        Mock::given(method("POST"))
            .and(path("/info"))
            .and(body_partial_json(serde_json::json!({"type": request_type})))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "unknown info request type"
            })))
            .mount(&server)
            .await;
    }

    server
}

#[tokio::test]
async fn vault_get_json_shows_details() {
    let env = IsolatedHome::new();
    let server = mock_vault_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "vault", "get", VAULT_ADDRESS])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["name"], "Test Vault");
    assert_eq!(json["vault_address"], VAULT_ADDRESS);
    assert_eq!(json["tvl"], "12345.67");
    assert_eq!(json["apr"], "0.1234");
    assert_eq!(json["allow_deposits"], true);
}

#[tokio::test]
async fn vault_get_json_accepts_scientific_notation_fields() {
    let env = IsolatedHome::new();
    let server = mock_scientific_vault_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "vault", "get", VAULT_ADDRESS])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["name"], "Scientific Vault");
    assert_eq!(json["leader_fraction"], "0.0000007445690056409042");
    assert_eq!(json["apr"], "-0.005262868504855727");
}

#[tokio::test]
async fn vault_get_null_response_returns_clean_not_found_error() {
    let env = IsolatedHome::new();
    let server = MockServer::start().await;

    support::mount_common_public_endpoints(&server).await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "vaultDetails"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string("null"))
        .mount(&server)
        .await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["vault", "get", VAULT_ADDRESS])
        .assert()
        .code(13)
        .stderr(predicate::str::contains("vault details not found"));
}

#[tokio::test]
async fn vault_positions_show_open_positions() {
    let env = IsolatedHome::new();
    let server = mock_vault_server().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["vault", "positions", VAULT_ADDRESS])
        .assert()
        .success()
        .stdout(predicate::str::contains("BTC"))
        .stdout(predicate::str::contains("0.25"))
        .stderr(predicate::str::contains("Completed in"));
}

#[tokio::test]
async fn vault_list_sorts_by_tvl_and_limits_results() {
    let env = IsolatedHome::new();
    let server = mock_vault_summaries_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "vault", "list", "--limit", "2"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    let rows = json.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], "Beta Yield");
    assert_eq!(
        rows[0]["vault_address"],
        "0x0000000000000000000000000000000000000022"
    );
    assert_eq!(rows[0]["tvl"], "2500");
    assert_eq!(rows[0]["kind"], "parent");
    assert_eq!(rows[1]["name"], "Alpha Carry");
}

#[tokio::test]
async fn vault_list_filters_kind_and_sorts_by_apr() {
    let env = IsolatedHome::new();
    let server = mock_vault_summaries_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format", "json", "vault", "list", "--kind", "child", "--sort", "apr",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    let rows = json.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "Gamma Hedge");
    assert_eq!(rows[0]["apr"], "0.25");
    assert_eq!(rows[0]["kind"], "child");
}

#[tokio::test]
async fn vault_list_accepts_protocol_and_user_kind_labels() {
    let env = IsolatedHome::new();
    let server = mock_vault_summaries_server().await;

    let protocol_output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "vault", "list", "--kind", "protocol"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let protocol_json: Value = serde_json::from_slice(&protocol_output).unwrap();
    let protocol_rows = protocol_json.as_array().unwrap();
    assert_eq!(protocol_rows.len(), 1);
    assert_eq!(protocol_rows[0]["name"], "Alpha Carry");
    assert_eq!(protocol_rows[0]["kind"], "normal");

    let user_output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format", "json", "vault", "list", "--kind", "user", "--sort", "name",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let user_json: Value = serde_json::from_slice(&user_output).unwrap();
    let user_rows = user_json.as_array().unwrap();
    assert_eq!(user_rows.len(), 2);
    assert_eq!(user_rows[0]["name"], "Beta Yield");
    assert_eq!(user_rows[0]["kind"], "parent");
    assert_eq!(user_rows[1]["name"], "Gamma Hedge");
    assert_eq!(user_rows[1]["kind"], "child");
}

#[tokio::test]
async fn vault_search_with_user_includes_user_deposit_when_available() {
    let env = IsolatedHome::new();
    let server = mock_vault_summaries_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--format",
            "json",
            "vault",
            "search",
            "alpha",
            "--user",
            VALID_ADDRESS,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    let rows = json.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "Alpha Carry");
    assert_eq!(rows[0]["user_deposit"], "42.5");
}

#[tokio::test]
async fn vault_search_rejects_malformed_user_before_network() {
    let env = IsolatedHome::new();

    env.command()
        .args(["vault", "search", "alpha", "--user", "not-an-address"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "address must be a 0x-prefixed 40-byte hex string",
        ));
}

#[tokio::test]
async fn vault_search_matches_name_leader_or_address() {
    let env = IsolatedHome::new();
    let server = mock_vault_summaries_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "vault", "search", "00bb"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    let rows = json.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "Beta Yield");

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "vault", "search", "alpha"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json.as_array().unwrap()[0]["name"], "Alpha Carry");
}

#[tokio::test]
async fn vault_search_no_matches_returns_empty_json_array() {
    let env = IsolatedHome::new();
    let server = mock_vault_summaries_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "vault", "search", "does-not-exist"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn vault_list_table_output_is_uncolored() {
    let env = IsolatedHome::new();
    let server = mock_vault_summaries_server().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "table", "vault", "list", "--limit", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Beta Yield"))
        .stdout(predicate::str::contains("\u{1b}[").not());
}

#[tokio::test]
async fn vault_deposit_and_withdraw_submit_vault_transfer_actions() {
    let env = IsolatedHome::new();
    let server = mock_vault_action_server().await;

    let deposit_output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "vault",
            "deposit",
            "--vault",
            VAULT_ADDRESS,
            "--amount",
            "1000",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let deposit_json: Value = serde_json::from_slice(&deposit_output).unwrap();
    assert_eq!(deposit_json["action"], "deposit");
    assert_eq!(deposit_json["status"], "submitted");
    assert_eq!(deposit_json["network"], "Testnet");
    assert_eq!(deposit_json["vault_address"], VAULT_ADDRESS);
    assert_eq!(deposit_json["amount"], "1000");
    assert_eq!(deposit_json["asset"], "USDC");
    assert_eq!(deposit_json["reversibility"], "partially_reversible");
    assert!(deposit_json["signer"].as_str().unwrap().starts_with("0x"));
    assert!(
        deposit_json["acting_as"]
            .as_str()
            .unwrap()
            .starts_with("0x")
    );

    let withdraw_output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "vault",
            "withdraw",
            "--vault",
            VAULT_ADDRESS,
            "--amount",
            "500",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let withdraw_json: Value = serde_json::from_slice(&withdraw_output).unwrap();
    assert_eq!(withdraw_json["action"], "withdraw");
    assert_eq!(withdraw_json["status"], "submitted");
    assert_eq!(withdraw_json["network"], "Testnet");
    assert_eq!(withdraw_json["vault_address"], VAULT_ADDRESS);
    assert_eq!(withdraw_json["amount"], "500");
    assert_eq!(withdraw_json["asset"], "USDC");
    assert_eq!(withdraw_json["reversibility"], "partially_reversible");
    assert!(withdraw_json["signer"].as_str().unwrap().starts_with("0x"));
    assert!(
        withdraw_json["acting_as"]
            .as_str()
            .unwrap()
            .starts_with("0x")
    );

    let requests = server.received_requests().await.unwrap();
    let actions = requests
        .iter()
        .filter(|request| request.url.path() == "/exchange")
        .map(|request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            (
                body["action"]["type"].as_str().unwrap().to_string(),
                body["action"]["vaultAddress"].as_str().unwrap().to_string(),
                body["action"]["isDeposit"].as_bool().unwrap(),
                body["action"]["usd"].as_u64().unwrap(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        actions,
        vec![
            (
                "vaultTransfer".to_string(),
                VAULT_ADDRESS.to_string(),
                true,
                1_000_000_000
            ),
            (
                "vaultTransfer".to_string(),
                VAULT_ADDRESS.to_string(),
                false,
                500_000_000
            )
        ]
    );
}

#[test]
fn vault_actions_without_wallet_exit_10_after_validation() {
    for args in [
        vec![
            "vault",
            "deposit",
            "--vault",
            VAULT_ADDRESS,
            "--amount",
            "1",
            "--testnet",
        ],
        vec![
            "vault",
            "withdraw",
            "--vault",
            VAULT_ADDRESS,
            "--amount",
            "1",
            "--testnet",
        ],
    ] {
        let env = IsolatedHome::new();
        env.command()
            .args(args)
            .assert()
            .code(10)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("Authentication required"));
    }
}

#[tokio::test]
async fn borrowlend_rates_and_get_show_reserve_rows() {
    let env = IsolatedHome::new();
    let server = mock_borrowlend_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "borrowlend", "rates"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json[0]["token"], "USDC");
    assert_eq!(json[0]["supply_rate"], "0.0142");
    assert_eq!(json[0]["borrow_rate"], "0.05");
    assert_eq!(json[0]["total_supply"], "41203454.1833538637");
    assert_eq!(json[0]["total_borrow"], "13027606.7840966005");
    assert!(
        json[0]["note"]
            .as_str()
            .unwrap()
            .contains("Live reserve state")
    );

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["borrowlend", "get", "USDC"])
        .assert()
        .success()
        .stdout(predicate::str::contains("USDC"))
        .stdout(predicate::str::contains("0.0142"))
        .stdout(predicate::str::contains("0.05"))
        .stdout(predicate::str::contains("Live reserve state"));
}

#[tokio::test]
async fn borrowlend_reserve_unavailable_fallback_is_explicit() {
    let env = IsolatedHome::new();
    let server = mock_borrowlend_unavailable_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "borrowlend", "rates"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json[0]["token"], "USDC");
    assert_eq!(json[0]["supply_rate"], "0");
    assert_eq!(json[0]["borrow_rate"], "0");
    assert!(
        json[0]["note"]
            .as_str()
            .unwrap()
            .contains("unavailable from public Hyperliquid API")
    );

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["borrowlend", "get", "USDC"])
        .assert()
        .success()
        .stdout(predicate::str::contains("USDC"))
        .stdout(predicate::str::contains(
            "unavailable from public Hyperliquid API",
        ));
}

#[tokio::test]
async fn borrowlend_user_splits_supplied_and_borrowed_balances_without_wallet() {
    let env = IsolatedHome::new();
    let server = mock_borrowlend_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "borrowlend", "user", VALID_ADDRESS])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["user"], VALID_ADDRESS);
    let positions = json["positions"].as_array().unwrap();
    let usdc = positions
        .iter()
        .find(|position| position["token"] == "USDC")
        .unwrap();
    assert_eq!(usdc["supplied"], "42.5");
    assert_eq!(usdc["borrowed"], "0");

    let hype = positions
        .iter()
        .find(|position| position["token"] == "HYPE")
        .unwrap();
    assert_eq!(hype["supplied"], "0");
    assert_eq!(hype["borrowed"], "3.75");
    assert_eq!(hype["hold"], "1.25");
    assert!(positions.iter().all(|position| position["token"] != "+100"));
}

#[tokio::test]
async fn borrowlend_user_table_and_pretty_show_borrowed_amounts_as_positive() {
    for args in [
        vec!["borrowlend", "user", VALID_ADDRESS],
        vec!["--format", "table", "borrowlend", "user", VALID_ADDRESS],
    ] {
        let env = IsolatedHome::new();
        let server = mock_borrowlend_server().await;

        let output = env
            .command()
            .env(API_OVERRIDE_ENV, server.uri())
            .args(args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let stdout = String::from_utf8(output).unwrap();
        let hype_line = stdout
            .lines()
            .find(|line| line.contains("HYPE"))
            .expect("HYPE borrow row should be rendered");
        let fields = hype_line
            .split_whitespace()
            .filter(|field| *field != "│" && *field != "|")
            .collect::<Vec<_>>();

        assert_eq!(fields, vec![VALID_ADDRESS, "HYPE", "0", "3.75", "1.25"]);
    }
}

#[tokio::test]
async fn borrowlend_supply_dry_run_emits_core_writer_preview() {
    let env = IsolatedHome::new();
    let server = mock_borrowlend_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "--dry-run",
            "borrowlend",
            "supply",
            "USDC",
            "--amount",
            "10",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["dry_run"], true);
    assert_eq!(json["command"], "borrowlend supply");
    assert_eq!(json["would_execute"], "supply_borrowlend");
    assert_eq!(json["args"]["operation"], "supply");
    assert_eq!(json["args"]["encoded_operation"], 0);
    assert_eq!(json["args"]["token"], "USDC");
    assert_eq!(json["args"]["token_index"], 0);
    assert_eq!(json["args"]["amount"], "10");
    assert_eq!(json["args"]["network"], "Testnet");
    assert_eq!(json["args"]["wei"], 1_000_000_000_u64);
    assert_eq!(json["args"]["reversibility"], "partially_reversible");
    assert_eq!(
        json["args"]["verified_shape"]["transport"],
        "HyperEVM CoreWriter.sendRawAction"
    );
    assert_eq!(json["args"]["verified_shape"]["action_id"], 15);
    assert!(json["signer"].as_str().unwrap().starts_with("0x"));
    assert!(json["acting_as"].as_str().unwrap().starts_with("0x"));
    assert!(json["vault_address"].is_null());
}

#[tokio::test]
async fn borrowlend_withdraw_dry_run_supports_max_preview() {
    let env = IsolatedHome::new();
    let server = mock_borrowlend_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "--dry-run",
            "borrowlend",
            "withdraw",
            "USDC",
            "--max",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["command"], "borrowlend withdraw");
    assert_eq!(json["would_execute"], "withdraw_borrowlend");
    assert_eq!(json["args"]["operation"], "withdraw");
    assert_eq!(json["args"]["encoded_operation"], 1);
    assert_eq!(json["args"]["max"], true);
    assert_eq!(json["args"]["network"], "Testnet");
    assert_eq!(json["args"]["wei"], 0);
    assert_eq!(json["args"]["reversibility"], "partially_reversible");
    assert!(json["signer"].as_str().unwrap().starts_with("0x"));
    assert!(json["acting_as"].as_str().unwrap().starts_with("0x"));
    assert!(json["vault_address"].is_null());
}

#[tokio::test]
async fn borrowlend_action_validation_fails_before_auth() {
    let env = IsolatedHome::new();
    let server = mock_borrowlend_server().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args([
            "--dry-run",
            "borrowlend",
            "supply",
            "USDC",
            "--amount",
            "0",
            "--testnet",
        ])
        .assert()
        .code(13)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "borrow/lend amount must be greater than zero",
        ));
}

#[tokio::test]
async fn borrowlend_live_actions_submit_verified_exchange_shape() {
    let env = IsolatedHome::new();
    let server = mock_borrowlend_server().await;

    let supply_output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "borrowlend",
            "supply",
            "USDC",
            "--amount",
            "5",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let supply_json: Value = serde_json::from_slice(&supply_output).unwrap();
    assert_eq!(supply_json["status"], "submitted");
    assert_eq!(supply_json["action"], "borrowlend");
    assert_eq!(supply_json["operation"], "supply");
    assert_eq!(supply_json["token"], "USDC");
    assert_eq!(supply_json["token_index"], 0);
    assert_eq!(supply_json["amount"], "5");
    assert_eq!(supply_json["max"], false);
    assert_eq!(supply_json["network"], "Testnet");
    assert!(supply_json["signer"].as_str().unwrap().starts_with("0x"));
    assert!(supply_json["acting_as"].as_str().unwrap().starts_with("0x"));
    assert_eq!(supply_json["reversibility"], "partially_reversible");

    let withdraw_output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "borrowlend",
            "withdraw",
            "USDC",
            "--max",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let withdraw_json: Value = serde_json::from_slice(&withdraw_output).unwrap();
    assert_eq!(withdraw_json["operation"], "withdraw");
    assert_eq!(withdraw_json["amount"], Value::Null);
    assert_eq!(withdraw_json["max"], true);
    assert!(
        withdraw_json["acting_as"]
            .as_str()
            .unwrap()
            .starts_with("0x")
    );
    assert_eq!(withdraw_json["reversibility"], "partially_reversible");

    let requests = server.received_requests().await.unwrap();
    let exchange_actions = requests
        .iter()
        .filter(|request| request.url.path() == "/exchange")
        .map(|request| serde_json::from_slice::<Value>(&request.body).unwrap()["action"].clone())
        .collect::<Vec<_>>();

    assert_eq!(exchange_actions.len(), 2);
    assert_eq!(exchange_actions[0]["type"], "borrowLend");
    assert_eq!(exchange_actions[0]["operation"], "supply");
    assert_eq!(exchange_actions[0]["token"], 0);
    assert_eq!(exchange_actions[0]["amount"], "5");
    assert_eq!(exchange_actions[1]["type"], "borrowLend");
    assert_eq!(exchange_actions[1]["operation"], "withdraw");
    assert_eq!(exchange_actions[1]["token"], 0);
    assert_eq!(exchange_actions[1]["amount"], Value::Null);
}
