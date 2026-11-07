#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use hyperliquid_cli::command_registry::CommandRegistry;
use hyperliquid_cli::command_spec::Risk;
use hyperliquid_cli::commands::schema::schema_query;
use hyperliquid_cli::output::TableData;
use serde_json::{Value, json};

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

pub fn fixture_path(name: &str) -> PathBuf {
    repo_root().join("tests/fixtures/contracts").join(name)
}

pub fn read_fixture(name: &str) -> String {
    let path = fixture_path(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read contract fixture {}: {err}", path.display()))
}

pub fn assert_json_fixture(name: &str, actual: &Value) {
    let pretty = format!("{}\n", serde_json::to_string_pretty(actual).unwrap());
    let path = fixture_path(name);

    if std::env::var_os("HYPERLIQUID_UPDATE_CONTRACTS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, pretty).unwrap();
        return;
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "missing contract fixture {} ({err}); rerun with HYPERLIQUID_UPDATE_CONTRACTS=1 and review the diff",
            path.display()
        )
    });
    assert_eq!(
        expected,
        pretty,
        "contract fixture {} drifted; rerun with HYPERLIQUID_UPDATE_CONTRACTS=1 only when the contract change is intentional and reviewed",
        path.display()
    );
}

pub fn schema_all() -> Vec<Value> {
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["--format", "json", "schema"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).unwrap()
}

pub fn command_path(schema: &Value) -> String {
    schema["command_path"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| part.as_str().unwrap())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The command schemas exactly as `hyperliquid schema` prints them.
fn registry_schemas() -> Vec<Value> {
    match schema_query(&[]).unwrap().to_json_value() {
        Value::Array(schemas) => schemas,
        other => panic!("schema output is not an array: {other}"),
    }
}

fn wrap(commands: impl Into<Value>) -> Value {
    json!({
        "characterization": true,
        "review_required_to_update": true,
        "commands": commands.into(),
    })
}

/// Every fixture derived from the command registry, computed in one pass. This is
/// the only place their contents are defined: `tests/contract_fixtures.rs` checks
/// them and, with `HYPERLIQUID_UPDATE_CONTRACTS=1`, rewrites them.
pub fn registry_fixtures() -> Vec<(&'static str, Value)> {
    let registry = CommandRegistry::get();
    let schemas = registry_schemas();

    let inventory = registry
        .commands()
        .iter()
        .map(|command| {
            let input_kinds = command
                .inputs
                .iter()
                .filter_map(|input| Some((input.id.clone(), input.kind?)))
                .collect::<std::collections::BTreeMap<_, _>>();
            json!({
                "command": command.command,
                "command_path": command.command_path,
                "group": command.group,
                "auth_required": command.auth_required,
                "dangerous": command.dangerous,
                "risk": command.risk,
                "mutability": command.lifecycle,
                "dry_run": command.dry_run,
                "raw_payload": command.raw_payload,
                "confirmation": command.confirmation,
                "transport": command.transport,
                "input_kinds": input_kinds,
                "output_contract": command.output_contract,
            })
        })
        .collect::<Vec<_>>();

    let high_risk = registry
        .commands()
        .iter()
        .filter(|command| {
            command.dangerous
                || matches!(
                    command.risk,
                    Risk::FundsMovement | Risk::LocalSecret | Risk::LocalState
                )
        })
        .map(|command| {
            json!({
                "command": command.command,
                "auth_required": command.auth_required,
                "dangerous": command.dangerous,
                "lifecycle": command.lifecycle,
                "risk": command.risk,
                "dry_run": command.dry_run,
                "raw_payload": command.raw_payload,
                "confirmation": command.confirmation,
                "ows_signer": command.ows_signer,
            })
        })
        .collect::<Vec<_>>();

    let representative = [
        "orders create",
        "orders modify",
        "transfer send",
        "wallet address",
        "borrowlend supply",
        "subscribe all-mids",
    ]
    .iter()
    .map(|command| {
        schemas
            .iter()
            .find(|schema| command_path(schema) == *command)
            .unwrap_or_else(|| panic!("missing schema for {command}"))
            .clone()
    })
    .collect::<Vec<_>>();

    vec![
        ("command_inventory.json", wrap(inventory)),
        ("registry_inventory.json", wrap(json!(registry.commands()))),
        ("schema_high_risk_metadata.json", wrap(high_risk)),
        ("schema_representative.json", wrap(representative)),
    ]
}
