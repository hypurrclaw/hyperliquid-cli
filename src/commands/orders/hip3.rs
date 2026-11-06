use hypersdk::{Address, Decimal};
use serde_json::json;

use crate::commands::account::fetch_abstraction;
use crate::commands::transfers::{self, SendAssetArgs};
use crate::errors::CliError;
use crate::http_api::ApiClient;
use crate::invocation::Remote;
use crate::mode::Mode;
use crate::signing::SelectedSigner;

const DEFAULT_HIP3_LEVERAGE: i64 = 10;
const MARGIN_BUFFER_BPS: i64 = 12_000;

#[derive(Debug, Clone)]
pub struct Hip3MarginPlan {
    pub dest_dex: String,
    pub amount: Decimal,
}

impl Hip3MarginPlan {
    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "action": "send_asset",
            "source": "perp",
            "dest": format!("dex:{}", self.dest_dex),
            "token": "USDC",
            "amount": self.amount.normalize().to_string(),
        })
    }
}

pub async fn plan_hip3_margin_transfer(
    api: &ApiClient,
    user: Address,
    dex: Option<&str>,
    notional: Decimal,
) -> Result<Option<Hip3MarginPlan>, CliError> {
    let Some(dest_dex) = dex.filter(|dex| !dex.is_empty()) else {
        return Ok(None);
    };
    if notional <= Decimal::ZERO {
        return Ok(None);
    }

    let Ok(abstraction) = fetch_abstraction(api, &user.to_string()).await else {
        return Ok(None);
    };
    if matches!(
        abstraction.normalized_mode.as_str(),
        "unified-account" | "portfolio-margin"
    ) {
        return Ok(None);
    }

    let required = (notional / Decimal::from(DEFAULT_HIP3_LEVERAGE)
        * Decimal::from(MARGIN_BUFFER_BPS)
        / Decimal::from(10_000))
    .round_dp(2);
    if required <= Decimal::ZERO {
        return Ok(None);
    }

    let dest_available = api
        .sdk()
        .clearinghouse_state(user, Some(dest_dex.to_string()))
        .await
        .map(|state| state.withdrawable)
        .unwrap_or(Decimal::ZERO);
    if dest_available >= required {
        return Ok(None);
    }

    Ok(Some(Hip3MarginPlan {
        dest_dex: dest_dex.to_string(),
        amount: required - dest_available,
    }))
}

pub async fn execute_hip3_margin_transfer(
    mode: &Mode,
    remote: &Remote,
    signer: &SelectedSigner,
    user: Address,
    plan: &Hip3MarginPlan,
) -> Result<(), anyhow::Error> {
    transfers::execute_send_asset(
        mode,
        remote,
        signer,
        &SendAssetArgs {
            to: user.to_string(),
            source: "perp".to_string(),
            dest: format!("dex:{}", plan.dest_dex),
            token: "USDC".to_string(),
            amount: plan.amount,
            from_subaccount: None,
            yes: true,
        },
    )
    .await
    .map(drop)
}

pub fn hip3_dex_from_coin(coin: &str, dex: Option<&str>) -> Option<String> {
    if let Some(dex) = dex.filter(|dex| !dex.is_empty()) {
        return Some(dex.to_string());
    }
    coin.split_once(':')
        .map(|(dex, _)| dex.to_string())
        .filter(|dex| !dex.is_empty())
}
