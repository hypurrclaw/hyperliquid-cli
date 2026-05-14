mod support;

use alloy::dyn_abi::{Eip712Types, Resolver, TypedData};
use alloy::signers::SignerSync;
use alloy::sol;
use alloy::sol_types::SolStruct;
use alloy_primitives::keccak256;
use hyperliquid_cli::auth::parse_private_key;
use hypersdk::Address;
use hypersdk::hypercore::Chain;
use hypersdk::hypercore::signing::sign_l1_action;
use predicates::prelude::*;
use serde::Serialize;
use serde_json::{Map, Value};
use support::{API_OVERRIDE_ENV, IsolatedHome, PRIVATE_KEY_ENV, VALID_PRIVATE_KEY, mount_all_mids};
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const VALID_ADDRESS: &str = "0x0000000000000000000000000000000000000001";
const VALIDATOR_ADDRESS: &str = "0x0000000000000000000000000000000000000002";
const HYPERLIQUID_EIP_PREFIX: &str = "HyperliquidTransaction:";

sol! {
    struct TokenDelegate {
        string hyperliquidChain;
        address validator;
        uint64 wei;
        bool isUndelegate;
        uint64 nonce;
    }

    struct CDeposit {
        string hyperliquidChain;
        uint64 wei;
        uint64 nonce;
    }

    struct CWithdraw {
        string hyperliquidChain;
        uint64 wei;
        uint64 nonce;
    }

    struct LinkStakingUser {
        string hyperliquidChain;
        address user;
        bool isFinalize;
        uint64 nonce;
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedTokenDelegateMessage {
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    validator: Address,
    wei: u64,
    is_undelegate: bool,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedCDepositMessage {
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    wei: u64,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedCWithdrawMessage {
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    wei: u64,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedLinkStakingUserMessage {
    hyperliquid_chain: Chain,
    user: Address,
    is_finalize: bool,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedClaimRewardsAction {
    #[serde(rename = "type")]
    action_type: &'static str,
}

async fn mount_override_healthcheck(server: &MockServer) {
    mount_all_mids(server, "51000", "3000").await;
}

async fn mock_staking_public_server() -> MockServer {
    let server = MockServer::start().await;
    mount_override_healthcheck(&server).await;

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
        .mount(&server)
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
        .mount(&server)
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
        .mount(&server)
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
        .mount(&server)
        .await;

    server
}

async fn mock_staking_action_server() -> MockServer {
    let server = MockServer::start().await;
    mount_override_healthcheck(&server).await;
    Mock::given(method("POST"))
        .and(path("/exchange"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok",
            "response": { "type": "default" }
        })))
        .mount(&server)
        .await;
    server
}

fn sorted_keys(object: &Map<String, Value>) -> Vec<&str> {
    let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
    keys.sort_unstable();
    keys
}

fn assert_object_keys(value: &Value, expected: &[&str]) {
    let object = value.as_object().expect("expected JSON object");
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    assert_eq!(sorted_keys(object), expected);
}

fn exchange_bodies(requests: &[wiremock::Request]) -> Vec<Value> {
    requests
        .iter()
        .filter(|request| request.url.path() == "/exchange")
        .map(|request| serde_json::from_slice(&request.body).unwrap())
        .collect()
}

fn assert_signed_exchange_envelope(body: &Value) -> u64 {
    assert_object_keys(
        body,
        &[
            "action",
            "expiresAfter",
            "nonce",
            "signature",
            "vaultAddress",
        ],
    );
    assert!(body["vaultAddress"].is_null());
    assert!(body["expiresAfter"].is_null());
    assert_object_keys(&body["signature"], &["r", "s", "v"]);
    let nonce = body["nonce"].as_u64().expect("nonce is a u64");
    assert!(nonce > 0);
    nonce
}

fn typed_data<T: SolStruct>(message: &impl Serialize, chain: Chain) -> TypedData {
    let mut resolver = Resolver::from_struct::<T>();
    resolver
        .ingest_string(T::eip712_encode_type())
        .expect("failed to ingest EIP-712 type");

    let mut types = Eip712Types::from(&resolver);
    let primary_type = types.remove(T::NAME).expect("missing primary EIP-712 type");
    types.insert(format!("{HYPERLIQUID_EIP_PREFIX}{}", T::NAME), primary_type);

    TypedData {
        domain: chain.domain(),
        resolver: Resolver::from(types),
        primary_type: format!("{HYPERLIQUID_EIP_PREFIX}{}", T::NAME),
        message: serde_json::to_value(message).expect("serialize typed-data message"),
    }
}

fn expected_user_action_signature<T: SolStruct>(message: &impl Serialize, chain: Chain) -> Value {
    let signer = parse_private_key(VALID_PRIVATE_KEY).unwrap();
    let signature = signer
        .sign_dynamic_typed_data_sync(&typed_data::<T>(message, chain))
        .unwrap();
    serde_json::to_value(hypersdk::hypercore::types::Signature::from(signature)).unwrap()
}

fn raw_rmp_hash<T: Serialize>(value: &T, nonce: u64) -> alloy_primitives::B256 {
    let mut bytes = rmp_serde::to_vec_named(value).unwrap();
    bytes.extend(nonce.to_be_bytes());
    bytes.push(0);
    keccak256(bytes)
}

async fn expected_raw_l1_signature(action: &impl Serialize, nonce: u64, chain: Chain) -> Value {
    let signer = parse_private_key(VALID_PRIVATE_KEY).unwrap();
    let signature = sign_l1_action(&signer, chain, raw_rmp_hash(action, nonce))
        .await
        .unwrap();
    serde_json::to_value(signature).unwrap()
}

#[tokio::test]
async fn staking_summary_json_includes_balances_delegations_and_rewards() {
    let env = IsolatedHome::new();
    let server = mock_staking_public_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "staking", "summary", VALID_ADDRESS])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json["address"], VALID_ADDRESS);
    assert_eq!(json["delegated"], "123.45");
    assert_eq!(json["undelegated"], "10");
    assert_eq!(json["total_pending_withdrawal"], "5.5");
    assert_eq!(json["pending_rewards"], "1.25");
    assert_eq!(json["delegations"][0]["validator"], VALIDATOR_ADDRESS);
}

#[tokio::test]
async fn staking_validators_lists_validator_names_and_addresses() {
    let env = IsolatedHome::new();
    let server = mock_staking_public_server().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["staking", "validators"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Validator One"))
        .stdout(predicate::str::contains(VALIDATOR_ADDRESS))
        .stderr(predicate::str::contains("Completed in"));
}

#[tokio::test]
async fn staking_rewards_json_shows_reward_amounts() {
    let env = IsolatedHome::new();
    let server = mock_staking_public_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .args(["--format", "json", "staking", "rewards", VALID_ADDRESS])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();

    assert_eq!(json[0]["source"], "staking");
    assert_eq!(json[0]["total_amount"], "1.25");
}

#[tokio::test]
async fn staking_delegate_submits_token_delegate_action() {
    let env = IsolatedHome::new();
    let server = mock_staking_action_server().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "staking",
            "delegate",
            "--validator",
            VALIDATOR_ADDRESS,
            "--amount",
            "1.5",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("delegate"))
        .stdout(predicate::str::contains("submitted"))
        .stdout(predicate::str::contains(VALIDATOR_ADDRESS));

    let requests = server.received_requests().await.unwrap();
    let exchange_request = requests
        .iter()
        .find(|request| request.url.path() == "/exchange")
        .expect("expected /exchange request");
    let body: Value = serde_json::from_slice(&exchange_request.body).unwrap();
    assert_eq!(body["action"]["type"], "tokenDelegate");
    assert_eq!(body["action"]["validator"], VALIDATOR_ADDRESS);
    assert_eq!(body["action"]["isUndelegate"], false);
    assert_eq!(body["action"]["wei"], 150000000_u64);
    assert_eq!(body["action"]["hyperliquidChain"], "Testnet");
}

#[tokio::test]
async fn staking_withdraw_notes_seven_day_queue() {
    let env = IsolatedHome::new();
    let server = mock_staking_action_server().await;

    let output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "staking",
            "withdraw",
            "--amount",
            "2",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["action"], "withdraw");
    assert_eq!(json["status"], "submitted");
    assert_eq!(json["network"], "Testnet");
    assert_eq!(json["amount"], "2");
    assert_eq!(json["asset"], "HYPE");
    assert_eq!(json["reversibility"], "partially_reversible");
    assert!(
        json["note"]
            .as_str()
            .unwrap()
            .contains("7-day withdrawal queue")
    );
    assert!(json["signer"].as_str().unwrap().starts_with("0x"));
    assert!(json["acting_as"].as_str().unwrap().starts_with("0x"));

    let requests = server.received_requests().await.unwrap();
    let exchange_request = requests
        .iter()
        .find(|request| request.url.path() == "/exchange")
        .expect("expected /exchange request");
    let body: Value = serde_json::from_slice(&exchange_request.body).unwrap();
    assert_eq!(body["action"]["type"], "cWithdraw");
    assert_eq!(body["action"]["wei"], 200000000_u64);
}

