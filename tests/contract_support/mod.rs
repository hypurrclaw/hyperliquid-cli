#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

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

pub fn selected_schemas(commands: &[&str]) -> Value {
    let schemas = schema_all();
    let selected = commands
        .iter()
        .map(|command| {
            schemas
                .iter()
                .find(|schema| command_path(schema) == *command)
                .unwrap_or_else(|| panic!("missing schema for {command}"))
                .clone()
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "characterization": true,
        "review_required_to_update": true,
        "commands": selected,
    })
}
