use assert_cmd::Command;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use wiremock::matchers::{body_partial_json, body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const API_OVERRIDE_ENV: &str = "HYPERLIQUID_API_BASE_URL";
pub const MAINNET_API_OVERRIDE_ENV: &str = "HYPERLIQUID_MAINNET_API_BASE_URL";
pub const TESTNET_API_OVERRIDE_ENV: &str = "HYPERLIQUID_TESTNET_API_BASE_URL";
pub const FORMAT_ENV: &str = "HYPERLIQUID_FORMAT";
pub const PRIVATE_KEY_ENV: &str = "HYPERLIQUID_PRIVATE_KEY";
pub const NETWORK_ENV: &str = "HYPERLIQUID_NETWORK";
pub const WATCH_MAX_TICKS_ENV: &str = "HYPERLIQUID_WATCH_MAX_TICKS";
pub const ACCOUNT_KEY_PASSPHRASE_ENV: &str = "HYPERLIQUID_ACCOUNT_KEY_PASSPHRASE";
pub const ACCOUNT_KEYCHAIN_DISABLED_ENV: &str = "HYPERLIQUID_ACCOUNT_KEYCHAIN_DISABLED";
pub const ACCOUNT_KEY_STORE_DIR_ENV: &str = "HYPERLIQUID_ACCOUNT_KEY_STORE_DIR";
#[allow(dead_code)]
pub const TEST_ACCOUNT_PASSPHRASE: &str = "deterministic integration account encryption passphrase";
#[allow(dead_code)]
pub const VALID_PRIVATE_KEY: &str =
    "0x0000000000000000000000000000000000000000000000000000000000000009";

pub struct IsolatedHome {
    _tmp: TempDir,
    pub home: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
}

impl IsolatedHome {
    pub fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        let config = tmp.path().join("config");
        let data = tmp.path().join("data");
        for path in [&home, &config, &data] {
            std::fs::create_dir_all(path).unwrap();
        }
        Self {
            _tmp: tmp,
            home,
            config,
            data,
        }
    }

    #[allow(dead_code)]
    pub fn tmp_path(&self) -> &Path {
        self._tmp.path()
    }

    pub fn command(&self) -> Command {
        let mut command = hyperliquid_command();
        self.apply_env(&mut command);
        command
    }

    pub fn apply_env(&self, command: &mut Command) {
        command
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("XDG_DATA_HOME", &self.data)
            .env(FORMAT_ENV, "pretty")
            .env_remove(PRIVATE_KEY_ENV)
            .env_remove(NETWORK_ENV)
            .env_remove(API_OVERRIDE_ENV)
            .env_remove(MAINNET_API_OVERRIDE_ENV)
            .env_remove(TESTNET_API_OVERRIDE_ENV)
            .env_remove(WATCH_MAX_TICKS_ENV)
            .env_remove(ACCOUNT_KEY_PASSPHRASE_ENV)
            .env_remove(ACCOUNT_KEYCHAIN_DISABLED_ENV)
            .env_remove(ACCOUNT_KEY_STORE_DIR_ENV);
    }

    #[allow(dead_code)]
    pub fn account_command(&self, passphrase: &str) -> Command {
        let mut command = self.command();
        self.apply_account_env(&mut command, Some(passphrase));
        command
    }

    #[allow(dead_code)]
    pub fn account_command_without_passphrase(&self) -> Command {
        let mut command = self.command();
        self.apply_account_env(&mut command, None);
        command
    }

    #[allow(dead_code)]
    pub fn account_command_for_paths_without_passphrase(
        &self,
        home: &Path,
        data: &Path,
    ) -> Command {
        let mut command = hyperliquid_command();
        command
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("XDG_DATA_HOME", data)
            .env(FORMAT_ENV, "pretty")
            .env(ACCOUNT_KEYCHAIN_DISABLED_ENV, "1")
            .env_remove(ACCOUNT_KEY_PASSPHRASE_ENV)
            .env_remove(ACCOUNT_KEY_STORE_DIR_ENV)
            .env_remove(PRIVATE_KEY_ENV)
            .env_remove(NETWORK_ENV);
        command
    }

    #[allow(dead_code)]
    pub fn apply_account_env(&self, command: &mut Command, passphrase: Option<&str>) {
        command
            .env(ACCOUNT_KEYCHAIN_DISABLED_ENV, "1")
            .env_remove(ACCOUNT_KEY_STORE_DIR_ENV);
        match passphrase {
            Some(passphrase) => {
                command.env(ACCOUNT_KEY_PASSPHRASE_ENV, passphrase);
            }
            None => {
                command.env_remove(ACCOUNT_KEY_PASSPHRASE_ENV);
            }
        }
    }

    #[allow(dead_code)]
    pub fn command_with_api_url(&self, url: impl AsRef<str>) -> Command {
        let mut command = self.command();
        command.env(API_OVERRIDE_ENV, url.as_ref());
        command
    }

    #[allow(dead_code)]
    pub fn command_with_server(&self, server: &MockServer) -> Command {
        self.command_with_api_url(server.uri())
    }

    #[allow(dead_code)]
    pub fn command_with_testnet_server(&self, server: &MockServer) -> Command {
        let mut command = self.command();
        command.env(TESTNET_API_OVERRIDE_ENV, server.uri());
        command
    }

    #[allow(dead_code)]
    pub fn command_with_mainnet_and_testnet(
        &self,
        mainnet_server: &MockServer,
        testnet_server: &MockServer,
    ) -> Command {
        let mut command = self.command();
        command
            .env(MAINNET_API_OVERRIDE_ENV, mainnet_server.uri())
            .env(TESTNET_API_OVERRIDE_ENV, testnet_server.uri());
        command
    }

    #[allow(dead_code)]
    pub fn account_command_with_server(&self, passphrase: &str, server: &MockServer) -> Command {
        let mut command = self.account_command(passphrase);
        command.env(API_OVERRIDE_ENV, server.uri());
        command
    }

    #[allow(dead_code)]
    pub fn account_command_with_mainnet_and_testnet(
        &self,
        passphrase: &str,
        mainnet_server: &MockServer,
        testnet_server: &MockServer,
    ) -> Command {
        let mut command = self.account_command(passphrase);
        command
            .env(MAINNET_API_OVERRIDE_ENV, mainnet_server.uri())
            .env(TESTNET_API_OVERRIDE_ENV, testnet_server.uri());
        command
    }

    #[allow(dead_code)]
    pub fn config_file_candidates(&self) -> Vec<PathBuf> {
        vec![
            self.config.join("hyperliquid").join("config.json"),
            self.home
                .join("Library")
                .join("Application Support")
                .join("hyperliquid")
                .join("config.json"),
        ]
    }

    #[allow(dead_code)]
    pub fn config_file_path(&self) -> PathBuf {
        self.config_file_candidates()
            .into_iter()
            .find(|path| path.exists())
            .expect("config.json should exist")
    }

    #[allow(dead_code)]
    pub fn accounts_db_candidates(&self) -> Vec<PathBuf> {
        vec![
            self.data.join("hyperliquid").join("accounts.db"),
            self.home
                .join("Library")
                .join("Application Support")
                .join("hyperliquid")
                .join("accounts.db"),
        ]
    }

    #[allow(dead_code)]
    pub fn accounts_db_path(&self) -> PathBuf {
        self.accounts_db_candidates()
            .into_iter()
            .find(|path| path.exists())
            .expect("accounts.db should exist")
    }

    #[allow(dead_code)]
    pub fn data_dir_path(&self) -> PathBuf {
        self.accounts_db_candidates()[0]
            .parent()
            .expect("accounts.db candidate should have a parent directory")
            .to_path_buf()
    }

    #[allow(dead_code)]
    pub fn legacy_accounts_key_path(&self) -> PathBuf {
        self.data_dir_path().join("accounts.key")
    }

    #[allow(dead_code)]
    pub fn ows_vault_candidates(&self) -> Vec<PathBuf> {
        vec![
            self.home.join(".hyperliquid"),
            self.data.join("hyperliquid").join("ows-vault"),
        ]
    }

    #[allow(dead_code)]
    pub fn ows_vault_path(&self) -> PathBuf {
        self.ows_vault_candidates()
            .into_iter()
            .find(|path| path.exists())
            .expect("OWS vault should exist")
    }

    #[allow(dead_code)]
    pub fn deprecated_config_key_candidates(&self) -> Vec<PathBuf> {
        vec![
            self.config.join("hyperliquid").join("account-data.key"),
            self.home
                .join("Library")
                .join("Application Support")
                .join("hyperliquid")
                .join("account-data.key"),
        ]
    }
}