#[tokio::test]
async fn staking_action_payloads_and_signatures_match_hyperliquid_shapes() {
    let env = IsolatedHome::new();
    let server = mock_staking_action_server().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "staking",
            "delegate",
            "--validator",
            VALIDATOR_ADDRESS,
            "--amount",
            "1.5",
            "--testnet",
        ])
        .assert()
        .success();

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "staking",
            "undelegate",
            "--validator",
            VALIDATOR_ADDRESS,
            "--amount",
            "4",
            "--testnet",
        ])
        .assert()
        .success();

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["staking", "deposit", "--amount", "3", "--testnet"])
        .assert()
        .success();

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["staking", "withdraw", "--amount", "2", "--testnet"])
        .assert()
        .success();

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["staking", "claim-rewards", "--testnet"])
        .assert()
        .success();

    let requests = server.received_requests().await.unwrap();
    let bodies = exchange_bodies(&requests);
    assert_eq!(bodies.len(), 5);

    let delegate = &bodies[0];
    let delegate_nonce = assert_signed_exchange_envelope(delegate);
    assert_object_keys(
        &delegate["action"],
        &[
            "hyperliquidChain",
            "isUndelegate",
            "nonce",
            "signatureChainId",
            "type",
            "validator",
            "wei",
        ],
    );
    assert_eq!(delegate["action"]["type"], "tokenDelegate");
    assert_eq!(delegate["action"]["signatureChainId"], "0x66eee");
    assert_eq!(delegate["action"]["hyperliquidChain"], "Testnet");
    assert_eq!(delegate["action"]["validator"], VALIDATOR_ADDRESS);
    assert_eq!(delegate["action"]["wei"], 150000000_u64);
    assert_eq!(delegate["action"]["isUndelegate"], false);
    assert_eq!(delegate["action"]["nonce"], delegate_nonce);
    assert_eq!(
        delegate["signature"],
        expected_user_action_signature::<TokenDelegate>(
            &ExpectedTokenDelegateMessage {
                signature_chain_id: "0x66eee".to_string(),
                hyperliquid_chain: Chain::Testnet,
                validator: VALIDATOR_ADDRESS.parse().unwrap(),
                wei: 150000000,
                is_undelegate: false,
                nonce: delegate_nonce,
            },
            Chain::Testnet,
        )
    );

    let undelegate = &bodies[1];
    let undelegate_nonce = assert_signed_exchange_envelope(undelegate);
    assert_object_keys(
        &undelegate["action"],
        &[
            "hyperliquidChain",
            "isUndelegate",
            "nonce",
            "signatureChainId",
            "type",
            "validator",
            "wei",
        ],
    );
    assert_eq!(undelegate["action"]["type"], "tokenDelegate");
    assert_eq!(undelegate["action"]["signatureChainId"], "0x66eee");
    assert_eq!(undelegate["action"]["hyperliquidChain"], "Testnet");
    assert_eq!(undelegate["action"]["validator"], VALIDATOR_ADDRESS);
    assert_eq!(undelegate["action"]["wei"], 400000000_u64);
    assert_eq!(undelegate["action"]["isUndelegate"], true);
    assert_eq!(undelegate["action"]["nonce"], undelegate_nonce);
    assert_eq!(
        undelegate["signature"],
        expected_user_action_signature::<TokenDelegate>(
            &ExpectedTokenDelegateMessage {
                signature_chain_id: "0x66eee".to_string(),
                hyperliquid_chain: Chain::Testnet,
                validator: VALIDATOR_ADDRESS.parse().unwrap(),
                wei: 400000000,
                is_undelegate: true,
                nonce: undelegate_nonce,
            },
            Chain::Testnet,
        )
    );

    let deposit = &bodies[2];
    let deposit_nonce = assert_signed_exchange_envelope(deposit);
    assert_object_keys(
        &deposit["action"],
        &[
            "hyperliquidChain",
            "nonce",
            "signatureChainId",
            "type",
            "wei",
        ],
    );
    assert_eq!(deposit["action"]["type"], "cDeposit");
    assert_eq!(deposit["action"]["signatureChainId"], "0x66eee");
    assert_eq!(deposit["action"]["hyperliquidChain"], "Testnet");
    assert_eq!(deposit["action"]["wei"], 300000000_u64);
    assert_eq!(deposit["action"]["nonce"], deposit_nonce);
    assert_eq!(
        deposit["signature"],
        expected_user_action_signature::<CDeposit>(
            &ExpectedCDepositMessage {
                signature_chain_id: "0x66eee".to_string(),
                hyperliquid_chain: Chain::Testnet,
                wei: 300000000,
                nonce: deposit_nonce,
            },
            Chain::Testnet,
        )
    );

    let withdraw = &bodies[3];
    let withdraw_nonce = assert_signed_exchange_envelope(withdraw);
    assert_object_keys(
        &withdraw["action"],
        &[
            "hyperliquidChain",
            "nonce",
            "signatureChainId",
            "type",
            "wei",
        ],
    );
    assert_eq!(withdraw["action"]["type"], "cWithdraw");
    assert_eq!(withdraw["action"]["signatureChainId"], "0x66eee");
    assert_eq!(withdraw["action"]["hyperliquidChain"], "Testnet");
    assert_eq!(withdraw["action"]["wei"], 200000000_u64);
    assert_eq!(withdraw["action"]["nonce"], withdraw_nonce);
    assert_eq!(
        withdraw["signature"],
        expected_user_action_signature::<CWithdraw>(
            &ExpectedCWithdrawMessage {
                signature_chain_id: "0x66eee".to_string(),
                hyperliquid_chain: Chain::Testnet,
                wei: 200000000,
                nonce: withdraw_nonce,
            },
            Chain::Testnet,
        )
    );

    let claim = &bodies[4];
    let claim_nonce = assert_signed_exchange_envelope(claim);
    assert_object_keys(&claim["action"], &["type"]);
    assert_eq!(claim["action"]["type"], "claimRewards");
    assert_eq!(
        claim["signature"],
        expected_raw_l1_signature(
            &ExpectedClaimRewardsAction {
                action_type: "claimRewards"
            },
            claim_nonce,
            Chain::Testnet,
        )
        .await
    );
}

