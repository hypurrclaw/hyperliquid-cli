mod contract_support;

use std::collections::{BTreeMap, BTreeSet};

use assert_cmd::Command;
use serde_json::{Value, json};

use contract_support::{assert_json_fixture, command_path, schema_all};

#[test]
fn command_inventory_matches_characterization_fixture() {
    let schemas = schema_all();
    let inventory = schemas.iter().map(inventory_entry).collect::<Vec<_>>();

    assert_json_fixture(
        "command_inventory.json",
        &json!({
            "characterization": true,
            "review_required_to_update": true,
            "commands": inventory,
        }),
    );
}

#[test]
fn every_catalog_command_resolves_to_cli_help() {
    for schema in schema_all() {
        let path = command_path(&schema);
        let args = path
            .split_whitespace()
            .chain(["--help"])
            .collect::<Vec<_>>();
        Command::cargo_bin("hyperliquid")
            .unwrap()
            .args(args)
            .assert()
            .success();
    }
}

#[test]
fn every_cli_leaf_is_cataloged_or_explicitly_excluded() {
    let cataloged = schema_all()
        .iter()
        .map(command_path)
        .collect::<BTreeSet<_>>();
    let discovered = discover_cli_leaves(Vec::new());
    let excluded = BTreeSet::from(["help".to_string()]);

    let missing = discovered
        .difference(&cataloged)
        .filter(|path| !excluded.contains(*path))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "CLI leaf commands missing from catalog or explicit exclusions: {missing:?}"
    );
}

fn inventory_entry(schema: &Value) -> Value {
    let metadata = &schema["json_schema"]["x-hyperliquid"];
    json!({
        "command": schema["command"],
        "command_path": schema["command_path"],
        "group": schema["group"],
        "auth_required": schema["auth_required"],
        "dangerous": schema["dangerous"],
        "risk": schema["risk"],
        "mutability": schema["lifecycle"],
        "dry_run": schema["dry_run"],
        "raw_payload": schema["raw_payload"],
        "confirmation": schema["confirmation"],
        "transport": transport_for(schema),
        "input_kinds": input_kinds(schema),
        "output_contract": output_contract_for(schema, metadata),
    })
}

fn transport_for(schema: &Value) -> Value {
    let lifecycle = schema["lifecycle"].as_str().unwrap_or_default();
    let transport = match lifecycle {
        "interactive_local" => vec!["cli_interactive"],
        "blocked_unsupported" => vec!["cli_blocked"],
        _ => vec!["cli_process"],
    };
    json!(transport)
}

fn input_kinds(schema: &Value) -> Value {
    let mut input_kinds = BTreeMap::new();
    if let Some(properties) = schema["json_schema"]["properties"].as_object() {
        for (arg, prop) in properties {
            if let Some(input_kind) = prop.get("input_kind").and_then(Value::as_str) {
                input_kinds.insert(arg.clone(), input_kind.to_string());
            }
        }
    }
    json!(input_kinds)
}

fn output_contract_for(schema: &Value, metadata: &Value) -> Value {
    let path = command_path(schema);
    let lifecycle = metadata["lifecycle"].as_str().unwrap_or_default();
    let contract = if path.starts_with("subscribe ") {
        "bounded_ndjson_stream"
    } else if path == "schema" {
        "schema_array_or_object"
    } else if lifecycle == "interactive_local" {
        "interactive_local"
    } else if metadata["dry_run"] == "optional" {
        "command_result_or_dry_run_envelope"
    } else {
        "json_value"
    };
    json!(contract)
}

fn discover_cli_leaves(prefix: Vec<String>) -> BTreeSet<String> {
    let mut args = prefix.iter().map(String::as_str).collect::<Vec<_>>();
    args.push("--help");
    let output = Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help = String::from_utf8(output).unwrap();
    let subcommands = parse_help_subcommands(&help);
    if subcommands.is_empty() {
        return BTreeSet::from([prefix.join(" ")]);
    }

    subcommands
        .into_iter()
        .flat_map(|subcommand| {
            let mut path = prefix.clone();
            path.push(subcommand);
            discover_cli_leaves(path)
        })
        .collect()
}

fn parse_help_subcommands(help: &str) -> Vec<String> {
    let mut in_commands = false;
    let mut commands = Vec::new();

    for line in help.lines() {
        if line == "Commands:" {
            in_commands = true;
            continue;
        }
        if in_commands && !line.starts_with("  ") {
            break;
        }
        if !in_commands {
            continue;
        }
        let Some(trimmed) = line.strip_prefix("  ") else {
            continue;
        };
        let Some(command) = trimmed.split_whitespace().next() else {
            continue;
        };
        if command != "help" {
            commands.push(command.to_string());
        }
    }

    commands
}
