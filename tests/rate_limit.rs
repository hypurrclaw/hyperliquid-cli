use assert_cmd::Command;
use predicates::prelude::*;
use std::net::TcpListener;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const API_OVERRIDE_ENV: &str = "HYPERLIQUID_API_BASE_URL";

fn hyperliquid_command() -> Command {
    let mut command = Command::cargo_bin("hyperliquid").unwrap();
    command
        .env("HYPERLIQUID_FORMAT", "pretty")
        .env_remove("HYPERLIQUID_PRIVATE_KEY")
        .env_remove("HYPERLIQUID_NETWORK");
    command
}

#[tokio::test]
async fn cli_exits_11_for_actual_http_429_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .respond_with(
            ResponseTemplate::new(429).set_body_string("Too many requests: rate limit exceeded"),
        )
        .mount(&server)
        .await;

    hyperliquid_command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["perps", "list"])
        .assert()
        .code(11)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Rate limited"))
        .stderr(predicate::str::contains("Unable to reach").not());
}

#[tokio::test]
async fn cli_exits_11_for_structured_rate_limit_error_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "err",
            "response": "rate limit exceeded"
        })))
        .mount(&server)
        .await;

    hyperliquid_command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["status"])
        .assert()
        .code(11)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Rate limited"))
        .stderr(predicate::str::contains("Unable to reach").not());
}

#[test]
fn cli_exits_12_for_unreachable_override_endpoint() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);

    hyperliquid_command()
        .env(API_OVERRIDE_ENV, url)
        .args(["perps", "list"])
        .assert()
        .code(12)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Unable to reach Hyperliquid API"))
        .stderr(predicate::str::contains("Rate limited").not());
}