#[tokio::test]
async fn staking_deposit_undelegate_and_claim_rewards_submit_expected_actions() {
    let env = IsolatedHome::new();
    let server = mock_staking_action_server().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["staking", "deposit", "--amount", "3", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("deposit"));

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "staking",
            "undelegate",
            "--validator",
            VALIDATOR_ADDRESS,
            "--amount",
            "4",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("undelegate"));

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args(["staking", "claim-rewards", "--testnet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("claim-rewards"));

    let requests = server.received_requests().await.unwrap();
    let action_types = requests
        .iter()
        .filter(|request| request.url.path() == "/exchange")
        .map(|request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            body["action"]["type"].as_str().unwrap().to_string()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        action_types,
        vec!["cDeposit", "tokenDelegate", "claimRewards"]
    );
}

#[tokio::test]
async fn staking_link_dry_run_previews_verified_shape_and_warning() {
    let env = IsolatedHome::new();

    let output = env
        .command()
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "--dry-run",
            "staking",
            "link",
            "initiate",
            "--user",
            VALID_ADDRESS,
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["command"], "staking link initiate");
    assert_eq!(json["would_execute"], "link_staking_user");
    assert_eq!(json["args"]["phase"], "initiate");
    assert_eq!(json["args"]["user"], VALID_ADDRESS);
    assert_eq!(json["args"]["is_finalize"], false);
    assert_eq!(json["args"]["network"], "Testnet");
    assert_eq!(json["args"]["reversibility"], "irreversible");
    assert_eq!(json["args"]["action"]["type"], "linkStakingUser");
    assert_eq!(json["args"]["action"]["signatureChainId"], "0x66eee");
    assert_eq!(
        json["args"]["verified_shape"]["eip712_fields"],
        serde_json::json!(["hyperliquidChain", "user", "isFinalize", "nonce"])
    );
    assert!(
        json["args"]["warning"]
            .as_str()
            .unwrap()
            .contains("permanent")
    );
    assert!(json["signer"].as_str().unwrap().starts_with("0x"));
    assert!(json["acting_as"].as_str().unwrap().starts_with("0x"));
    assert!(json["vault_address"].is_null());

    let finalize_output = env
        .command()
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "--dry-run",
            "staking",
            "link",
            "finalize",
            "--user",
            VALID_ADDRESS,
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let finalize_json: Value = serde_json::from_slice(&finalize_output).unwrap();
    assert_eq!(finalize_json["command"], "staking link finalize");
    assert_eq!(finalize_json["args"]["phase"], "finalize");
    assert_eq!(finalize_json["args"]["action"]["isFinalize"], true);
}

