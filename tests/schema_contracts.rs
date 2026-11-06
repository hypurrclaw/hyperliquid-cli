mod contract_support;

use serde_json::json;

use assert_cmd::Command;
use contract_support::schema_all;

#[test]
fn subscribe_schema_exposes_machine_context_stream_bounds() {
    let schemas = schema_all();
    let schema = schemas
        .iter()
        .find(|schema| schema["command_path"] == json!(["subscribe", "all-mids"]))
        .unwrap();

    assert_eq!(schema["output_contract"], "bounded_ndjson_stream");
    assert_eq!(schema["stream_bounds"]["required_in_machine_context"], true);
    assert_eq!(
        schema["json_schema"]["x-hyperliquid"]["stream_bounds"]["env"],
        json!(["HYPERLIQUID_SUBSCRIBE_MAX_EVENTS"])
    );
}

#[test]
fn snapshot_watch_schema_exposes_conditional_stream_bounds() {
    let schemas = schema_all();
    let schema = schemas
        .iter()
        .find(|schema| schema["command_path"] == json!(["mids"]))
        .unwrap();

    assert_eq!(
        schema["stream_bounds"]["required_when"],
        json!({"watch": true})
    );
    assert_eq!(schema["stream_bounds"]["cli_args"], json!(["max_ticks"]));
    assert_eq!(
        schema["json_schema"]["x-hyperliquid"]["stream_bounds"]["env"],
        json!(["HYPERLIQUID_WATCH_MAX_TICKS"])
    );
}

#[test]
fn wallet_export_schema_advertises_yes_as_confirmation_bypass() {
    let schemas = schema_all();
    let schema = schemas
        .iter()
        .find(|schema| schema["command_path"] == json!(["wallet", "export"]))
        .unwrap();

    assert_eq!(schema["confirmation_bypass"]["supported"], true);
    assert_eq!(schema["confirmation_bypass"]["arg"], "yes");
    assert_eq!(
        schema["json_schema"]["x-hyperliquid"]["confirmation_bypass"]["supported"],
        true
    );
}

#[test]
fn schema_group_lookup_returns_child_schemas() {
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .args([
            "--format",
            "json",
            "--select",
            "command,description",
            "--max-results",
            "2",
            "schema",
            "subscribe",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let schemas: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let rows = schemas.as_array().unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["command"], "hyperliquid subscribe trades");
    assert!(rows[0].get("description").is_some());
}

#[test]
fn schema_leaf_lookup_still_returns_single_schema_object() {
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["--format", "json", "schema", "account", "portfolio"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let schema: serde_json::Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(schema["command"], "hyperliquid account portfolio <ADDRESS>");
    assert_eq!(schema["command_path"], json!(["account", "portfolio"]));
}
