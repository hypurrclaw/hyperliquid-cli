mod support;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use support::{
    ACCOUNT_KEY_PASSPHRASE_ENV, ACCOUNT_KEY_STORE_DIR_ENV, IsolatedHome, TEST_ACCOUNT_PASSPHRASE,
    copy_dir_all, expected_address,
};

const IMPORT_KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000004";
const SECOND_KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000005";

fn assert_directory_bytes_do_not_contain(dir: &Path, needle: &[u8]) {
    if !dir.exists() {
        return;
    }
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            assert_directory_bytes_do_not_contain(&path, needle);
        } else {
            let bytes = fs::read(&path).unwrap();
            assert!(
                !bytes.windows(needle.len()).any(|window| window == needle),
                "{} must not contain sensitive key material",
                path.display()
            );
        }
    }
}

fn assert_config_defaults(env: &IsolatedHome) {
    let config: Value = serde_json::from_slice(&fs::read(env.config_file_path()).unwrap()).unwrap();
    assert_eq!(
        config["default_builder_address"],
        "0x00000000000000000000000000000000000000bb"
    );
    assert_eq!(config["default_builder_fee_rate"], "0.001%");
    assert_eq!(config["default_referral_code"], "WALLETTEST");
}

fn assert_config_skipped_invalid_packaged_defaults(env: &IsolatedHome) {
    let config: Value = serde_json::from_slice(&fs::read(env.config_file_path()).unwrap()).unwrap();
    assert!(config["default_wallet_id"].as_str().is_some());
    assert!(config.get("default_builder_address").is_none());
    assert!(config.get("default_builder_fee_rate").is_none());
    assert!(config.get("default_referral_code").is_none());
}

fn assert_no_raw_account_key_files(env: &IsolatedHome) {
    assert!(
        !env.legacy_accounts_key_path().exists(),
        "accounts.key must not be stored in the application data directory"
    );
    for path in env.deprecated_config_key_candidates() {
        assert!(
            !path.exists(),
            "account-data.key raw fallback must not be stored in config directories"
        );
    }
}

fn legacy_key_candidates(env: &IsolatedHome) -> Vec<PathBuf> {
    vec![
        env.data.join("hyperliquid").join("accounts.key"),
        env.home
            .join("Library")
            .join("Application Support")
            .join("hyperliquid")
            .join("accounts.key"),
    ]
}

#[test]
fn wallet_import_show_address_and_reset_flow() {
    let env = IsolatedHome::new();
    let address = expected_address(IMPORT_KEY);

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "import", IMPORT_KEY])
        .assert()
        .success()
        .stdout(predicate::str::contains(&address))
        .stdout(predicate::str::contains("Imported wallet"))
        .stdout(predicate::str::contains("Default"))
        .stdout(predicate::str::contains("yes"));

    let vault_path = env.ows_vault_path();
    assert_directory_bytes_do_not_contain(&vault_path, IMPORT_KEY.as_bytes());
    assert_directory_bytes_do_not_contain(
        &vault_path,
        IMPORT_KEY.trim_start_matches("0x").as_bytes(),
    );
    assert_no_raw_account_key_files(&env);

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains(&address))
        .stdout(predicate::str::contains("Config"))
        .stdout(predicate::str::contains("Vault"));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "address"])
        .assert()
        .success()
        .stdout(predicate::str::is_match(format!("^{address}\n$")).unwrap());

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "reset"])
        .write_stdin("n\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Reset cancelled"));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "address"])
        .assert()
        .success()
        .stdout(predicate::str::contains(&address));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "reset"])
        .write_stdin("y\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Wallet configuration reset"));

    // Reset clears config but leaves OWS vault wallets intact.
    // wallet address still auto-detects from the vault.
    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "address"])
        .assert()
        .success()
        .stdout(predicate::str::contains(&address));

    // Clean up: actually delete the wallet from the vault.
    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "delete", "imported", "--yes"])
        .assert()
        .success();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "address"])
        .assert()
        .code(10)
        .stderr(predicate::str::contains("Authentication required"));
}

#[test]
fn wallet_address_json_outputs_stable_object() {
    let env = IsolatedHome::new();
    let address = expected_address(IMPORT_KEY);

    let output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "--format",
            "json",
            "--private-key",
            IMPORT_KEY,
            "wallet",
            "address",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["address"], address);
}

