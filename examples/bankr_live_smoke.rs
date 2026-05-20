use alloy::dyn_abi::TypedData;
use hyperliquid_cli::bankr;
use hyperliquid_cli::signing::SelectedSigner;
use hypersdk::hypercore::Chain;
use hypersdk::hypercore::api::UpdateLeverage;
use hypersdk::hypercore::types::Action;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = bankr::resolve_selector("default")?;
    println!("wallet_address={}", config.address());

    let signer = SelectedSigner::bankr(config.clone());
    let err = signer
        .sign_l1_action_sync(
            Action::UpdateLeverage(UpdateLeverage {
                asset: 0,
                is_cross: true,
                leverage: 2,
            }),
            1_777_963_000_000,
            None,
            Chain::Testnet,
        )
        .unwrap_err();
    assert_eq!(err.exit_code(), 13);
    println!("raw_l1_fail_closed_ok=true");

    let typed_data: TypedData = serde_json::from_value(serde_json::json!({
        "types": {
            "EIP712Domain": [
                {"name": "name", "type": "string"},
                {"name": "chainId", "type": "uint256"}
            ],
            "BankrCliSmoke": [
                {"name": "message", "type": "string"},
                {"name": "nonce", "type": "uint256"}
            ]
        },
        "primaryType": "BankrCliSmoke",
        "domain": {
            "name": "Hyperliquid CLI Bankr Smoke",
            "chainId": "0x3e7"
        },
        "message": {
            "message": "bankr signing smoke",
            "nonce": "0x1"
        }
    }))?;

    let _typed_signature = bankr::sign_typed_data(&config, &typed_data)?;
    println!("typed_data_signature_ok=true");

    let _message_signature = bankr::sign_message(&config, b"hyperliquid-cli bankr live smoke")?;
    println!("personal_sign_signature_ok=true");

    Ok(())
}
