mod support;

use support::{IsolatedHome, fixture_fill};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const USER: &str = "0x0000000000000000000000000000000000000000";
const START_MS: u64 = 1_777_593_600_000;
const END_MS: u64 = 1_777_680_000_000;

async fn history_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "allMids"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "BTC": "50000"
        })))
        .mount(&server)
        .await;
    server
}

async fn mount_info(server: &MockServer, expected: serde_json::Value, response: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(expected))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .mount(server)
        .await;
}

fn json_stdout(output: Vec<u8>) -> serde_json::Value {
    serde_json::from_slice(&output).expect("stdout should be valid JSON")
}

#[tokio::test]
async fn account_history_endpoints_fees_rate_limit_and_portfolio_history_are_snake_case() {
    let env = IsolatedHome::new();
    let server = history_server().await;

    mount_info(
        &server,
        serde_json::json!({"type": "userFees", "user": USER}),
        serde_json::json!({
            "dailyUserVlm": "123.45",
            "feeSchedule": {
                "cross": "0.00035",
                "add": "0.0001"
            }
        }),
    )
    .await;
    mount_info(
        &server,
        serde_json::json!({"type": "userRateLimit", "user": USER}),
        serde_json::json!({
            "nRequestsUsed": 7,
            "nRequestsCap": 1000
        }),
    )
    .await;
    mount_info(
        &server,
        serde_json::json!({"type": "portfolio", "user": USER}),
        serde_json::json!({
            "day": {
                "accountValueHistory": [[START_MS, "1000"]],
                "pnlHistory": [[START_MS, "12.5"]],
                "vlm": "50"
            },
            "allTime": {
                "accountValueHistory": [[START_MS, "900"], [END_MS, "1000"]]
            },
            "perpDayVlm": "25"
        }),
    )
    .await;

    let fees = json_stdout(
        env.command_with_server(&server)
            .args(["--format", "json", "account", "fees", USER])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert_eq!(fees["daily_user_vlm"], "123.45");
    assert_eq!(fees["fee_schedule"]["cross"], "0.00035");

    let rate_limit = json_stdout(
        env.command_with_server(&server)
            .args(["--format", "json", "account", "rate-limit", USER])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert_eq!(rate_limit["n_requests_used"], 7);
    assert_eq!(rate_limit["n_requests_cap"], 1000);

    let portfolio = json_stdout(
        env.command_with_server(&server)
            .args(["--format", "json", "account", "portfolio-history", USER])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert!(portfolio.get("day").is_some());
    assert!(portfolio.get("all_time").is_some());
    assert_eq!(portfolio["day"]["account_value_history"][0][1], "1000");
    assert_eq!(portfolio["perp_day_vlm"], "25");
}

#[tokio::test]
async fn account_history_endpoints_fills_by_time_parses_rfc3339_and_projects_json() {
    let env = IsolatedHome::new();
    let server = history_server().await;
    mount_info(
        &server,
        serde_json::json!({
            "type": "userFillsByTime",
            "user": USER,
            "startTime": START_MS,
            "endTime": END_MS,
            "aggregateByTime": true
        }),
        serde_json::json!([fixture_fill("BTC", 4242)]),
    )
    .await;

    let json = json_stdout(
        env.command_with_server(&server)
            .args([
                "--format",
                "json",
                "--select",
                "coin,time",
                "account",
                "fills",
                USER,
                "--start",
                "2026-05-01T00:00:00Z",
                "--end",
                "2026-05-02T00:00:00Z",
                "--aggregate-by-time",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let first = json
        .as_array()
        .unwrap()
        .first()
        .unwrap()
        .as_object()
        .unwrap();
    assert_eq!(
        first.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["coin", "time"]
    );
    assert_eq!(first["coin"], "BTC");
    assert_eq!(first["time"], 1_700_000_000_000_u64);
}

#[test]
fn account_fills_aggregate_by_time_requires_start() {
    let env = IsolatedHome::new();

    let output = env
        .command()
        .args([
            "--format",
            "json",
            "account",
            "fills",
            USER,
            "--aggregate-by-time",
        ])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let usage = json_stdout(output);

    assert_eq!(usage["category"], "usage");
    assert_eq!(usage["exit_code"], 2);
    assert!(usage["error"].as_str().unwrap().contains("--start"));
}

#[tokio::test]
async fn account_history_endpoints_ledger_funding_and_twap_payloads_are_time_bounded() {
    let env = IsolatedHome::new();
    let server = history_server().await;

    mount_info(
        &server,
        serde_json::json!({
            "type": "userNonFundingLedgerUpdates",
            "user": USER,
            "startTime": START_MS,
            "endTime": END_MS
        }),
        serde_json::json!([
            {
                "time": START_MS,
                "hash": "0xabc",
                "delta": {
                    "type": "deposit",
                    "amount": "100"
                }
            }
        ]),
    )
    .await;
    mount_info(
        &server,
        serde_json::json!({
            "type": "userFunding",
            "user": USER,
            "startTime": START_MS,
            "endTime": END_MS
        }),
        serde_json::json!([
            {
                "time": START_MS,
                "coin": "BTC",
                "usdc": "1.25",
                "szi": "0.5",
                "fundingRate": "0.0001"
            }
        ]),
    )
    .await;
    mount_info(
        &server,
        serde_json::json!({"type": "twapHistory", "user": USER}),
        serde_json::json!([
            {
                "state": {
                    "coin": "BTC",
                    "twapId": 55
                },
                "status": {
                    "status": "finished"
                }
            }
        ]),
    )
    .await;
    mount_info(
        &server,
        serde_json::json!({
            "type": "userTwapSliceFillsByTime",
            "user": USER,
            "startTime": START_MS,
            "endTime": END_MS,
            "aggregateByTime": true
        }),
        serde_json::json!([
            {
                "twapId": 55,
                "fill": fixture_fill("BTC", 4242)
            }
        ]),
    )
    .await;

    let ledger = json_stdout(
        env.command_with_server(&server)
            .args([
                "--format",
                "json",
                "account",
                "ledger",
                USER,
                "--start",
                "1777593600000",
                "--end",
                "1777680000000",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert_eq!(ledger[0]["delta"]["type"], "deposit");
    assert_eq!(ledger[0]["delta"]["amount"], "100");

    let funding = json_stdout(
        env.command_with_server(&server)
            .args([
                "--format",
                "json",
                "account",
                "funding",
                USER,
                "--start",
                "1777593600000",
                "--end",
                "1777680000000",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert_eq!(funding[0]["funding_rate"], "0.0001");

    let twap_history = json_stdout(
        env.command_with_server(&server)
            .args(["--format", "json", "account", "twap-history", USER])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert_eq!(twap_history[0]["state"]["twap_id"], 55);

    let twap_fills = json_stdout(
        env.command_with_server(&server)
            .args([
                "--format",
                "json",
                "account",
                "twap-fills",
                USER,
                "--start",
                "1777593600000",
                "--end",
                "1777680000000",
                "--aggregate-by-time",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert_eq!(twap_fills[0]["twap_id"], 55);
    assert_eq!(twap_fills[0]["fill"]["coin"], "BTC");
}

#[tokio::test]
async fn account_history_endpoints_order_status_uses_public_order_status_payload() {
    let env = IsolatedHome::new();
    let server = history_server().await;
    mount_info(
        &server,
        serde_json::json!({
            "type": "orderStatus",
            "user": USER,
            "oid": 123
        }),
        serde_json::json!({
            "status": "order",
            "order": {
                "status": "filled",
                "statusTimestamp": 1_700_000_000_001_u64,
                "order": {
                    "coin": "BTC",
                    "oid": 123
                }
            }
        }),
    )
    .await;

    let status = json_stdout(
        env.command_with_server(&server)
            .args([
                "--format", "json", "orders", "status", "--user", USER, "--oid", "123",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert_eq!(status["order"]["status"], "filled");
    assert_eq!(status["order"]["status_timestamp"], 1_700_000_000_001_u64);
}

#[tokio::test]
async fn account_history_endpoints_order_status_normalizes_cloid_payload() {
    let env = IsolatedHome::new();
    let server = history_server().await;
    mount_info(
        &server,
        serde_json::json!({
            "type": "orderStatus",
            "user": USER,
            "oid": "0x00000000000000000000000000000abc"
        }),
        serde_json::json!({
            "status": "order",
            "order": {
                "status": "open",
                "statusTimestamp": 1_700_000_000_002_u64,
                "order": {
                    "coin": "BTC",
                    "oid": 124
                }
            }
        }),
    )
    .await;

    let status = json_stdout(
        env.command_with_server(&server)
            .args([
                "--format", "json", "orders", "status", "--user", USER, "--cloid", "0xabc",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert_eq!(status["order"]["status"], "open");
    assert_eq!(status["order"]["status_timestamp"], 1_700_000_000_002_u64);
}

#[tokio::test]
async fn account_history_endpoints_staking_history_is_exposed() {
    let env = IsolatedHome::new();
    let server = history_server().await;
    mount_info(
        &server,
        serde_json::json!({"type": "delegatorHistory", "user": USER}),
        serde_json::json!([
            {
                "time": START_MS,
                "hash": "0xdef",
                "delta": {
                    "type": "delegate",
                    "validator": "0x1111111111111111111111111111111111111111",
                    "amount": "250000000"
                }
            }
        ]),
    )
    .await;

    let history = json_stdout(
        env.command_with_server(&server)
            .args(["--format", "json", "staking", "history", USER])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert_eq!(history[0]["delta"]["type"], "delegate");
    assert_eq!(history[0]["delta"]["amount"], "250000000");
}