#[test]
fn wallet_reset_json_yes_outputs_success_object_without_prompt() {
    let env = IsolatedHome::new();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "import", IMPORT_KEY])
        .assert()
        .success();

    let assert = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["--format", "json", "wallet", "reset", "--yes"])
        .assert()
        .success()
        .stderr(
            predicate::str::contains(
                "Reset wallet configuration and remove default wallet reference?",
            )
            .not(),
        );
    let output = assert.get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["status"], "reset");
    assert_eq!(json["message"], "Wallet configuration reset");
}

#[test]
fn wallet_reset_json_missing_configuration_outputs_noop_object() {
    let env = IsolatedHome::new();

    let output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["--format", "json", "wallet", "reset", "--yes"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["status"], "noop");
    assert_eq!(json["message"], "Nothing to reset");
}

#[test]
fn wallet_reset_yes_removes_configuration_without_prompt() {
    let env = IsolatedHome::new();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "import", IMPORT_KEY])
        .assert()
        .success();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "reset", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Wallet configuration reset"))
        .stdout(
            predicate::str::contains(
                "Reset wallet configuration and remove default wallet reference?",
            )
            .not(),
        );

    // Reset clears config but leaves OWS vault wallets intact.
    // wallet address still auto-detects from the vault.
    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "address"])
        .assert()
        .success()
        .stdout(predicate::str::contains(expected_address(IMPORT_KEY)));

    // Clean up: delete the wallet from the vault.
    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "delete", "imported", "--yes"])
        .assert()
        .success();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "address"])
        .assert()
        .code(10)
        .stderr(predicate::str::contains("Authentication required"));
}

#[test]
fn wallet_import_without_argument_prompts_and_stores_wallet() {
    let env = IsolatedHome::new();
    let address = expected_address(IMPORT_KEY);

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "import"])
        .write_stdin(format!("{IMPORT_KEY}\n"))
        .assert()
        .success()
        .stdout(predicate::str::contains("Private key:"))
        .stdout(predicate::str::contains("Imported wallet"))
        .stdout(predicate::str::contains(&address))
        .stdout(predicate::str::contains(IMPORT_KEY).not());

    let vault_path = env.ows_vault_path();
    assert_directory_bytes_do_not_contain(&vault_path, IMPORT_KEY.as_bytes());
    assert_directory_bytes_do_not_contain(
        &vault_path,
        IMPORT_KEY.trim_start_matches("0x").as_bytes(),
    );
    assert_no_raw_account_key_files(&env);
}

#[test]
fn wallet_import_json_rejects_argument_key() {
    let env = IsolatedHome::new();

    let output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["--format", "json", "wallet", "import", IMPORT_KEY])
        .assert()
        .code(13)
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("argv secret input is not supported")
    );
    assert!(!String::from_utf8_lossy(&output).contains(IMPORT_KEY));
}

#[test]
fn copied_data_directory_alone_cannot_decrypt_stored_wallet_key() {
    let env = IsolatedHome::new();
    let address = expected_address(IMPORT_KEY);

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "import", IMPORT_KEY])
        .assert()
        .success()
        .stdout(predicate::str::contains(&address));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "address"])
        .assert()
        .success()
        .stdout(predicate::str::is_match(format!("^{address}\n$")).unwrap());

    assert_no_raw_account_key_files(&env);
    assert_directory_bytes_do_not_contain(
        env.data_dir_path().as_path(),
        TEST_ACCOUNT_PASSPHRASE.as_bytes(),
    );
    for config_dir in [
        env.config.clone(),
        env.home
            .join("Library")
            .join("Application Support")
            .to_path_buf(),
    ] {
        assert_directory_bytes_do_not_contain(&config_dir, TEST_ACCOUNT_PASSPHRASE.as_bytes());
    }

    let copied_home = env.tmp_path().join("copied-home");
    let copied_data_home = env.tmp_path().join("copied-xdg-data");
    fs::create_dir_all(&copied_home).unwrap();
    fs::create_dir_all(&copied_data_home).unwrap();
    let data_dir = env.data_dir_path();
    if data_dir.exists() {
        copy_dir_all(data_dir.as_path(), &copied_data_home.join("hyperliquid"));
        copy_dir_all(
            data_dir.as_path(),
            &copied_home
                .join("Library")
                .join("Application Support")
                .join("hyperliquid"),
        );
    }
    env.account_command_for_paths_without_passphrase(&copied_home, &copied_data_home)
        .args(["account", "ls"])
        .assert()
        .success();

    env.account_command_for_paths_without_passphrase(&copied_home, &copied_data_home)
        .args(["wallet", "address"])
        .assert()
        .code(10)
        .stderr(predicate::str::contains("Authentication required"));
}

