mod support;

use predicates::prelude::*;
use serde_json::Value;
use std::fs;

use support::IsolatedHome;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn feedback_posts_structured_scenario_json() {
    let home = IsolatedHome::new();
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/feedback"))
        .and(body_partial_json(serde_json::json!({
            "source": "hyperliquid-cli",
            "scenario": {
                "command": "orders create",
                "expected": "dry-run preview",
                "actual": "unexpected error"
            },
            "contact": "agent@example.test",
            "tags": ["bug", "agent"]
        })))
        .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
            "status": "accepted",
            "id": "fb_test_123"
        })))
        .mount(&server)
        .await;

    let output = home
        .command()
        .args([
            "--no-update-check",
            "--format",
            "json",
            "feedback",
            "--url",
            &format!("{}/feedback", server.uri()),
            "--scenario-json",
            r#"{"command":"orders create","expected":"dry-run preview","actual":"unexpected error"}"#,
            "--contact",
            "agent@example.test",
            "--tags",
            "bug,agent",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let value: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["status"], "accepted");
    assert_eq!(value["id"], "fb_test_123");
}

#[tokio::test]
async fn feedback_does_not_require_hyperliquid_app_context() {
    let home = IsolatedHome::new();
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/feedback"))
        .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
            "status": "accepted",
            "id": "fb_config_broken"
        })))
        .mount(&server)
        .await;

    home.command()
        .args([
            "--no-update-check",
            "--format",
            "json",
            "feedback",
            "--url",
            &format!("{}/feedback", server.uri()),
            "--scenario-json",
            r#"{"command":"status","actual":"config failed"}"#,
        ])
        .env("HYPERLIQUID_NETWORK", "not-a-network")
        .assert()
        .success()
        .stdout(predicate::str::contains("fb_config_broken"));
}

#[tokio::test]
async fn feedback_accepts_scenario_file_and_stdin() {
    let home = IsolatedHome::new();
    let server = MockServer::start().await;
    let scenario_path = home.tmp_path().join("scenario.json");
    fs::write(
        &scenario_path,
        r#"{"command":"mids","expected":"prices","actual":"empty"}"#,
    )
    .unwrap();

    Mock::given(method("POST"))
        .and(path("/feedback"))
        .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
            "status": "accepted"
        })))
        .mount(&server)
        .await;

    home.command()
        .args([
            "--no-update-check",
            "--format",
            "json",
            "feedback",
            "--url",
            &format!("{}/feedback", server.uri()),
            "--scenario-file",
            "scenario.json",
        ])
        .current_dir(home.tmp_path())
        .assert()
        .success();

    home.command()
        .args([
            "--no-update-check",
            "--format",
            "json",
            "feedback",
            "--url",
            &format!("{}/feedback", server.uri()),
            "--scenario-file",
            "-",
        ])
        .write_stdin(r#"{"command":"status","actual":"ok"}"#)
        .assert()
        .success();
}

#[test]
fn feedback_requires_object_scenario_json() {
    let home = IsolatedHome::new();

    home.command()
        .args([
            "--no-update-check",
            "--format",
            "json",
            "feedback",
            "--url",
            "http://127.0.0.1:8787/feedback",
            "--scenario-json",
            r#"["not", "an", "object"]"#,
        ])
        .assert()
        .code(13)
        .stdout(predicate::str::contains("scenario JSON must be an object"));
}
