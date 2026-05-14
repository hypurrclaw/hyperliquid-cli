// Integration tests for config resolution.
//
// Tests the priority chain:
//   CLI flag > HYPERLIQUID_PRIVATE_KEY env > ~/.config/hyperliquid/config.json
// And network selection:
//   --testnet flag > HYPERLIQUID_NETWORK env > config file
//
// Run with: cargo test --test config_resolution

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

// ── Helper: create a temp config directory with a config.json ───────

const CLI_KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000001";
const ENV_KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000002";
const FILE_KEY: &str = "0x0000000000000000000000000000000000000000000000000000000000000003";

fn create_temp_home() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let home_dir = tmp.path().join("home");
    let xdg_config_home = tmp.path().join("xdg-config");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&xdg_config_home).unwrap();

    // Create both platform-correct locations that `dirs::config_dir` may use.
    // Linux honors XDG_CONFIG_HOME; macOS uses ~/Library/Application Support.
    let config_dir = xdg_config_home.join("hyperliquid");
    let mac_config_dir = home_dir
        .join("Library")
        .join("Application Support")
        .join("hyperliquid");
    fs::create_dir_all(&config_dir).unwrap();
    fs::create_dir_all(&mac_config_dir).unwrap();

    (tmp, home_dir, xdg_config_home)
}

fn write_config_files(
    home_dir: &std::path::Path,
    xdg_config_home: &std::path::Path,
    config: &serde_json::Value,
) {
    let json = serde_json::to_string_pretty(config).unwrap();
    let xdg_path = xdg_config_home.join("hyperliquid").join("config.json");
    let mac_path = home_dir
        .join("Library")
        .join("Application Support")
        .join("hyperliquid")
        .join("config.json");
    fs::write(xdg_path, &json).unwrap();
    fs::write(mac_path, json).unwrap();
}

fn expected_address(private_key: &str) -> String {
    let signer = private_key
        .parse::<hypersdk::hypercore::PrivateKeySigner>()
        .unwrap();
    signer.address().to_string()
}

// ── Test: missing config does not crash read-only commands ───────────

#[test]
fn test_missing_config_no_crash() {
    // With no config file and no env vars, the CLI should still work
    // for read-only commands (e.g., --help, --version)
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .env_remove("HYPERLIQUID_PRIVATE_KEY")
        .env_remove("HYPERLIQUID_NETWORK")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::contains("hyperliquid"));
}

#[test]
fn test_missing_config_help_works() {
    Command::cargo_bin("hyperliquid")
        .unwrap()
        .env_remove("HYPERLIQUID_PRIVATE_KEY")
        .env_remove("HYPERLIQUID_NETWORK")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("perps"));
}

// ── CLI-level tests for config priority chain ────────────────────────

#[test]
fn test_cli_private_key_flag_overrides_env_and_config() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    write_config_files(
        &home_dir,
        &xdg_config_home,
        &serde_json::json!({
            "private_key": FILE_KEY,
            "network": "mainnet"
        }),
    );

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env("HYPERLIQUID_PRIVATE_KEY", ENV_KEY)
        .arg("--private-key")
        .arg(CLI_KEY)
        .arg("wallet")
        .arg("address")
        .assert()
        .success()
        .stdout(predicates::str::contains(expected_address(CLI_KEY)));
}

#[test]
fn test_cli_private_key_env_overrides_config() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    write_config_files(
        &home_dir,
        &xdg_config_home,
        &serde_json::json!({
            "private_key": FILE_KEY,
            "network": "mainnet"
        }),
    );

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env("HYPERLIQUID_PRIVATE_KEY", ENV_KEY)
        .arg("wallet")
        .arg("address")
        .assert()
        .success()
        .stdout(predicates::str::contains(expected_address(ENV_KEY)));
}

#[test]
fn test_cli_private_key_config_fallback() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    write_config_files(
        &home_dir,
        &xdg_config_home,
        &serde_json::json!({
            "private_key": FILE_KEY,
            "network": "mainnet"
        }),
    );

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env_remove("HYPERLIQUID_PRIVATE_KEY")
        .arg("wallet")
        .arg("address")
        .assert()
        .success()
        .stdout(predicates::str::contains(expected_address(FILE_KEY)));
}

#[test]
fn test_cli_testnet_flag_overrides_env_and_config() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    write_config_files(
        &home_dir,
        &xdg_config_home,
        &serde_json::json!({ "network": "mainnet" }),
    );

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env("HYPERLIQUID_NETWORK", "mainnet")
        .arg("--testnet")
        .assert()
        .success()
        .stdout(predicates::str::contains("Network: testnet"))
        .stdout(predicates::str::contains(
            "https://api.hyperliquid-testnet.xyz",
        ));
}