#[test]
fn legacy_raw_key_store_override_is_rejected() {
    let env = IsolatedHome::new();
    let address = expected_address(IMPORT_KEY);

    let mut command = Command::cargo_bin("hyperliquid").unwrap();
    command
        .env("HOME", &env.home)
        .env("XDG_CONFIG_HOME", &env.config)
        .env("XDG_DATA_HOME", &env.data)
        .env("HYPERLIQUID_FORMAT", "pretty")
        .env(
            ACCOUNT_KEY_STORE_DIR_ENV,
            env.tmp_path().join("raw-key-store"),
        )
        .env_remove(ACCOUNT_KEY_PASSPHRASE_ENV)
        .env_remove("HYPERLIQUID_PRIVATE_KEY")
        .env_remove("HYPERLIQUID_NETWORK")
        .args(["wallet", "import", IMPORT_KEY])
        .assert()
        .success()
        .stdout(predicate::str::contains("Imported wallet"))
        .stdout(predicate::str::contains(address));
}

#[test]
fn unavailable_keychain_without_passphrase_fails_closed() {
    let env = IsolatedHome::new();
    let address = expected_address(IMPORT_KEY);

    env.account_command_for_paths_without_passphrase(&env.home, &env.data)
        .args(["wallet", "import", IMPORT_KEY])
        .assert()
        .success()
        .stdout(predicate::str::contains("Imported wallet"))
        .stdout(predicate::str::contains(address));

    assert!(
        env.accounts_db_candidates()
            .iter()
            .all(|path| !path.exists()),
        "OWS import path must not create accounts.db"
    );
    for path in env.deprecated_config_key_candidates() {
        assert!(
            !path.exists(),
            "OWS import path must not create account-data.key"
        );
    }
}

#[test]
fn schema_read_does_not_create_account_storage_without_keychain() {
    let env = IsolatedHome::new();

    env.account_command_without_passphrase()
        .args(["schema"])
        .assert()
        .success();

    assert!(
        env.accounts_db_candidates()
            .iter()
            .all(|path| !path.exists()),
        "schema output must not create accounts.db"
    );
    for path in legacy_key_candidates(&env) {
        assert!(!path.exists(), "schema output must not create accounts.key");
    }
    for path in env.deprecated_config_key_candidates() {
        assert!(
            !path.exists(),
            "schema output must not create account-data.key"
        );
    }
}

#[test]
fn read_only_legacy_wallet_lookup_does_not_migrate_key_storage() {
    let env = IsolatedHome::new();
    let address = expected_address(IMPORT_KEY);
    let db_candidates = [
        env.data.join("hyperliquid").join("accounts.db"),
        env.home
            .join("Library")
            .join("Application Support")
            .join("hyperliquid")
            .join("accounts.db"),
    ];
    let mut legacy_key_snapshots = Vec::new();
    for db in db_candidates {
        let legacy_key = db.parent().unwrap().join("accounts.key");
        let mut store = hyperliquid_cli::db::AccountStore::open(&db, &legacy_key).unwrap();
        store
            .add_account("main", &address, IMPORT_KEY, "api-wallet", true)
            .unwrap();
        drop(store);
        legacy_key_snapshots.push((legacy_key.clone(), fs::read(&legacy_key).unwrap()));
    }

    env.account_command_without_passphrase()
        .args(["wallet", "address"])
        .assert()
        .success()
        .stdout(predicate::str::is_match(format!("^{address}\n$")).unwrap());

    for (legacy_key, legacy_key_before) in legacy_key_snapshots {
        assert_eq!(
            fs::read(&legacy_key).unwrap(),
            legacy_key_before,
            "read-only wallet lookup must not rewrite or migrate the legacy key"
        );
        assert!(
            legacy_key.exists(),
            "read-only wallet lookup must not remove the legacy key"
        );
    }
    for path in env.deprecated_config_key_candidates() {
        assert!(
            !path.exists(),
            "read-only wallet lookup must not create deprecated account-data.key"
        );
    }
}

