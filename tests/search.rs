mod support;

use predicates::prelude::*;
use serde_json::Value;
use support::IsolatedHome;

#[test]
fn search_help_lists_type_filter() {
    IsolatedHome::new()
        .command()
        .args(["search", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--type"))
        .stdout(predicate::str::contains("perp"))
        .stdout(predicate::str::contains("Examples:"))
        .stdout(predicate::str::contains("--dry-run"));
}

#[test]
fn schema_search_is_read_only() {
    let output = IsolatedHome::new()
        .command()
        .args(["--format", "json", "schema", "search"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["command"], "hyperliquid search <QUERY>");
    assert_eq!(json["lifecycle"], "read_only");
    assert_eq!(json["auth_required"], false);
    assert_eq!(json["dry_run"], "not_supported");
}