#[test]
fn test_cli_network_env_overrides_config() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    write_config_files(
        &home_dir,
        &xdg_config_home,
        &serde_json::json!({ "network": "mainnet" }),
    );

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env("HYPERLIQUID_NETWORK", "testnet")
        .assert()
        .success()
        .stdout(predicates::str::contains("Network: testnet"));
}

#[test]
fn test_cli_network_env_mainnet_overrides_config_testnet() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    write_config_files(
        &home_dir,
        &xdg_config_home,
        &serde_json::json!({ "network": "testnet" }),
    );

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env("HYPERLIQUID_NETWORK", "mainnet")
        .assert()
        .success()
        .stdout(predicates::str::contains("Network: mainnet"))
        .stdout(predicates::str::contains("https://api.hyperliquid.xyz"));
}

#[test]
fn test_cli_network_config_fallback() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    write_config_files(
        &home_dir,
        &xdg_config_home,
        &serde_json::json!({ "network": "testnet" }),
    );

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env_remove("HYPERLIQUID_NETWORK")
        .assert()
        .success()
        .stdout(predicates::str::contains("Network: testnet"));
}

#[test]
fn test_cli_invalid_network_env_returns_configuration_error() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env("HYPERLIQUID_NETWORK", "bogus")
        .arg("status")
        .assert()
        .failure()
        .code(2)
        .stdout(predicates::str::contains("Configuration error"))
        .stdout(predicates::str::contains("HYPERLIQUID_NETWORK"))
        .stdout(predicates::str::contains("bogus"));
}

#[test]
fn test_cli_malformed_existing_config_returns_configuration_error() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    let bad_json = "{not valid json";
    fs::write(
        xdg_config_home.join("hyperliquid").join("config.json"),
        bad_json,
    )
    .unwrap();
    fs::write(
        home_dir
            .join("Library")
            .join("Application Support")
            .join("hyperliquid")
            .join("config.json"),
        bad_json,
    )
    .unwrap();

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env_remove("HYPERLIQUID_PRIVATE_KEY")
        .env_remove("HYPERLIQUID_NETWORK")
        .assert()
        .failure()
        .code(2)
        .stdout(predicates::str::contains("Configuration error"))
        .stdout(predicates::str::contains("invalid config file"));
}

#[test]
fn test_cli_invalid_config_network_returns_configuration_error() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    write_config_files(
        &home_dir,
        &xdg_config_home,
        &serde_json::json!({ "network": "bogus" }),
    );

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env_remove("HYPERLIQUID_NETWORK")
        .assert()
        .failure()
        .code(2)
        .stdout(predicates::str::contains("Configuration error"))
        .stdout(predicates::str::contains("invalid config file"))
        .stdout(predicates::str::contains("bogus"));
}

#[test]
fn test_cli_testnet_flag_overrides_invalid_network_sources() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    write_config_files(
        &home_dir,
        &xdg_config_home,
        &serde_json::json!({ "network": "bogus" }),
    );

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env("HYPERLIQUID_NETWORK", "bogus")
        .arg("--testnet")
        .assert()
        .success()
        .stdout(predicates::str::contains("Network: testnet"))
        .stdout(predicates::str::contains(
            "https://api.hyperliquid-testnet.xyz",
        ));
}

#[test]
fn test_cli_missing_config_remains_non_fatal() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();

    Command::cargo_bin("hyperliquid")
        .unwrap()
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env_remove("HYPERLIQUID_PRIVATE_KEY")
        .env_remove("HYPERLIQUID_NETWORK")
        .assert()
        .success()
        .stdout(predicates::str::contains("Network: mainnet"));
}

// ── Unit-level tests for config module (in-process) ──────────────────

// These test the config resolution logic directly via the library.

/// Helper to set up a unique test environment that won't interfere
/// with the user's real config. We test the config resolution functions
/// directly by manipulating env vars and temp files.
mod config_unit_tests {
    use hyperliquid_cli::config;
    use std::fs;
    use tempfile::TempDir;

    /// Test that config_dir returns a valid path.
    #[test]
    fn test_config_dir_returns_path() {
        let dir = config::config_dir();
        assert!(dir.is_some());
        let dir = dir.unwrap();
        assert!(dir.to_string_lossy().contains("hyperliquid"));
    }