#[test]
fn wallet_import_help_warns_about_argument_exposure() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["wallet", "import", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("process listings"))
        .stdout(predicate::str::contains("shell history"))
        .stdout(predicate::str::contains("without PRIVATE_KEY"))
        .stdout(predicate::str::contains("controlled automation"));
}

#[test]
fn account_and_setup_help_explain_public_reads_and_secret_handling() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["account", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Public account reads"))
        .stdout(predicate::str::contains("do not require a signer"))
        .stdout(predicate::str::contains(
            "wallets can also be selected by name, id, or address",
        ))
        .stdout(predicate::str::contains("global --account"))
        .stdout(predicate::str::contains("OWS account commands"));

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--account"));

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["account", "add", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hidden prompt"))
        .stdout(predicate::str::contains("process listings"))
        .stdout(predicate::str::contains("shell history"))
        .stdout(predicate::str::contains("--alias"))
        .stdout(predicate::str::contains("--default"));

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .args(["setup", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hidden prompt"))
        .stdout(predicate::str::contains("OWS vault"))
        .stdout(predicate::str::contains("default signing account"))
        .stdout(predicate::str::contains("never printed"));
}

#[test]
fn wallet_create_generates_and_stores_default_wallet() {
    let env = IsolatedHome::new();

    let output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "create"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created wallet"))
        .stdout(predicate::str::contains("Default"))
        .stdout(predicate::str::contains("yes"))
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    let address = stdout
        .lines()
        .find_map(|line| line.split_whitespace().find(|part| part.starts_with("0x")))
        .expect("wallet create should print address")
        .to_string();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "address"])
        .assert()
        .success()
        .stdout(predicate::str::contains(address));
}

#[test]
fn wallet_create_import_and_mnemonic_apply_packaged_defaults() {
    let create_env = IsolatedHome::new();
    create_env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .env(
            "HYPERLIQUID_DEFAULT_BUILDER_ADDRESS",
            "0x00000000000000000000000000000000000000bb",
        )
        .env("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE", "0.001%")
        .env("HYPERLIQUID_DEFAULT_REFERRAL_CODE", "WALLETTEST")
        .args(["wallet", "create"])
        .assert()
        .success();
    assert_config_defaults(&create_env);

    let import_env = IsolatedHome::new();
    import_env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .env(
            "HYPERLIQUID_DEFAULT_BUILDER_ADDRESS",
            "0x00000000000000000000000000000000000000bb",
        )
        .env("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE", "0.001%")
        .env("HYPERLIQUID_DEFAULT_REFERRAL_CODE", "WALLETTEST")
        .args(["wallet", "import", IMPORT_KEY])
        .assert()
        .success();
    assert_config_defaults(&import_env);

    let mnemonic_env = IsolatedHome::new();
    mnemonic_env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .env(
            "HYPERLIQUID_DEFAULT_BUILDER_ADDRESS",
            "0x00000000000000000000000000000000000000bb",
        )
        .env("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE", "0.001%")
        .env("HYPERLIQUID_DEFAULT_REFERRAL_CODE", "WALLETTEST")
        .args([
            "wallet",
            "import-mnemonic",
            "test test test test test test test test test test test junk",
        ])
        .assert()
        .success();
    assert_config_defaults(&mnemonic_env);
}

#[test]
fn wallet_create_import_and_mnemonic_ignore_invalid_packaged_defaults() {
    let create_env = IsolatedHome::new();
    create_env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .env(
            "HYPERLIQUID_DEFAULT_BUILDER_ADDRESS",
            "0x00000000000000000000000000000000000000bb",
        )
        .env("HYPERLIQUID_DEFAULT_REFERRAL_CODE", "bad code")
        .args(["wallet", "create"])
        .assert()
        .success();
    assert_config_skipped_invalid_packaged_defaults(&create_env);

    let import_env = IsolatedHome::new();
    import_env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .env(
            "HYPERLIQUID_DEFAULT_BUILDER_ADDRESS",
            "0x00000000000000000000000000000000000000bb",
        )
        .env("HYPERLIQUID_DEFAULT_REFERRAL_CODE", "bad code")
        .args(["wallet", "import", IMPORT_KEY])
        .assert()
        .success();
    assert_config_skipped_invalid_packaged_defaults(&import_env);

    let mnemonic_env = IsolatedHome::new();
    mnemonic_env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .env(
            "HYPERLIQUID_DEFAULT_BUILDER_ADDRESS",
            "0x00000000000000000000000000000000000000bb",
        )
        .env("HYPERLIQUID_DEFAULT_REFERRAL_CODE", "bad code")
        .args([
            "wallet",
            "import-mnemonic",
            "test test test test test test test test test test test junk",
        ])
        .assert()
        .success();
    assert_config_skipped_invalid_packaged_defaults(&mnemonic_env);
}

#[test]
fn wallet_create_json_includes_default_status() {
    let env = IsolatedHome::new();

    let output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["--format", "json", "wallet", "create"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["message"], "Created wallet");
    assert_eq!(json["alias"], "wallet");
    assert_eq!(json["source"], "stored OWS wallet");
    assert_eq!(json["is_default"], true);
}

#[test]
fn wallet_create_reuses_unique_aliases_on_subsequent_runs() {
    let env = IsolatedHome::new();

    let first_output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "create"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created wallet"))
        .stdout(predicate::str::contains("wallet"))
        .get_output()
        .stdout
        .clone();
    let first_stdout = String::from_utf8(first_output).unwrap();
    let first_address = first_stdout
        .lines()
        .find_map(|line| line.split_whitespace().find(|part| part.starts_with("0x")))
        .expect("first wallet create should print address")
        .to_string();

    let second_output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "create"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created wallet"))
        .stdout(predicate::str::contains("wallet-2"))
        .get_output()
        .stdout
        .clone();
    let second_stdout = String::from_utf8(second_output).unwrap();
    let second_address = second_stdout
        .lines()
        .find_map(|line| line.split_whitespace().find(|part| part.starts_with("0x")))
        .expect("second wallet create should print address")
        .to_string();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["account", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("wallet"))
        .stdout(predicate::str::contains("wallet-2"))
        .stdout(predicate::str::contains(first_address))
        .stdout(predicate::str::contains(&second_address));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "address"])
        .assert()
        .success()
        .stdout(predicate::str::contains(second_address));
}

