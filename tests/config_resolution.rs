// Integration tests for config resolution.
//
// Tests the priority chain:
//   CLI flag > HYPERLIQUID_PRIVATE_KEY env > ~/.config/hyperliquid/config.json
// And network selection:
//   --testnet flag > HYPERLIQUID_NETWORK env > config file
//
// Run with: cargo test --test config_resolution

use assert_cmd::Command;
#[allow(dead_code)]
mod support;

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
        .stdout(predicates::str::contains(env!("CARGO_PKG_VERSION")));
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
        .stdout(predicates::str::contains(support::expected_address(
            CLI_KEY,
        )));
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
        .stdout(predicates::str::contains(support::expected_address(
            ENV_KEY,
        )));
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
        .stdout(predicates::str::contains(support::expected_address(
            FILE_KEY,
        )));
}

/// Run `status` against a local mock API and return the network the
/// invocation resolved, as reported in its JSON document.
async fn resolved_network(command: &mut Command) -> String {
    let server = support::mock_market_server().await;
    let output = command
        .env("HYPERLIQUID_API_BASE_URL", server.uri())
        .args(["--no-update-check", "--format", "json", "status"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    json["network"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn test_cli_testnet_flag_overrides_env_and_config() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    write_config_files(
        &home_dir,
        &xdg_config_home,
        &serde_json::json!({ "network": "mainnet" }),
    );

    let mut command = Command::cargo_bin("hyperliquid").unwrap();
    command
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env("HYPERLIQUID_NETWORK", "mainnet")
        .arg("--testnet");
    assert_eq!(resolved_network(&mut command).await, "testnet");
}

#[tokio::test]
async fn test_cli_network_env_overrides_config() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    write_config_files(
        &home_dir,
        &xdg_config_home,
        &serde_json::json!({ "network": "mainnet" }),
    );

    let mut command = Command::cargo_bin("hyperliquid").unwrap();
    command
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env("HYPERLIQUID_NETWORK", "testnet");
    assert_eq!(resolved_network(&mut command).await, "testnet");
}

#[tokio::test]
async fn test_cli_network_env_mainnet_overrides_config_testnet() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    write_config_files(
        &home_dir,
        &xdg_config_home,
        &serde_json::json!({ "network": "testnet" }),
    );

    let mut command = Command::cargo_bin("hyperliquid").unwrap();
    command
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env("HYPERLIQUID_NETWORK", "mainnet");
    assert_eq!(resolved_network(&mut command).await, "mainnet");
}

#[tokio::test]
async fn test_cli_network_config_fallback() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    write_config_files(
        &home_dir,
        &xdg_config_home,
        &serde_json::json!({ "network": "testnet" }),
    );

    let mut command = Command::cargo_bin("hyperliquid").unwrap();
    command
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env_remove("HYPERLIQUID_NETWORK");
    assert_eq!(resolved_network(&mut command).await, "testnet");
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
        .arg("status")
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
        .arg("status")
        .assert()
        .failure()
        .code(2)
        .stdout(predicates::str::contains("Configuration error"))
        .stdout(predicates::str::contains("invalid config file"))
        .stdout(predicates::str::contains("bogus"));
}

#[tokio::test]
async fn test_cli_testnet_flag_overrides_invalid_network_sources() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();
    write_config_files(
        &home_dir,
        &xdg_config_home,
        &serde_json::json!({ "network": "bogus" }),
    );

    let mut command = Command::cargo_bin("hyperliquid").unwrap();
    command
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env("HYPERLIQUID_NETWORK", "bogus")
        .arg("--testnet");
    assert_eq!(resolved_network(&mut command).await, "testnet");
}

#[tokio::test]
async fn test_cli_missing_config_remains_non_fatal() {
    let (_tmp, home_dir, xdg_config_home) = create_temp_home();

    let mut command = Command::cargo_bin("hyperliquid").unwrap();
    command
        .env("HOME", &home_dir)
        .env("XDG_CONFIG_HOME", &xdg_config_home)
        .env_remove("HYPERLIQUID_PRIVATE_KEY")
        .env_remove("HYPERLIQUID_NETWORK");
    assert_eq!(resolved_network(&mut command).await, "mainnet");
}

// ── In-process tests over `Settings` ─────────────────────────────────
//
// Resolution order and edge cases are covered by unit tests in `src/settings.rs`;
// these check the same contract through the public API with a real config file.

mod settings_tests {
    use hyperliquid_cli::config::{Config, ConfigFile};
    use hyperliquid_cli::network::Network;
    use hyperliquid_cli::settings::{EnvSource, Settings, env};
    use tempfile::TempDir;

    fn settings_with_file(env_pairs: &[(&str, &str)], config: &Config) -> (TempDir, Settings) {
        let tmp = TempDir::new().unwrap();
        let file = ConfigFile::at(tmp.path().join("nested").join("config.json"));
        file.save(config).unwrap();
        let settings = Settings::new(EnvSource::fixed(env_pairs.iter().copied()), Some(file));
        (tmp, settings)
    }