#[tokio::test]
async fn staking_link_initiate_and_finalize_submit_verified_actions() {
    let env = IsolatedHome::new();
    let server = mock_staking_action_server().await;

    let initiate_output = env
        .command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "--format",
            "json",
            "staking",
            "link",
            "initiate",
            "--user",
            VALID_ADDRESS,
            "--yes",
            "--testnet",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let initiate_json: Value = serde_json::from_slice(&initiate_output).unwrap();
    assert_eq!(initiate_json["status"], "submitted");
    assert_eq!(initiate_json["action"], "staking-link");
    assert_eq!(initiate_json["phase"], "initiate");
    assert_eq!(initiate_json["user"], VALID_ADDRESS);
    assert_eq!(initiate_json["is_finalize"], false);
    assert_eq!(initiate_json["network"], "Testnet");
    assert_eq!(initiate_json["reversibility"], "irreversible");
    assert!(
        initiate_json["warning"]
            .as_str()
            .unwrap()
            .contains("permanent")
    );
    assert!(initiate_json["signer"].as_str().unwrap().starts_with("0x"));
    assert!(
        initiate_json["acting_as"]
            .as_str()
            .unwrap()
            .starts_with("0x")
    );

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "staking",
            "link",
            "finalize",
            "--user",
            VALID_ADDRESS,
            "--yes",
            "--testnet",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("finalize"))
        .stdout(predicate::str::contains("staking-link"));

    let requests = server.received_requests().await.unwrap();
    let bodies = exchange_bodies(&requests);
    assert_eq!(bodies.len(), 2);

    let initiate = &bodies[0];
    let initiate_nonce = assert_signed_exchange_envelope(initiate);
    assert_object_keys(
        &initiate["action"],
        &[
            "hyperliquidChain",
            "isFinalize",
            "nonce",
            "signatureChainId",
            "type",
            "user",
        ],
    );
    assert_eq!(initiate["action"]["type"], "linkStakingUser");
    assert_eq!(initiate["action"]["signatureChainId"], "0x66eee");
    assert_eq!(initiate["action"]["hyperliquidChain"], "Testnet");
    assert_eq!(initiate["action"]["user"], VALID_ADDRESS);
    assert_eq!(initiate["action"]["isFinalize"], false);
    assert_eq!(initiate["action"]["nonce"], initiate_nonce);
    assert_eq!(
        initiate["signature"],
        expected_user_action_signature::<LinkStakingUser>(
            &ExpectedLinkStakingUserMessage {
                hyperliquid_chain: Chain::Testnet,
                user: VALID_ADDRESS.parse().unwrap(),
                is_finalize: false,
                nonce: initiate_nonce,
            },
            Chain::Testnet,
        )
    );

    let finalize = &bodies[1];
    let finalize_nonce = assert_signed_exchange_envelope(finalize);
    assert_eq!(finalize["action"]["type"], "linkStakingUser");
    assert_eq!(finalize["action"]["signatureChainId"], "0x66eee");
    assert_eq!(finalize["action"]["hyperliquidChain"], "Testnet");
    assert_eq!(finalize["action"]["user"], VALID_ADDRESS);
    assert_eq!(finalize["action"]["isFinalize"], true);
    assert_eq!(finalize["action"]["nonce"], finalize_nonce);
    assert_eq!(
        finalize["signature"],
        expected_user_action_signature::<LinkStakingUser>(
            &ExpectedLinkStakingUserMessage {
                hyperliquid_chain: Chain::Testnet,
                user: VALID_ADDRESS.parse().unwrap(),
                is_finalize: true,
                nonce: finalize_nonce,
            },
            Chain::Testnet,
        )
    );
}