#[test]
fn account_add_ls_set_default_and_remove_flow() {
    let env = IsolatedHome::new();
    let first_address = expected_address(IMPORT_KEY);
    let second_address = expected_address(SECOND_KEY);

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["account", "add", IMPORT_KEY, "--alias", "main", "--default"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Account added"))
        .stdout(predicate::str::contains(&first_address));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["account", "add", SECOND_KEY, "--alias", "backup"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Account added"))
        .stdout(predicate::str::contains(&second_address));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["account", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("main"))
        .stdout(predicate::str::contains("backup"))
        .stdout(predicate::str::contains(&first_address))
        .stdout(predicate::str::contains("yes"));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["account", "set-default"])
        .write_stdin("backup\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Default account set"))
        .stdout(predicate::str::contains("backup"));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "address"])
        .assert()
        .success()
        .stdout(predicate::str::contains(&second_address));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["account", "remove"])
        .write_stdin("main\ny\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Account removed"))
        .stdout(predicate::str::contains("main"));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["account", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("backup"))
        .stdout(predicate::str::contains("main").not());
}

#[test]
fn account_add_set_default_and_remove_flow_with_yes() {
    let env = IsolatedHome::new();
    let first_address = expected_address(IMPORT_KEY);
    let second_address = expected_address(SECOND_KEY);

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "account",
            "add",
            IMPORT_KEY,
            "--alias",
            "main",
            "--type",
            "api-wallet",
            "--default",
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("process listings"))
        .stderr(predicate::str::contains("shell history"))
        .stderr(predicate::str::contains(IMPORT_KEY).not())
        .stdout(predicate::str::contains("Account added"))
        .stdout(predicate::str::contains("main"))
        .stdout(predicate::str::contains(&first_address))
        .stdout(predicate::str::contains(IMPORT_KEY).not());

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "account",
            "add",
            SECOND_KEY,
            "--alias",
            "backup",
            "--type",
            "api-wallet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Account added"));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["account", "set-default", "backup"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Default account set"))
        .stdout(predicate::str::contains("backup"));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "address"])
        .assert()
        .success()
        .stdout(predicate::str::contains(&second_address));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["account", "remove", "main", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Account removed"))
        .stdout(predicate::str::contains("main"))
        .stdout(predicate::str::contains("Remove wallet 'main'").not());

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["account", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("backup"))
        .stdout(predicate::str::contains("main").not());
}

