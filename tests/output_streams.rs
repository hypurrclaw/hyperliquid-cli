// Integration tests for how output is shaped per stream: styling, tables and
// JSON on pipes, empty-state and truncation notices, and cell sanitization.
//
// `assert_cmd` runs the binary with piped stdout and stderr, so every run here
// is a non-TTY run unless a test forces color through the environment.

mod support;

use predicates::prelude::*;
use serde_json::{Value, json};
use support::{API_OVERRIDE_ENV, IsolatedHome, mount_override_healthcheck};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn validator(name: &str, index: u64) -> Value {
    json!({
        "validator": format!("0x{:040x}", index),
        "signer": "0x0000000000000000000000000000000000000003",
        "name": name,
        "description": "test validator",
        "nRecentBlocks": 1,
        "stake": 123456789_u64,
        "isJailed": false,
        "unjailableAfter": null,
        "isActive": true,
        "commission": "0.04",
        "stats": [
            ["day", {"uptimeFraction": "1.0", "predictedApr": "0.025", "nSamples": 1440}]
        ]
    })
}

async fn validators_server(validators: Vec<Value>) -> MockServer {
    let server = MockServer::start().await;
    mount_override_healthcheck(&server).await;
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(json!({"type": "validatorSummaries"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(Value::Array(validators)))
        .mount(&server)
        .await;
    server
}

fn run(env: &IsolatedHome, server: &MockServer, args: &[&str]) -> assert_cmd::assert::Assert {
    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .arg("--no-update-check")
        .args(args)
        .assert()
        .success()
}

#[tokio::test]
async fn table_on_a_pipe_is_tab_separated_without_borders() {
    let env = IsolatedHome::new();
    let server = validators_server(vec![validator("Validator One", 2)]).await;

    let output = run(
        &env,
        &server,
        &["--format", "table", "staking", "validators"],
    )
    .stderr(predicate::str::is_empty())
    .get_output()
    .clone();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(!stdout.contains(['╭', '│', '╰', '─']), "{stdout}");
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2, "{stdout}");
    let columns = lines[0].split('\t').count();
    assert!(columns > 1);
    assert_eq!(lines[1].split('\t').count(), columns);
    assert!(lines[1].contains("Validator One"));
}

#[tokio::test]
async fn pretty_on_a_pipe_has_no_ansi_and_no_timing_footer() {
    let env = IsolatedHome::new();
    let server = validators_server(vec![validator("Validator One", 2)]).await;

    run(
        &env,
        &server,
        &["--format", "pretty", "staking", "validators"],
    )
    .stdout(predicate::str::contains("Validator One"))
    .stdout(predicate::str::contains('\u{1b}').not())
    .stderr(predicate::str::contains("Completed in").not())
    .stderr(predicate::str::is_empty());
}

#[tokio::test]
async fn color_precedence_follows_no_color_then_force_color() {
    let env = IsolatedHome::new();
    let server = validators_server(vec![validator("Validator One", 2)]).await;
    let args = ["--format", "pretty", "staking", "validators"];

    let forced = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env("FORCE_COLOR", "1")
        .arg("--no-update-check")
        .args(args)
        .assert()
        .success();
    forced.stdout(predicate::str::contains("\u{1b}[36m"));

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env("FORCE_COLOR", "1")
        .env("NO_COLOR", "1")
        .arg("--no-update-check")
        .args(args)
        .assert()
        .success()
        .stdout(predicate::str::contains('\u{1b}').not());

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env("FORCE_COLOR", "0")
        .arg("--no-update-check")
        .args(args)
        .assert()
        .success()
        .stdout(predicate::str::contains('\u{1b}').not());
}

#[tokio::test]
async fn json_on_a_pipe_is_one_compact_line() {
    let env = IsolatedHome::new();
    let server = validators_server(vec![validator("Validator One", 2)]).await;

    let output = run(
        &env,
        &server,
        &["--format", "json", "staking", "validators"],
    )
    .stderr(predicate::str::is_empty())
    .get_output()
    .clone();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert_eq!(stdout.trim_end().lines().count(), 1, "{stdout}");
    serde_json::from_str::<Value>(&stdout).unwrap();
}

#[tokio::test]
async fn empty_result_prints_header_on_stdout_and_sentence_on_stderr() {
    let env = IsolatedHome::new();
    let server = validators_server(vec![]).await;

    let output = run(
        &env,
        &server,
        &["--format", "table", "staking", "validators"],
    )
    .get_output()
    .clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert!(!stdout.contains("Message"), "{stdout}");
    assert!(!stderr.trim().is_empty());
    assert_eq!(stderr.trim().lines().count(), 1, "{stderr}");

    run(
        &env,
        &server,
        &["--format", "json", "staking", "validators"],
    )
    .stdout(predicate::str::diff("[]\n"))
    .stderr(predicate::str::is_empty());
}

#[tokio::test]
async fn max_results_truncation_is_reported_on_stderr_for_human_formats_only() {
    let env = IsolatedHome::new();
    let server = validators_server(vec![validator("Validator One", 2), validator("Two", 4)]).await;
    let notice = "Showing 1 of 2. Remove --max-results to see all.";

    for format in ["table", "pretty"] {
        run(
            &env,
            &server,
            &[
                "--format",
                format,
                "--max-results",
                "1",
                "staking",
                "validators",
            ],
        )
        .stderr(predicate::str::contains(notice));
    }

    run(
        &env,
        &server,
        &[
            "--format",
            "json",
            "--max-results",
            "1",
            "staking",
            "validators",
        ],
    )
    .stderr(predicate::str::is_empty());
}

#[tokio::test]
async fn cells_with_control_and_bidi_characters_are_escaped() {
    let env = IsolatedHome::new();
    let hostile = "evil\u{1b}[31m\nFORGED\tROW\u{202e}REV";
    let server = validators_server(vec![validator(hostile, 6)]).await;

    for format in ["table", "pretty"] {
        let output = run(
            &env,
            &server,
            &["--format", format, "staking", "validators"],
        )
        .get_output()
        .clone();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains("evil\\x1b[31m\\nFORGED\\tROW\\u{202e}REV"),
            "{stdout}"
        );
        assert!(!stdout.contains('\u{1b}'), "{stdout}");
        assert!(!stdout.contains('\u{202e}'), "{stdout}");
        assert!(!stdout.contains("\nFORGED"), "{stdout}");
    }
}
