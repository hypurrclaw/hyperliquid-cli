mod support;

use predicates::prelude::*;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use support::{
    API_OVERRIDE_ENV, IsolatedHome, MAINNET_API_OVERRIDE_ENV, PRIVATE_KEY_ENV,
    TEST_ACCOUNT_PASSPHRASE, TESTNET_API_OVERRIDE_ENV, VALID_PRIVATE_KEY, fixture_basic_order,
    fixture_fill, fixture_order_error_response, fixture_order_success_response,
    mount_account_state, mount_common_public_endpoints, mount_successful_exchange_actions,
};

const IMPORT_PRIVATE_KEY: &str =
    "0x0000000000000000000000000000000000000000000000000000000000000006";
const USER_ADDRESS: &str = "0x0000000000000000000000000000000000000001";
const VALIDATOR_ADDRESS: &str = "0x0000000000000000000000000000000000000002";

async fn mount_borrowlend_and_staking_public(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(serde_json::json!({
            "type": "allBorrowLendReserveStates"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            [
                0,
                {
                    "borrowYearlyRate": "0.05",
                    "supplyYearlyRate": "0.0142",
                    "balance": "1000",
                    "utilization": "0.2",
                    "oraclePx": "1.0",
                    "ltv": "0.0",
                    "totalSupplied": "1000",
                    "totalBorrowed": "200"
                }
            ]
        ])))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "delegatorSummary"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "delegated": "123.45",
            "undelegated": "10",
            "totalPendingWithdrawal": "5.5",
            "nPendingWithdrawals": 2
        })))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "delegations"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "validator": VALIDATOR_ADDRESS,
                "amount": "100.0",
                "lockedUntilTimestamp": 1700000000000_u64
            }
        ])))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "delegatorRewards"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "time": 1700000000000_u64,
                "source": "staking",
                "totalAmount": "1.25"
            }
        ])))
        .mount(server)
        .await;

    Mock::given(method("POST"))
        .and(path("/info"))
        .and(body_partial_json(
            serde_json::json!({"type": "validatorSummaries"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "validator": VALIDATOR_ADDRESS,
                "signer": "0x0000000000000000000000000000000000000003",
                "name": "Validator One",
                "description": "test validator",
                "nRecentBlocks": 1,
                "stake": 123456789_u64,
                "isJailed": false,
                "unjailableAfter": null,
                "isActive": true,
                "commission": "0.04",
                "stats": [
                    ["day", {"uptimeFraction": "1.0", "predictedApr": "0.025", "nSamples": 1440}]
                ]
            }
        ])))
        .mount(server)
        .await;
}

async fn mock_cross_area_server(order_response: serde_json::Value) -> MockServer {
    let server = MockServer::start().await;
    mount_common_public_endpoints(&server).await;
    mount_account_state(
        &server,
        vec![fixture_basic_order("BTC", 4242)],
        vec![fixture_fill("BTC", 4242)],
        "1000",
        "1000",
        5,
    )
    .await;
    mount_borrowlend_and_staking_public(&server).await;
    mount_successful_exchange_actions(&server, order_response).await;

    server
}