    fn no_env() -> EnvSource {
        EnvSource::fixed::<&str, &str>([])
    }

    #[test]
    fn config_paths_live_under_a_hyperliquid_directory() {
        let file = ConfigFile::default_location().expect("config dir");
        let path = file.path().to_string_lossy();
        assert!(path.contains("hyperliquid"));
        assert!(path.ends_with("config.json"));
    }

    #[test]
    fn missing_config_defaults_to_mainnet_without_a_key() {
        let settings = Settings::new(no_env(), None);
        assert!(!settings.testnet(false).unwrap());
        assert_eq!(settings.private_key(None).unwrap(), None);
        let missing = Settings::new(
            no_env(),
            Some(ConfigFile::at("/nonexistent/config.json".into())),
        );
        assert_eq!(missing.network(false).unwrap(), Network::Mainnet);
    }

    #[test]
    fn network_chain_flag_then_env_then_file() {
        let testnet_file = Config {
            network: Network::Testnet,
            ..Config::default()
        };
        let (_tmp, from_file) = settings_with_file(&[], &testnet_file);
        assert!(from_file.testnet(false).unwrap());

        let (_tmp, env_wins) = settings_with_file(&[(env::NETWORK, "mainnet")], &testnet_file);
        assert!(!env_wins.testnet(false).unwrap());

        let (_tmp, flag_wins) = settings_with_file(&[(env::NETWORK, "mainnet")], &testnet_file);
        assert!(flag_wins.testnet(true).unwrap());

        let (_tmp, env_case) = settings_with_file(&[(env::NETWORK, "TestNet")], &Config::default());
        assert!(env_case.testnet(false).unwrap());
    }

    #[test]
    fn invalid_network_env_is_rejected_unless_the_flag_decides() {
        let bogus = Settings::new(EnvSource::fixed([(env::NETWORK, "bogus")]), None);
        let err = bogus.testnet(false).unwrap_err();
        assert!(err.to_string().contains(env::NETWORK));
        assert!(bogus.testnet(true).unwrap());
    }

    #[test]
    fn private_key_chain_flag_then_env_then_file() {
        let with_key = Config {
            private_key: Some("0xfile".to_string()),
            ..Config::default()
        };
        let (_tmp, all) = settings_with_file(&[(env::PRIVATE_KEY, "0xenv")], &with_key);
        assert_eq!(all.private_key(Some("0xcli")).unwrap().unwrap(), "0xcli");
        assert_eq!(all.private_key(None).unwrap().unwrap(), "0xenv");

        let (_tmp, file_only) = settings_with_file(&[], &with_key);
        assert_eq!(file_only.private_key(None).unwrap().unwrap(), "0xfile");

        let (_tmp, empty_env) = settings_with_file(&[(env::PRIVATE_KEY, "")], &with_key);
        assert_eq!(empty_env.private_key(Some("")).unwrap().unwrap(), "0xfile");
    }

    #[test]
    fn api_base_urls_default_to_the_public_endpoints() {
        assert_eq!(
            Network::Mainnet.api_base_url(),
            "https://api.hyperliquid.xyz"
        );
        assert_eq!(
            Network::Testnet.api_base_url(),
            "https://api.hyperliquid-testnet.xyz"
        );
    }

    #[test]
    fn saved_config_roundtrips_through_the_file() {
        let config = Config {
            private_key: Some("0xkey".to_string()),
            network: Network::Testnet,
            default_wallet_id: Some("wallet".to_string()),
            default_builder_address: Some("0xbuilder".to_string()),
            default_builder_fee_rate: Some("0.001%".to_string()),
            default_referral_code: Some("CODE".to_string()),
        };
        let (_tmp, settings) = settings_with_file(&[], &config);
        assert_eq!(settings.config().unwrap(), Some(&config));
        assert_eq!(settings.default_wallet_id().unwrap(), Some("wallet"));
    }

    #[test]
    fn broken_config_files_are_configuration_errors() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.json");
        std::fs::write(&path, "{not valid json").unwrap();
        let settings = Settings::new(no_env(), Some(ConfigFile::at(path)));
        let err = settings.config().unwrap_err();
        assert!(err.to_string().contains("invalid config file"));
    }

    #[test]
    fn config_struct_defaults_and_serde() {
        let default = Config::default();
        assert_eq!(default.network, Network::Mainnet);
        assert!(default.private_key.is_none());

        let json = serde_json::to_string(&Config::default()).unwrap();
        assert!(!json.contains("private_key"));
        let parsed: Config = serde_json::from_str(r#"{"network":"TESTNET"}"#).unwrap();
        assert_eq!(parsed.network, Network::Testnet);
    }
}