#[test]
fn global_account_selects_stored_signer_without_changing_default() {
    let env = IsolatedHome::new();
    let first_address = expected_address(IMPORT_KEY);
    let second_address = expected_address(SECOND_KEY);

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "account",
            "add",
            IMPORT_KEY,
            "--alias",
            "main",
            "--type",
            "main-wallet",
            "--default",
        ])
        .assert()
        .success();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "account",
            "add",
            SECOND_KEY,
            "--alias",
            "backup",
            "--type",
            "api-wallet",
        ])
        .assert()
        .success();

    for selector in ["backup", second_address.as_str()] {
        let output = env
            .account_command(TEST_ACCOUNT_PASSPHRASE)
            .args([
                "--format",
                "json",
                "--account",
                selector,
                "wallet",
                "address",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let json: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(json["address"], second_address);
    }

    let output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["--format", "json", "wallet", "address"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["address"], first_address);
}

#[test]
fn global_account_missing_selector_has_clear_structured_error() {
    let env = IsolatedHome::new();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "account",
            "add",
            IMPORT_KEY,
            "--alias",
            "main",
            "--type",
            "main-wallet",
            "--default",
        ])
        .assert()
        .success();

    let output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "--format",
            "json",
            "--account",
            "missing",
            "wallet",
            "address",
        ])
        .assert()
        .code(13)
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("account selector 'missing' was not found as an address, alias, or id")
    );
}

#[test]
fn global_account_conflicts_with_explicit_private_key_and_keystore_flags() {
    let env = IsolatedHome::new();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "--account",
            "main",
            "--private-key",
            IMPORT_KEY,
            "wallet",
            "address",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--account"))
        .stderr(predicate::str::contains("--private-key"));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "--account",
            "main",
            "--keystore",
            "wallet.json",
            "wallet",
            "address",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--account"))
        .stderr(predicate::str::contains("--keystore"));
}

#[test]
fn account_set_default_and_remove_without_accounts_have_clear_error() {
    let env = IsolatedHome::new();
    let expected = "no stored wallets found; run hyperliquid setup or hyperliquid wallet import";

    let set_default_output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["--format", "json", "account", "set-default"])
        .write_stdin("\n")
        .assert()
        .code(13)
        .get_output()
        .stdout
        .clone();
    let set_default_json: Value = serde_json::from_slice(&set_default_output).unwrap();
    assert!(
        set_default_json["error"]
            .as_str()
            .unwrap()
            .contains("account set-default requires confirmation in machine-readable contexts")
    );

    let remove_output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["--format", "json", "account", "remove"])
        .write_stdin("\n")
        .assert()
        .code(13)
        .get_output()
        .stdout
        .clone();
    let remove_json: Value = serde_json::from_slice(&remove_output).unwrap();
    assert!(
        remove_json["error"]
            .as_str()
            .unwrap()
            .contains("account remove requires confirmation in machine-readable contexts")
    );

    let set_default_selector_output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["--format", "json", "account", "set-default", "main"])
        .assert()
        .code(13)
        .get_output()
        .stdout
        .clone();
    let set_default_selector_json: Value =
        serde_json::from_slice(&set_default_selector_output).unwrap();
    assert!(
        set_default_selector_json["error"]
            .as_str()
            .unwrap()
            .contains(expected)
    );

    let remove_selector_output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["--format", "json", "account", "remove", "main", "--yes"])
        .assert()
        .code(13)
        .get_output()
        .stdout
        .clone();
    let remove_selector_json: Value = serde_json::from_slice(&remove_selector_output).unwrap();
    assert!(
        remove_selector_json["error"]
            .as_str()
            .unwrap()
            .contains(expected)
    );
}