pub fn hyperliquid_command() -> Command {
    Command::cargo_bin("hyperliquid").unwrap()
}

#[allow(dead_code)]
pub fn expected_address(private_key: &str) -> String {
    private_key
        .parse::<hypersdk::hypercore::PrivateKeySigner>()
        .unwrap()
        .address()
        .to_string()
}

#[allow(dead_code)]
pub fn copy_dir_all(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_all(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

#[allow(dead_code)]
pub fn fixture_all_mids() -> serde_json::Value {
    serde_json::json!({
        "BTC": "50000",
        "ETH": "3000"
    })
}

#[allow(dead_code)]
pub fn fixture_perp_meta_btc_eth() -> serde_json::Value {
    serde_json::json!({
        "universe": [
            {
                "name": "BTC",
                "szDecimals": 5,
                "maxLeverage": 50,
                "onlyIsolated": false,
                "marginMode": null,
                "growthMode": "disabled"
            },
            {
                "name": "ETH",
                "szDecimals": 4,
                "maxLeverage": 25,
                "onlyIsolated": false,
                "marginMode": null,
                "growthMode": "disabled"
            }
        ],
        "collateralToken": 0
    })
}

#[allow(dead_code)]
pub fn fixture_perp_meta_btc_only() -> serde_json::Value {
    serde_json::json!({
        "universe": [
            {
                "name": "BTC",
                "szDecimals": 5,
                "maxLeverage": 50,
                "onlyIsolated": false,
                "marginMode": null,
                "growthMode": "disabled"
            }
        ],
        "collateralToken": 0
    })
}

#[allow(dead_code)]
pub fn fixture_spot_meta_usdc_only() -> serde_json::Value {
    serde_json::json!({
        "universe": [],
        "tokens": [
            {
                "name": "USDC",
                "index": 0,
                "tokenId": "0x00000000000000000000000000000000",
                "szDecimals": 6,
                "weiDecimals": 6,
                "evmContract": null
            }
        ]
    })
}

#[allow(dead_code)]
pub fn fixture_malformed_spot_meta() -> serde_json::Value {
    serde_json::json!({
        "universe": [
            {
                "name": "PURR/USDC",
                "index": 0,
                "tokens": [1, 0]
            },
            {
                "name": "BROKEN/USDC",
                "index": 1,
                "tokens": [9, 0]
            }
        ],
        "tokens": [
            {
                "name": "USDC",
                "index": 0,
                "tokenId": "0x00000000000000000000000000000000",
                "szDecimals": 6,
                "weiDecimals": 6,
                "evmContract": null
            },
            {
                "name": "PURR",
                "index": 1,
                "tokenId": "0x00000000000000000000000000000001",
                "szDecimals": 0,
                "weiDecimals": 0,
                "evmContract": null
            }
        ]
    })
}

#[allow(dead_code)]
pub fn fixture_basic_order(coin: &str, oid: u64) -> serde_json::Value {
    fixture_basic_order_with_cloid(coin, oid, None)
}

#[allow(dead_code)]
pub fn fixture_basic_order_with_cloid(
    coin: &str,
    oid: u64,
    cloid: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "timestamp": 1700000000000_u64,
        "coin": coin,
        "side": "B",
        "limitPx": "50000",
        "sz": "0.1",
        "oid": oid,
        "origSz": "0.1",
        "cloid": cloid,
        "orderType": "Limit",
        "tif": "Gtc",
        "reduceOnly": false
    })
}