    /// Test that config_file_path returns a valid path ending in config.json.
    #[test]
    fn test_config_file_path_returns_path() {
        let path = config::config_file_path();
        assert!(path.is_some());
        let path = path.unwrap();
        assert!(path.to_string_lossy().ends_with("config.json"));
    }

    /// Test: CLI flag takes priority over everything.
    #[test]
    fn test_private_key_cli_flag_priority() {
        let cli_key = "0xcli_flag_key_1234567890abcdef1234567890abcdef1234567890abcdef";

        // Even with env var set, CLI flag should win
        let result = config::resolve_private_key_with_env(
            Some(cli_key),
            Some("0xenv_key_1234567890abcdef1234567890abcdef1234567890abcdef1234"),
        );
        assert_eq!(result, Some(cli_key.to_string()));
    }

    /// Test: env var is used when no CLI flag.
    #[test]
    fn test_private_key_env_fallback() {
        let env_key = "0xenv_key_1234567890abcdef1234567890abcdef1234567890abcdef1234";

        let result = config::resolve_private_key_with_env(None, Some(env_key));
        assert_eq!(result, Some(env_key.to_string()));
    }

    /// Test: config file is used when no CLI flag or env var.
    #[test]
    fn test_private_key_config_file_fallback() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("hyperliquid");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.json");

