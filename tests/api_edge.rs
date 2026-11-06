//! The raw `/info` paths share the API client's retry and status policy.

mod support;

use serde_json::Value;
use support::{
    API_OVERRIDE_ENV, IsolatedHome, PRIVATE_KEY_ENV, VALID_PRIVATE_KEY, fixture_all_mids,
    fixture_perp_meta_btc_only, fixture_spot_meta_usdc_only,
};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn mount_info(server: &MockServer, request_type: &str, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": request_type})))
        .respond_with(response)
        .mount(server)
        .await;
}

async fn market_server() -> MockServer {
    let server = MockServer::start().await;
    let ok = |body: Value| ResponseTemplate::new(200).set_body_json(body);
    mount_info(&server, "allMids", ok(fixture_all_mids())).await;
    mount_info(&server, "spotMeta", ok(fixture_spot_meta_usdc_only())).await;
    mount_info(&server, "meta", ok(fixture_perp_meta_btc_only())).await;
    mount_info(&server, "perpDexs", ok(serde_json::json!([]))).await;
    server
}

async fn requests_of_type(server: &MockServer, request_type: &str) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| {
            serde_json::from_slice::<Value>(&request.body)
                .is_ok_and(|body| body["type"] == request_type)
        })
        .count()
}

fn error_json(stdout: &[u8]) -> Value {
    serde_json::from_slice(stdout).expect("stdout should be a JSON error envelope")
}

#[tokio::test]
async fn meta_retries_a_failing_read_then_reports_an_api_failure() {
    let env = IsolatedHome::new();
    let server = market_server().await;
    mount_info(
        &server,
        "metaAndAssetCtxs",
        ResponseTemplate::new(503).set_body_string("down"),
    )
    .await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "meta"])
        .assert()
        .code(12)
        .get_output()
        .stdout
        .clone();

    let error = error_json(&output);
    assert_eq!(error["reason"], "api_error");
    assert_eq!(error["retryable"], true);
    assert_eq!(requests_of_type(&server, "metaAndAssetCtxs").await, 3);
}

#[tokio::test]
async fn raw_historical_orders_fallback_reports_a_rejected_status_as_an_api_failure() {
    let env = IsolatedHome::new();
    let server = market_server().await;
    // The SDK decode fails on this shape, which sends the CLI to the raw read.
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "historicalOrders"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([1])))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    mount_info(
        &server,
        "historicalOrders",
        ResponseTemplate::new(422).set_body_string("rejected"),
    )
    .await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["--format", "json", "orders", "history"])
        .assert()
        .code(12)
        .get_output()
        .stdout
        .clone();

    assert_eq!(error_json(&output)["reason"], "api_error");
    assert_eq!(requests_of_type(&server, "historicalOrders").await, 2);
}