#[allow(dead_code)]
pub fn fixture_fill(coin: &str, oid: u64) -> serde_json::Value {
    serde_json::json!({
        "coin": coin,
        "px": "50000",
        "sz": "0.1",
        "side": "B",
        "time": 1700000000000_u64,
        "startPosition": "0",
        "dir": "Open Long",
        "closedPnl": "0",
        "hash": "0xabc",
        "oid": oid,
        "crossed": true,
        "fee": "0.01",
        "tid": 43_u64,
        "cloid": null,
        "feeToken": "USDC",
        "liquidation": null
    })
}

#[allow(dead_code)]
pub fn fixture_clearinghouse_state(withdrawable: &str, leverage: u32) -> serde_json::Value {
    serde_json::json!({
        "marginSummary": {
            "accountValue": "10000",
            "totalNtlPos": "5000",
            "totalRawUsd": "10000",
            "totalMarginUsed": "1000"
        },
        "crossMarginSummary": {
            "accountValue": "10000",
            "totalNtlPos": "5000",
            "totalRawUsd": "10000",
            "totalMarginUsed": "1000"
        },
        "crossMaintenanceMarginUsed": "100",
        "withdrawable": withdrawable,
        "assetPositions": [
            {
                "type": "oneWay",
                "position": {
                    "coin": "BTC",
                    "szi": "0.1",
                    "leverage": {"type": "cross", "value": leverage},
                    "entryPx": "50000",
                    "positionValue": "5100",
                    "unrealizedPnl": "100",
                    "returnOnEquity": "0.1",
                    "liquidationPx": null,
                    "marginUsed": "1000",
                    "maxLeverage": 50,
                    "cumFunding": {"allTime": "0", "sinceOpen": "0", "sinceChange": "0"}
                }
            }
        ],
        "time": 1700000000000_u64
    })
}