#[tokio::test]
async fn first_visit_setup_then_query_and_first_order_succeed() {
    let env = IsolatedHome::new();
    let server = mock_cross_area_server(fixture_order_success_response(4242)).await;

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .arg("setup")
        .write_stdin(format!("2\ny\n\n\n{IMPORT_PRIVATE_KEY}\n"))
        .assert()
        .success()
        .stdout(predicate::str::contains("Test query succeeded"))
        .stdout(predicate::str::contains("Setup complete"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .args(["perps", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BTC"))
        .stderr(predicate::str::contains("Completed in"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .args([
            "orders", "create", "--coin", "BTC", "--side", "buy", "--price", "50000", "--size",
            "0.1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("4242"))
        .stdout(predicate::str::contains("resting"));
}

#[tokio::test]
async fn unauthenticated_read_only_commands_work_and_auth_commands_exit_10() {
    let env = IsolatedHome::new();
    let server = mock_cross_area_server(fixture_order_success_response(4242)).await;

    for args in [
        vec!["perps", "list"],
        vec!["spot", "list"],
        vec!["mids"],
        vec!["account", "fills", USER_ADDRESS],
        vec!["account", "orders", USER_ADDRESS],
        vec!["account", "portfolio", USER_ADDRESS],
        vec!["status"],
        vec!["borrowlend", "rates"],
        vec!["staking", "validators"],
    ] {
        env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
            .args(args)
            .assert()
            .success();
    }

    for args in [
        vec![
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "0.1",
            "--testnet",
        ],
        vec!["orders", "cancel", "12345", "--testnet"],
        vec!["orders", "cancel-all", "-y", "--testnet"],
        vec![
            "positions",
            "update-leverage",
            "--coin",
            "BTC",
            "--leverage",
            "5",
            "--testnet",
        ],
        vec!["transfer", "spot-to-perp", "--amount", "1", "--testnet"],
        vec![
            "staking",
            "delegate",
            "--validator",
            VALIDATOR_ADDRESS,
            "--amount",
            "1",
            "--testnet",
        ],
    ] {
        env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
            .args(args)
            .assert()
            .code(10)
            .stderr(predicate::str::contains("hyperliquid setup"));
    }

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["wallet", "reset"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to reset"));
}

#[tokio::test]
async fn stored_account_aliases_resolve_for_public_user_lookup_commands() {
    let env = IsolatedHome::new();
    let server = mock_cross_area_server(fixture_order_success_response(4242)).await;

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "account",
            "add",
            IMPORT_PRIVATE_KEY,
            "--alias",
            "main",
            "--type",
            "main-wallet",
            "--default",
        ])
        .assert()
        .success();

    for args in [
        vec!["account", "portfolio", "main", "--testnet"],
        vec!["borrowlend", "user", "main", "--testnet"],
    ] {
        env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
            .args(args)
            .assert()
            .success()
            .stdout(predicate::str::contains("USDC"))
            .stdout(predicate::str::contains("1000"));
    }

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .args(["account", "fills", "main", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BTC"))
        .stdout(predicate::str::contains("4242"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .args(["account", "orders", "main", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BTC"))
        .stdout(predicate::str::contains("4242"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .args(["account", "subaccounts", "main", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no subaccounts found"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .args(["staking", "summary", "main", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("123.45"))
        .stdout(predicate::str::contains("1.25"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .args(["staking", "rewards", "main", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("staking"))
        .stdout(predicate::str::contains("1.25"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .args(["--account", "main", "orders", "open", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BTC"))
        .stdout(predicate::str::contains("4242"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .args([
            "--account",
            "main",
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "0.1",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("4242"));
}

#[tokio::test]
async fn stored_account_aliases_do_not_resolve_for_transfer_destinations() {
    let env = IsolatedHome::new();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args([
            "account",
            "add",
            IMPORT_PRIVATE_KEY,
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
            "transfer",
            "send",
            "--to",
            "main",
            "--amount",
            "1",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "address must be a 0x-prefixed 40-byte hex string",
        ));
}

#[tokio::test]
async fn order_open_fills_positions_and_public_fills_share_order_state() {
    let env = IsolatedHome::new();
    let server = mock_cross_area_server(fixture_order_success_response(4242)).await;

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "0.1",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("4242"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["orders", "open", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BTC"))
        .stdout(predicate::str::contains("4242"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["positions", "list", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BTC"))
        .stdout(predicate::str::contains("cross 5x"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &server)
        .args(["account", "fills", USER_ADDRESS, "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BTC"))
        .stdout(predicate::str::contains("4242"));
}

#[tokio::test]
async fn transfer_confirmation_and_balance_queries_reflect_changed_balances() {
    let env = IsolatedHome::new();
    let before = mock_cross_area_server(fixture_order_success_response(4242)).await;
    let after = MockServer::start().await;
    mount_common_public_endpoints(&after).await;
    mount_account_state(&after, vec![], vec![], "1100", "900", 5).await;
    mount_borrowlend_and_staking_public(&after).await;

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &before)
        .args(["account", "portfolio", USER_ADDRESS, "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1000"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &before)
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["transfer", "spot-to-perp", "--amount", "100", "--testnet"])
        .write_stdin("y\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("spot-to-perp"))
        .stdout(predicate::str::contains("100"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &after)
        .args(["account", "portfolio", USER_ADDRESS, "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1100"))
        .stdout(predicate::str::contains("900"));
}

#[tokio::test]
async fn leverage_update_changes_order_outcome_from_rejected_to_accepted() {
    let env = IsolatedHome::new();
    let reject_server = mock_cross_area_server(fixture_order_error_response(
        "insufficient margin at 5x leverage",
    ))
    .await;
    let accept_server = mock_cross_area_server(fixture_order_success_response(5252)).await;

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &reject_server)
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "positions",
            "update-leverage",
            "--coin",
            "BTC",
            "--leverage",
            "5",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("update-leverage"))
        .stdout(predicate::str::contains("5"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &reject_server)
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "1",
            "--testnet",
        ])
        .assert()
        .code(13)
        .stderr(predicate::str::contains(
            "insufficient margin at 5x leverage",
        ));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &accept_server)
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "positions",
            "update-leverage",
            "--coin",
            "BTC",
            "--leverage",
            "10",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("10"));

    env.account_command_with_server(TEST_ACCOUNT_PASSPHRASE, &accept_server)
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "orders",
            "create",
            "--coin",
            "BTC",
            "--side",
            "buy",
            "--price",
            "50000",
            "--size",
            "1",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("5252"));
}

#[tokio::test]
async fn testnet_mode_uses_testnet_endpoint_override() {
    let env = IsolatedHome::new();
    let mainnet = MockServer::start().await;
    let testnet = mock_cross_area_server(fixture_order_success_response(4242)).await;

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .env(MAINNET_API_OVERRIDE_ENV, mainnet.uri())
        .env(TESTNET_API_OVERRIDE_ENV, testnet.uri())
        .args(["--format", "json", "--testnet", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("testnet"))
        .stdout(predicate::str::contains(testnet.uri()));
}

#[test]
fn network_and_auth_errors_include_recovery_suggestions() {
    let env = IsolatedHome::new();

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .env(API_OVERRIDE_ENV, "http://127.0.0.1:9")
        .args(["perps", "list"])
        .assert()
        .code(12)
        .stderr(predicate::str::contains("Unable to reach Hyperliquid API"))
        .stderr(predicate::str::contains("Check your network connection"));

    env.account_command(TEST_ACCOUNT_PASSPHRASE)
        .args(["orders", "open"])
        .assert()
        .code(10)
        .stderr(predicate::str::contains("Authentication required"))
        .stderr(predicate::str::contains("hyperliquid setup"));
}
