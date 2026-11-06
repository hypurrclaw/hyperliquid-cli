mod contract_support;

use assert_cmd::Command;

use contract_support::{command_path, schema_all};

#[test]
fn every_registered_command_resolves_to_cli_help() {
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