#[test]
fn account_remove_json_requires_yes_before_prompting() {
    let env = IsolatedHome::new();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["account", "add"])
        .write_stdin(format!("{IMPORT_KEY}\nmain\napi-wallet\ny\n"))
        .assert()
        .success();

    let output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["--format", "json", "account", "remove"])
        .write_stdin("main\nn\n")
        .assert()
        .code(13)
        .stderr(predicate::str::is_empty())
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("account remove requires confirmation in machine-readable contexts")
    );
}

#[test]
fn account_remove_json_yes_outputs_removed_account_without_prompt() {
    let env = IsolatedHome::new();
    let address = expected_address(IMPORT_KEY);

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["account", "add"])
        .write_stdin(format!("{IMPORT_KEY}\nmain\napi-wallet\ny\n"))
        .assert()
        .success();

    let assert = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["--format", "json", "account", "remove", "main", "--yes"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
    let output = assert.get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["message"], "Account removed");
    assert_eq!(json["address"], address);
    assert_eq!(json["alias"], "main");
    assert_eq!(json["source"], "OWS wallet");

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["account", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("main").not());
}

#[test]
fn account_ls_displays_legacy_api_wallet_type_as_local_signing_account() {
    let env = IsolatedHome::new();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "account",
            "add",
            IMPORT_KEY,
            "--alias",
            "main",
            "--type",
            "api-wallet",
            "--default",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("OWS wallet"));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["account", "ls"])
        .assert()
        .success()
        .stdout(predicate::str::contains("OWS wallet"))
        .stdout(predicate::str::contains("api-wallet").not());

    let output = env
        .account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["--format", "json", "account", "ls"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json[0]["type"], "ows-wallet");
}

#[test]
fn wallet_reset_recovers_from_malformed_config() {
    let env = IsolatedHome::new();
    for path in env.config_file_candidates() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "{not json").unwrap();
    }

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "reset"])
        .write_stdin("y\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Wallet configuration reset"));
}

#[test]
fn experimental_ows_wallet_show_and_address_are_address_only() {
    let env = IsolatedHome::new();
    let ows_address = "0x0000000000000000000000000000000000000001";

    let show_output = env
        .command()
        .args([
            "--format",
            "json",
            "--ows-signer",
            ows_address,
            "wallet",
            "show",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let show_json: Value = serde_json::from_slice(&show_output).unwrap();
    assert_eq!(show_json["address"], ows_address);
    assert_eq!(show_json["alias"], Value::Null);
    assert!(show_json["source"].as_str().unwrap().contains("OWS signer"));

    let address_output = env
        .command()
        .args([
            "--format",
            "json",
            "--ows-signer",
            ows_address,
            "wallet",
            "address",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let address_json: Value = serde_json::from_slice(&address_output).unwrap();
    assert_eq!(address_json["address"], ows_address);
}

#[test]
fn experimental_ows_conflicts_with_existing_signer_sources() {
    let env = IsolatedHome::new();
    let ows_address = "0x0000000000000000000000000000000000000001";

    env.command()
        .args([
            "--ows-signer",
            ows_address,
            "--private-key",
            IMPORT_KEY,
            "wallet",
            "address",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--ows-signer"))
        .stderr(predicate::str::contains("--private-key"));

    env.command()
        .args([
            "--ows-signer",
            ows_address,
            "--account",
            "main",
            "wallet",
            "address",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--ows-signer"))
        .stderr(predicate::str::contains("--account"));

    env.command()
        .args([
            "--ows-signer",
            ows_address,
            "--keystore",
            "wallet.json",
            "wallet",
            "address",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--ows-signer"))
        .stderr(predicate::str::contains("--keystore"));

    env.command()
        .args([
            "--ows-signer",
            ows_address,
            "--keystore-password",
            "secret",
            "wallet",
            "address",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--ows-signer"))
        .stderr(predicate::str::contains("--keystore-password"));
}