#[tokio::test]
async fn staking_link_requires_confirmation_without_yes() {
    let env = IsolatedHome::new();
    let server = mock_staking_action_server().await;

    env.command()
        .env(API_OVERRIDE_ENV, server.uri())
        .env(PRIVATE_KEY_ENV, VALID_PRIVATE_KEY)
        .args([
            "staking",
            "link",
            "initiate",
            "--user",
            VALID_ADDRESS,
            "--testnet",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "staking-link confirmation required",
        ));

    let requests = server.received_requests().await.unwrap();
    assert!(exchange_bodies(&requests).is_empty());
}

#[test]
fn staking_link_validation_fails_before_auth() {
    let env = IsolatedHome::new();
    env.command()
        .args([
            "staking",
            "link",
            "finalize",
            "--user",
            "0x0000000000000000000000000000000000000000",
            "--yes",
            "--testnet",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "staking-link user address must not be the zero address",
        ));
}

#[test]
fn staking_actions_without_wallet_exit_10() {
    for args in [
        vec![
            "staking",
            "delegate",
            "--validator",
            VALIDATOR_ADDRESS,
            "--amount",
            "1",
            "--testnet",
        ],
        vec![
            "staking",
            "undelegate",
            "--validator",
            VALIDATOR_ADDRESS,
            "--amount",
            "1",
            "--testnet",
        ],
        vec!["staking", "deposit", "--amount", "1", "--testnet"],
        vec!["staking", "withdraw", "--amount", "1", "--testnet"],
        vec!["staking", "claim-rewards", "--testnet"],
        vec![
            "staking",
            "link",
            "initiate",
            "--user",
            VALID_ADDRESS,
            "--yes",
            "--testnet",
        ],
        vec![
            "staking",
            "link",
            "finalize",
            "--user",
            VALID_ADDRESS,
            "--yes",
            "--testnet",
        ],
    ] {
        let env = IsolatedHome::new();
        env.command()
            .args(args)
            .assert()
            .code(10)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("Authentication required"));
    }
}
