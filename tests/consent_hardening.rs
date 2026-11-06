mod support;

use predicates::prelude::*;
use serde_json::Value;
use std::fs;

use support::{IsolatedHome, TEST_ACCOUNT_PASSPHRASE, expected_address};

const IMPORT_KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000004";

fn import_wallet(env: &IsolatedHome) {
    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "import", IMPORT_KEY])
        .assert()
        .success();
}

#[test]
fn setup_with_empty_stdin_fails_without_writing_anything() {
    let env = IsolatedHome::new();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .arg("setup")
        .write_stdin("")
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Stdin ended before an answer"));

    assert!(
        env.config_file_candidates()
            .iter()
            .all(|path| !path.exists()),
        "setup must not write config when stdin ends"
    );
    assert!(
        env.ows_vault_candidates().iter().all(|path| !path.exists()),
        "setup must not create a wallet when stdin ends"
    );
}

#[test]
fn account_remove_yes_without_selector_is_missing_input_in_json_mode() {
    let env = IsolatedHome::new();
    import_wallet(&env);

    let assert = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["--format", "json", "account", "remove", "--yes"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Select wallet").not());
    let json: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();

    assert_eq!(json["category"], "missing_input");
    assert!(json["error"].as_str().unwrap().contains("<WALLET>"));
    assert!(!json["error"].as_str().unwrap().contains("Select wallet"));
}

#[cfg(unix)]
#[test]
fn config_is_written_owner_only_and_atomically() {
    use std::os::unix::fs::PermissionsExt;

    let env = IsolatedHome::new();
    import_wallet(&env);

    let config_path = env.config_file_path();
    let mode = fs::metadata(&config_path).unwrap().permissions().mode() & 0o777;
    let dir_mode = fs::metadata(config_path.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    assert_eq!(dir_mode, 0o700);

    let leftovers: Vec<String> = fs::read_dir(config_path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temp files left behind: {leftovers:?}"
    );
}

#[test]
fn wallet_receipt_reports_config_keys_written() {
    let env = IsolatedHome::new();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "import", IMPORT_KEY])
        .assert()
        .success()
        .stdout(predicate::str::contains("Config keys written"))
        .stdout(predicate::str::contains("default_wallet_id"));
}

#[test]
fn stale_default_wallet_fails_naming_it_instead_of_picking_another() {
    let env = IsolatedHome::new();
    import_wallet(&env);
    let config_path = env.config_file_path();
    let mut config: Value =
        serde_json::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config["default_wallet_id"] = Value::from("ghost-wallet");
    fs::write(&config_path, serde_json::to_string(&config).unwrap()).unwrap();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "address"])
        .assert()
        .code(10)
        .stderr(predicate::str::contains(
            "Default wallet ghost-wallet isn't in the vault.",
        ))
        .stderr(predicate::str::contains(
            "    hyperliquid account set-default <WALLET>",
        ));
}

#[test]
fn broken_config_is_an_error_for_wallet_list() {
    let env = IsolatedHome::new();
    import_wallet(&env);
    fs::write(env.config_file_path(), "{not json").unwrap();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "list"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid config file"));
}

#[test]
fn read_only_wallet_commands_do_not_create_the_vault() {
    let env = IsolatedHome::new();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "list"])
        .assert()
        .success();
    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "show"])
        .assert()
        .code(10);

    assert!(
        env.ows_vault_candidates().iter().all(|path| !path.exists()),
        "read-only commands must not create the OWS vault"
    );
}

#[test]
fn wallet_delete_previews_on_stderr_and_declines_by_default() {
    let env = IsolatedHome::new();
    import_wallet(&env);

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "delete", "imported"])
        .write_stdin("n\n")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(expected_address(IMPORT_KEY)))
        .stderr(predicate::str::contains("Default: yes"))
        .stderr(predicate::str::contains(
            "This deletes the encrypted key from the vault. This cannot be undone.",
        ))
        .stderr(predicate::str::contains("Back up first:"))
        .stderr(predicate::str::contains(
            "    hyperliquid wallet export imported",
        ))
        .stderr(predicate::str::contains("Delete wallet imported?"));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("imported"));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "delete", "imported"])
        .write_stdin("y\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted wallet"));
}

#[test]
fn wallet_create_warns_when_vault_passphrase_is_unset() {
    let env = IsolatedHome::new();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "create"])
        .assert()
        .success()
        .stderr(predicate::str::contains("OWS_PASSPHRASE isn't set"))
        .stderr(predicate::str::contains("empty passphrase"));

    let env = IsolatedHome::new();
    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .env("OWS_PASSPHRASE", "correct horse")
        .args(["wallet", "create"])
        .assert()
        .success()
        .stderr(predicate::str::contains("OWS_PASSPHRASE isn't set").not());
}

#[test]
fn plaintext_config_key_warns_once_on_stderr() {
    let env = IsolatedHome::new();
    for path in env.config_file_candidates() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!(r#"{{"private_key":"{IMPORT_KEY}"}}"#)).unwrap();
    }

    let assert = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "address"])
        .assert()
        .success()
        .stderr(predicate::str::contains("plaintext private key"))
        .stderr(predicate::str::contains(IMPORT_KEY).not());
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert_eq!(stderr.matches("stores a plaintext private key").count(), 1);
}

#[test]
fn keystore_password_can_come_from_the_environment() {
    let env = IsolatedHome::new();

    env.command()
        .args(["--keystore", "missing.json", "wallet", "address"])
        .assert()
        .code(10)
        .stderr(predicate::str::contains("HYPERLIQUID_KEYSTORE_PASSWORD"));

    // With the variable set, the password requirement is satisfied and the
    // failure moves on to reading the (missing) keystore.
    env.command()
        .env("HYPERLIQUID_KEYSTORE_PASSWORD", "pw")
        .args(["--keystore", "missing.json", "wallet", "address"])
        .assert()
        .code(10)
        .stderr(predicate::str::contains("failed to decrypt keystore"));
}
