mod support;

use serde_json::Value;
use support::{API_OVERRIDE_ENV, IsolatedHome};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const USER: &str = "0x00000000000000000000000000000000000000aa";

async fn mock_info_server() -> MockServer {
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

fn json_stdout(output: Vec<u8>) -> Value {
    serde_json::from_slice(&output).expect("stdout should be valid JSON")
}

#[tokio::test]
async fn account_abstraction_read_returns_normalized_mode() {
    let env = IsolatedHome::new();
    let server = mock_info_server().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "userAbstraction",
            "user": USER
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::Value::String("unifiedAccount".to_string())),
        )
        .mount(&server)
        .await;

    let out = env
        .command_with_server(&server)
        .args(["--format", "json", "account", "abstraction", USER])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = json_stdout(out);
    assert_eq!(
        json["user"].as_str().unwrap().to_ascii_lowercase(),
        USER.to_ascii_lowercase()
    );
    assert_eq!(json["raw_mode"], "unifiedAccount");
    assert_eq!(json["normalized_mode"], "unified-account");
}

#[test]
fn account_abstraction_set_dry_run_includes_user_set_action() {
    let env = IsolatedHome::new();
    let out = env
        .command()
        .args([
            "--format",
            "json",
            "--dry-run",
            "account",
            "abstraction",
            "set",
            "--mode",
            "disabled",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["command"], "account abstraction set");
    assert_eq!(json["would_execute"], "set_abstraction");
    assert_eq!(json["args"]["mode"], "disabled");
    assert_eq!(json["args"]["protocol_mode"], "disabled");
    assert_eq!(json["args"]["action"]["type"], "userSetAbstraction");
    assert_eq!(json["args"]["action"]["abstraction"], "disabled");
}

#[tokio::test]
async fn account_abstraction_read_uses_api_override() {
    let env = IsolatedHome::new();
    let server = mock_info_server().await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "userAbstraction",
            "user": USER
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::Value::String("portfolioMargin".to_string())),
        )
        .mount(&server)
        .await;

    let mut cmd = env.command();
    cmd.env(API_OVERRIDE_ENV, server.uri());
    let out = cmd
        .args(["--format", "json", "account", "abstraction", USER])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json = json_stdout(out);
    assert_eq!(json["normalized_mode"], "portfolio-margin");
    assert_eq!(json["raw_mode"], "portfolioMargin");
}