        let file_key = "0xfile_key_1234567890abcdef1234567890abcdef1234567890abcdef12345";
        let config_json = serde_json::json!({
            "private_key": file_key
        });
        fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let result = config::resolve_private_key_from_file(Some(&config_path)).unwrap();
        assert_eq!(result, Some(file_key.to_string()));
    }

    /// Test: returns None when no source has a key.
    #[test]
    fn test_private_key_none_when_all_empty() {
        let result = config::resolve_private_key_with_env(None, None);
        // Without a config file present, should be None
        // (config file resolution would require a real file)
        assert!(result.is_none());
    }

    /// Test: empty CLI flag falls through to env.
    #[test]
    fn test_private_key_empty_cli_falls_through() {
        let env_key = "0xenv_key_1234567890abcdef1234567890abcdef1234567890abcdef1234";
        let result = config::resolve_private_key_with_env(Some(""), Some(env_key));
        assert_eq!(result, Some(env_key.to_string()));
    }

    /// Test: empty env var falls through to None.
    #[test]
    fn test_private_key_empty_env_falls_through() {
        let result = config::resolve_private_key_with_env(None, Some(""));
        assert!(result.is_none());
    }

    // ── Network resolution ──────────────────────────────────────

    /// Test: --testnet flag forces testnet.
    #[test]
    fn test_testnet_cli_flag_true() {
        let result = config::resolve_testnet_with_env(true, Some("mainnet")).unwrap();
        assert!(result);
    }

    /// Test: env var "testnet" selects testnet when no CLI flag.
    #[test]
    fn test_testnet_env_var() {
        let result = config::resolve_testnet_with_env(false, Some("testnet")).unwrap();
        assert!(result);
    }

    /// Test: env var "mainnet" keeps mainnet.
    #[test]
    fn test_mainnet_env_var() {
        let result = config::resolve_testnet_with_env(false, Some("mainnet")).unwrap();
        assert!(!result);
    }

    /// Test: env var is case-insensitive.
    #[test]
    fn test_testnet_env_case_insensitive() {
        let result = config::resolve_testnet_with_env(false, Some("TESTNET")).unwrap();
        assert!(result);

        let result = config::resolve_testnet_with_env(false, Some("TestNet")).unwrap();
        assert!(result);
    }

    /// Test: no flag and no env defaults to mainnet.
    #[test]
    fn test_default_is_mainnet() {
        let result = config::resolve_testnet_with_env(false, None).unwrap();
        assert!(!result);
    }

    /// Test: CLI --testnet flag overrides env var "mainnet".
    #[test]
    fn test_testnet_flag_overrides_env() {
        let result = config::resolve_testnet_with_env(true, Some("mainnet")).unwrap();
        assert!(result);
    }

    #[test]
    fn test_invalid_network_env_returns_error() {
        let err = config::resolve_testnet_with_env(false, Some("bogus")).unwrap_err();
        assert!(err.to_string().contains("Configuration error"));
        assert!(err.to_string().contains("HYPERLIQUID_NETWORK"));
        assert!(err.to_string().contains("bogus"));
    }

    #[test]
    fn test_testnet_flag_overrides_invalid_env() {
        let result = config::resolve_testnet_with_env(true, Some("bogus")).unwrap();
        assert!(result);
    }

    /// Test: config file network is used when no CLI flag or env var.
    #[test]
    fn test_testnet_from_config_file() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("hyperliquid");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.json");

        let config_json = serde_json::json!({
            "network": "testnet"
        });
        fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let result = config::resolve_testnet_from_file(Some(&config_path)).unwrap();
        assert!(result);
    }

    /// Test: config file with "mainnet" stays mainnet.
    #[test]
    fn test_mainnet_from_config_file() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("hyperliquid");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.json");

        let config_json = serde_json::json!({
            "network": "mainnet"
        });
        fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let result = config::resolve_testnet_from_file(Some(&config_path)).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_invalid_network_in_config_file_returns_error() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("hyperliquid");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.json");

        let config_json = serde_json::json!({
            "network": "bogus"
        });
        fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let err = config::resolve_testnet_from_file(Some(&config_path)).unwrap_err();
        assert!(err.to_string().contains("Configuration error"));
        assert!(err.to_string().contains("invalid config file"));
        assert!(err.to_string().contains("bogus"));
    }

    #[test]
    fn test_private_key_resolution_ignores_invalid_lower_priority_network() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("hyperliquid");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.json");

        let file_key = "0xfile_key_1234567890abcdef1234567890abcdef1234567890abcdef12345";
        let config_json = serde_json::json!({
            "private_key": file_key,
            "network": "bogus"
        });
        fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        let result = config::resolve_private_key_from_file(Some(&config_path)).unwrap();
        assert_eq!(result, Some(file_key.to_string()));
    }

    /// Test: missing config file defaults to mainnet.
    #[test]
    fn test_testnet_missing_config_defaults_mainnet() {
        let result = config::resolve_testnet_from_file(None).unwrap();
        assert!(!result);
    }

    // ── API base URL ────────────────────────────────────────────

    #[test]
    fn test_api_base_url_mainnet() {
        assert_eq!(config::api_base_url(false), "https://api.hyperliquid.xyz");
    }

    #[test]
    fn test_api_base_url_testnet() {
        assert_eq!(
            config::api_base_url(true),
            "https://api.hyperliquid-testnet.xyz"
        );
    }

    // ── save_config / load_config roundtrip ─────────────────────

    #[test]
    fn test_save_and_load_config_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");

        let config = config::Config {
            private_key: Some("0xabc123".to_string()),
            network: config::Network::Testnet,
            default_wallet_id: None,
            default_builder_address: None,
            default_builder_fee_rate: None,
            default_referral_code: None,
        };

        config::save_config_to_path(&config, &config_path).unwrap();

        // Verify file exists
        assert!(config_path.exists());

        // Load it back
        let loaded = config::load_config_from_path(&config_path).unwrap();
        assert_eq!(loaded.private_key, Some("0xabc123".to_string()));
        assert_eq!(loaded.network, config::Network::Testnet);
    }

    #[test]
    fn test_save_config_creates_directory() {
        let tmp = TempDir::new().unwrap();
        let nested_dir = tmp.path().join("deep").join("nested").join("config");
        let config_path = nested_dir.join("config.json");

        let config = config::Config {
            private_key: None,
            network: config::Network::Mainnet,
            default_wallet_id: None,
            default_builder_address: None,
            default_builder_fee_rate: None,
            default_referral_code: None,
        };

        config::save_config_to_path(&config, &config_path).unwrap();
        assert!(config_path.exists());
    }

    #[test]
    fn test_load_config_missing_file() {
        let result =
            config::load_config_from_path(std::path::Path::new("/nonexistent/config.json"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_config_invalid_json() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.json");
        fs::write(&config_path, "{invalid json").unwrap();

        let result = config::load_config_from_path(&config_path);
        assert!(result.is_err());
    }

    // ── Full resolution chain (integration) ─────────────────────

    #[test]
    fn test_full_chain_cli_overrides_env_and_file() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("hyperliquid");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.json");

        let config_json = serde_json::json!({
            "private_key": "0xfile_key",
            "network": "mainnet"
        });
        fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        // CLI key should win over env and file
        let resolved = config::resolve_private_key_full(
            Some("0xcli_key"),
            Some("0xenv_key"),
            Some(&config_path),
        )
        .unwrap();
        assert_eq!(resolved, Some("0xcli_key".to_string()));
    }

    #[test]
    fn test_full_chain_env_overrides_file() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("hyperliquid");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.json");

        let config_json = serde_json::json!({
            "private_key": "0xfile_key",
            "network": "mainnet"
        });
        fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        // Env key should win over file
        let resolved =
            config::resolve_private_key_full(None, Some("0xenv_key"), Some(&config_path)).unwrap();
        assert_eq!(resolved, Some("0xenv_key".to_string()));
    }

    #[test]
    fn test_full_chain_file_fallback() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("hyperliquid");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.json");

        let config_json = serde_json::json!({
            "private_key": "0xfile_key",
            "network": "testnet"
        });
        fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        // No CLI, no env → file wins
        let resolved = config::resolve_private_key_full(None, None, Some(&config_path)).unwrap();
        assert_eq!(resolved, Some("0xfile_key".to_string()));
    }

    #[test]
    fn test_full_chain_no_sources() {
        // No CLI, no env, no file → None
        let resolved = config::resolve_private_key_full(None, None, None).unwrap();
        assert!(resolved.is_none());
    }

    #[test]
    fn test_full_network_chain_cli_overrides_env_and_file() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("hyperliquid");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.json");

        let config_json = serde_json::json!({
            "network": "mainnet"
        });
        fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        // CLI --testnet should win over env "mainnet" and file "mainnet"
        let resolved =
            config::resolve_testnet_full(true, Some("mainnet"), Some(&config_path)).unwrap();
        assert!(resolved);
    }

    #[test]
    fn test_full_network_chain_env_overrides_file() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("hyperliquid");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.json");

        let config_json = serde_json::json!({
            "network": "mainnet"
        });
        fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        // Env "testnet" should win over file "mainnet"
        let resolved =
            config::resolve_testnet_full(false, Some("testnet"), Some(&config_path)).unwrap();
        assert!(resolved);
    }

    #[test]
    fn test_full_network_chain_file_fallback() {
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("hyperliquid");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("config.json");

        let config_json = serde_json::json!({
            "network": "testnet"
        });
        fs::write(&config_path, serde_json::to_string(&config_json).unwrap()).unwrap();

        // No CLI, no env → file wins
        let resolved = config::resolve_testnet_full(false, None, Some(&config_path)).unwrap();
        assert!(resolved);
    }

    #[test]
    fn test_full_network_chain_default_mainnet() {
        // No CLI, no env, no file → mainnet
        let resolved = config::resolve_testnet_full(false, None, None).unwrap();
        assert!(!resolved);
    }

    // ── Network enum ────────────────────────────────────────────

    #[test]
    fn test_network_enum_display() {
        assert_eq!(config::Network::Mainnet.to_string(), "mainnet");
        assert_eq!(config::Network::Testnet.to_string(), "testnet");
    }

    #[test]
    fn test_network_enum_serde_roundtrip() {
        let mainnet = config::Network::Mainnet;
        let json = serde_json::to_string(&mainnet).unwrap();
        assert_eq!(json, "\"mainnet\"");
        let parsed: config::Network = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, config::Network::Mainnet);

        let testnet = config::Network::Testnet;
        let json = serde_json::to_string(&testnet).unwrap();
        assert_eq!(json, "\"testnet\"");
        let parsed: config::Network = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, config::Network::Testnet);
    }

    #[test]
    fn test_network_enum_case_insensitive_parse() {
        let parsed: config::Network = serde_json::from_str("\"TESTNET\"").unwrap();
        assert_eq!(parsed, config::Network::Testnet);

        let parsed: config::Network = serde_json::from_str("\"Mainnet\"").unwrap();
        assert_eq!(parsed, config::Network::Mainnet);
    }

    // ── Config struct ───────────────────────────────────────────

    #[test]
    fn test_config_struct_with_all_fields() {
        let config = config::Config {
            private_key: Some("0xkey".to_string()),
            network: config::Network::Testnet,
            default_wallet_id: None,
            default_builder_address: None,
            default_builder_fee_rate: None,
            default_referral_code: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: config::Config = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.private_key, Some("0xkey".to_string()));
        assert_eq!(parsed.network, config::Network::Testnet);
    }

    #[test]
    fn test_config_struct_optional_fields() {
        let config = config::Config {
            private_key: None,
            network: config::Network::Mainnet,
            default_wallet_id: None,
            default_builder_address: None,
            default_builder_fee_rate: None,
            default_referral_code: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        // private_key should be null or absent
        assert!(json.contains("null") || !json.contains("private_key"));
    }

    #[test]
    fn test_config_default_is_mainnet_no_key() {
        let config = config::Config::default();
        assert!(config.private_key.is_none());
        assert_eq!(config.network, config::Network::Mainnet);
    }
}