#[allow(dead_code)]
pub fn fixture_order_success_response(oid: u64) -> serde_json::Value {
    serde_json::json!({
        "status": "ok",
        "response": {
            "type": "order",
            "data": {
                "statuses": [
                    {
                        "resting": {
                            "oid": oid,
                            "cloid": null
                        }
                    }
                ]
            }
        }
    })
}

#[allow(dead_code)]
pub fn fixture_order_error_response(message: &str) -> serde_json::Value {
    serde_json::json!({
        "status": "ok",
        "response": {
            "type": "order",
            "data": {
                "statuses": [
                    {
                        "error": message
                    }
                ]
            }
        }
    })
}

#[allow(dead_code)]
pub async fn mount_all_mids(server: &MockServer, btc: &str, eth: &str) {
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "allMids"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "BTC": btc,
            "ETH": eth
        })))
        .mount(server)
        .await;
}

#[allow(dead_code)]
pub async fn mount_override_healthcheck(server: &MockServer) {
    mount_all_mids(server, "50000", "3000").await;
}

#[allow(dead_code)]
pub async fn mock_all_mids_server() -> MockServer {
    mock_all_mids_server_with_prices("50000.0", "3000.0").await
}

#[allow(dead_code)]
pub async fn mock_all_mids_server_with_prices(btc: &str, eth: &str) -> MockServer {
    let server = MockServer::start().await;
    mount_all_mids(&server, btc, eth).await;
    server
}

#[allow(dead_code)]
pub async fn mount_market_meta(server: &MockServer, body: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_string_contains(r#""type":"meta""#))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

#[allow(dead_code)]
pub async fn mount_spot_meta(server: &MockServer, body: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_string_contains(r#""type":"spotMeta""#))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

#[allow(dead_code)]
pub async fn mount_perp_dexs(server: &MockServer, body: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_string_contains(r#""type":"perpDexs""#))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

#[allow(dead_code)]
pub async fn mount_common_public_endpoints(server: &MockServer) {
    mount_override_healthcheck(server).await;
    mount_market_meta(server, fixture_perp_meta_btc_eth()).await;
    mount_spot_meta(server, fixture_spot_meta_usdc_only()).await;
    mount_perp_dexs(server, serde_json::json!([])).await;
}

#[allow(dead_code)]
pub async fn mock_market_server() -> MockServer {
    let server = MockServer::start().await;
    mount_override_healthcheck(&server).await;
    mount_market_meta(&server, fixture_perp_meta_btc_eth()).await;
    mount_spot_meta(&server, fixture_spot_meta_usdc_only()).await;
    server
}

#[allow(dead_code)]
pub async fn mount_account_state(
    server: &MockServer,
    open_orders: Vec<serde_json::Value>,
    fills: Vec<serde_json::Value>,
    withdrawable: &str,
    spot_usdc_total: &str,
    leverage: u32,
) {
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "frontendOpenOrders"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(open_orders))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({"type": "userFills"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(fills))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "clearinghouseState"}),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(fixture_clearinghouse_state(withdrawable, leverage)),
        )
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "spotClearinghouseState"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "balances": [
                {
                    "coin": "USDC",
                    "token": 0,
                    "hold": "0",
                    "total": spot_usdc_total,
                    "entryNtl": "0"
                }
            ]
        })))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "userVaultEquities"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "subAccounts"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(server)
        .await;
}

#[allow(dead_code)]
pub async fn mount_successful_exchange_actions(
    server: &MockServer,
    order_response: serde_json::Value,
) {
    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_string_contains(r#""type":"updateLeverage""#))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": { "type": "default" }
        })))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_string_contains(r#""type":"usdClassTransfer""#))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": { "type": "default" }
        })))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/exchange"))
        .and(body_string_contains(r#""type":"order""#))
        .respond_with(ResponseTemplate::new(200).set_body_json(order_response))
        .mount(server)
        .await;
}
