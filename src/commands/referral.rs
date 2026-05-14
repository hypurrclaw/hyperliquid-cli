//! Referral status and set-referrer commands.

use std::time::Instant;

use hypersdk::Address;
use hypersdk::hypercore::Chain;
use serde::Serialize;

use crate::commands::actions;
use crate::dry_run::ActionReversibility;
use crate::errors::CliError;
use crate::http_api::post_info_json;
use crate::output::{self, OutputFormat, TableData};
use crate::signing::SelectedSigner;

/// Compile-time default referral code captured from the build environment.
///
/// Set `HYPERLIQUID_DEFAULT_REFERRAL_CODE` while building the binary to bake in
/// a default referral code. A runtime env var with the same name still overrides
/// this value. Empty by default in the upstream repo, meaning no mainnet default.
pub const DEFAULT_REFERRAL_CODE: &str = match option_env!("HYPERLIQUID_DEFAULT_REFERRAL_CODE") {
    Some(code) => code,
    None => "",
};

/// Resolve the default referral code.
///
/// Priority:
/// 1. `HYPERLIQUID_DEFAULT_REFERRAL_CODE` runtime env var (trimmed)
/// 2. Config file `default_referral_code`
/// 3. Build-time `HYPERLIQUID_DEFAULT_REFERRAL_CODE`
/// 4. On testnet: built-in `"TESTNET"` fallback
/// 5. On mainnet: returns `None` (caller should require an explicit code)
pub fn resolve_default_referral_code(is_testnet: bool) -> Result<Option<String>, CliError> {
    let config = crate::config::load_config()
        .map_err(|err| CliError::Configuration(format!("failed to load config: {err}")))?;
    Ok(resolve_default_referral_code_from_config(
        is_testnet,
        config.as_ref(),
    ))
}

pub fn resolve_default_referral_code_from_config(
    is_testnet: bool,
    config: Option<&crate::config::Config>,
) -> Option<String> {
    if let Ok(code) = std::env::var("HYPERLIQUID_DEFAULT_REFERRAL_CODE") {
        let code = code.trim().to_string();
        if !code.is_empty() {
            return Some(code);
        }
    }
    if let Some(code) = config
        .and_then(|config| config.default_referral_code.as_deref())
        .map(str::trim)
        .filter(|code| !code.is_empty())
    {
        return Some(code.to_string());
    }
    if !DEFAULT_REFERRAL_CODE.is_empty() {
        return Some(DEFAULT_REFERRAL_CODE.to_string());
    }
    if is_testnet {
        return Some("TESTNET".to_string());
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReferralStatusOutput {
    pub user: String,
    pub referral_code: Option<String>,
    pub referral_count: usize,
    pub referred_by_code: Option<String>,
    pub referred_by: Option<String>,
    pub unclaimed_rewards: Option<String>,
    pub claimed_rewards: Option<String>,
    pub builder_rewards: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ReferralSetConfirmation {
    action: String,
    status: String,
    network: String,
    signer: String,
    query_address: String,
    code: String,
    reversibility: String,
}

struct ReferralSetOutput {
    row: ReferralSetConfirmation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferralActionKind {
    Set,
    Register,
}

impl ReferralActionKind {
    #[must_use]
    pub fn reversibility(self) -> ActionReversibility {
        match self {
            Self::Set | Self::Register => ActionReversibility::Irreversible,
        }
    }

    #[must_use]
    pub fn action_type(self) -> &'static str {
        match self {
            Self::Set => "setReferrer",
            Self::Register => "registerReferrer",
        }
    }
}

impl TableData for ReferralStatusOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "User",
            "Referral Code",
            "Referral Count",
            "Referred By Code",
            "Referred By",
            "Unclaimed Rewards",
            "Claimed Rewards",
            "Builder Rewards",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.user.clone(),
            display_optional(&self.referral_code),
            self.referral_count.to_string(),
            display_optional(&self.referred_by_code),
            display_optional(&self.referred_by),
            display_optional(&self.unclaimed_rewards),
            display_optional(&self.claimed_rewards),
            display_optional(&self.builder_rewards),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

impl TableData for ReferralSetOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Action",
            "Status",
            "Network",
            "Signer",
            "Query Address",
            "Code",
            "Reversibility",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.row.action.clone(),
            self.row.status.clone(),
            self.row.network.clone(),
            self.row.signer.clone(),
            self.row.query_address.clone(),
            self.row.code.clone(),
            self.row.reversibility.clone(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.row).unwrap_or_else(|_| serde_json::json!({}))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferralInfoRequest<'a> {
    #[serde(rename = "type")]
    request_type: &'static str,
    user: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SetReferrerAction<'a> {
    #[serde(rename = "type")]
    action_type: &'static str,
    code: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RegisterReferrerAction<'a> {
    #[serde(rename = "type")]
    action_type: &'static str,
    code: &'a str,
}

/// Query referral status for the authenticated user.
pub async fn status(
    api_base_url: &str,
    user: Address,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    let user = user.to_string();
    let raw = post_info(
        api_base_url,
        &ReferralInfoRequest {
            request_type: "referral",
            user: &user,
        },
    )
    .await?;
    let output = referral_status_from_value(user, &raw);
    output::print_data(&output, format, start.elapsed());
    Ok(())
}

/// Set the authenticated user's referrer code.
pub async fn set(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    code: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    validate_referral_code(code)?;
    let start = Instant::now();
    actions::send_raw_l1_json_action(
        api_base_url,
        chain,
        signer,
        &SetReferrerAction {
            action_type: "setReferrer",
            code,
        },
        actions::RawL1ActionMetadata::new(actions::nonce_now()),
        "set referrer",
    )
    .await?;

    output::print_data(
        &ReferralSetOutput {
            row: ReferralSetConfirmation {
                action: "set-referrer".to_string(),
                status: "submitted".to_string(),
                network: chain.to_string(),
                signer: signer.address().to_string(),
                query_address: signer.query_address().to_string(),
                code: code.to_string(),
                reversibility: format_reversibility(ReferralActionKind::Set.reversibility())
                    .to_string(),
            },
        },
        format,
        start.elapsed(),
    );
    Ok(())
}

/// Register/create a referral code for the authenticated user.
pub async fn register(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    code: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    validate_referrer_code(code)?;
    let start = Instant::now();
    actions::send_raw_l1_json_action(
        api_base_url,
        chain,
        signer,
        &RegisterReferrerAction {
            action_type: "registerReferrer",
            code,
        },
        actions::RawL1ActionMetadata::new(actions::nonce_now()),
        "register referrer",
    )
    .await?;

    output::print_data(
        &ReferralSetOutput {
            row: ReferralSetConfirmation {
                action: "register-referrer".to_string(),
                status: "submitted".to_string(),
                network: chain.to_string(),
                signer: signer.address().to_string(),
                query_address: signer.query_address().to_string(),
                code: code.to_string(),
                reversibility: format_reversibility(ReferralActionKind::Register.reversibility())
                    .to_string(),
            },
        },
        format,
        start.elapsed(),
    );
    Ok(())
}

pub fn referral_status_from_value(user: String, raw: &serde_json::Value) -> ReferralStatusOutput {
    let referrer_state = raw.get("referrerState");
    let referrer_data = referrer_state.and_then(|state| state.get("data"));
    let referral_code = referrer_data
        .and_then(|data| data.get("code"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let referral_count = referrer_data
        .and_then(|data| data.get("referralStates"))
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);
    let referred_by = raw.get("referredBy");

    ReferralStatusOutput {
        user,
        referral_code,
        referral_count,
        referred_by_code: referred_by
            .and_then(|value| value.get("code"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        referred_by: referred_by
            .and_then(|value| value.get("referrer"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        unclaimed_rewards: string_field(raw, "unclaimedRewards"),
        claimed_rewards: string_field(raw, "claimedRewards"),
        builder_rewards: string_field(raw, "builderRewards"),
    }
}

pub fn validate_referral_code(code: &str) -> Result<(), CliError> {
    validate_code(code, 32, "referral code")
}

pub fn validate_referrer_code(code: &str) -> Result<(), CliError> {
    validate_code(code, 20, "referrer code")
}

pub fn referral_dry_run_value(
    chain: Chain,
    signer: Option<Address>,
    query_address: Option<Address>,
    code: &str,
    action: ReferralActionKind,
) -> serde_json::Value {
    serde_json::json!({
        "network": chain.to_string(),
        "signer": signer.map(|address| address.to_string()),
        "query_address": query_address.map(|address| address.to_string()),
        "code": code,
        "action": {
            "type": action.action_type(),
            "code": code,
        },
        "reversibility": action.reversibility(),
    })
}

fn validate_code(code: &str, max_len: usize, label: &str) -> Result<(), CliError> {
    let trimmed = code.trim();
    if trimmed != code {
        return Err(CliError::Unsupported(format!(
            "{label} must not contain leading or trailing whitespace"
        )));
    }
    if trimmed.is_empty() {
        return Err(CliError::Unsupported(format!("{label} cannot be empty")));
    }
    if trimmed.len() > max_len {
        return Err(CliError::Unsupported(format!(
            "{label} must be {max_len} characters or fewer"
        )));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(CliError::Unsupported(format!(
            "{label} may only contain ASCII letters, numbers, '-' or '_'"
        )));
    }
    Ok(())
}

async fn post_info(
    api_base_url: &str,
    request: &impl Serialize,
) -> Result<serde_json::Value, CliError> {
    post_info_json(api_base_url, request, "").await
}

fn string_field(raw: &serde_json::Value, field: &str) -> Option<String> {
    raw.get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn display_optional(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "None".to_string())
}

fn format_reversibility(reversibility: ActionReversibility) -> &'static str {
    match reversibility {
        ActionReversibility::Reversible => "reversible",
        ActionReversibility::PartiallyReversible => "partially_reversible",
        ActionReversibility::Irreversible => "irreversible",
    }
}

#[cfg(test)]
fn env_guard() -> impl Drop {
    use std::sync::Mutex;
    use std::sync::OnceLock;
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let mutex = LOCK.get_or_init(|| Mutex::new(()));
    let guard = mutex.lock().unwrap();
    struct EnvRestore {
        _guard: std::sync::MutexGuard<'static, ()>,
    }
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            unsafe {
                std::env::remove_var("HYPERLIQUID_DEFAULT_REFERRAL_CODE");
            }
        }
    }
    EnvRestore { _guard: guard }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn referral_status_extracts_code_and_count() {
        let raw = serde_json::json!({
            "referredBy": {
                "referrer": "0x0000000000000000000000000000000000000001",
                "code": "REFERRED"
            },
            "unclaimedRewards": "1.25",
            "claimedRewards": "2.50",
            "builderRewards": "0.10",
            "referrerState": {
                "stage": "ready",
                "data": {
                    "code": "MINE",
                    "referralStates": [
                        { "user": "0x0000000000000000000000000000000000000002" },
                        { "user": "0x0000000000000000000000000000000000000003" }
                    ]
                }
            }
        });

        let status = referral_status_from_value(
            "0x0000000000000000000000000000000000000009".to_string(),
            &raw,
        );

        assert_eq!(status.referral_code.as_deref(), Some("MINE"));
        assert_eq!(status.referral_count, 2);
        assert_eq!(status.referred_by_code.as_deref(), Some("REFERRED"));
        assert_eq!(status.unclaimed_rewards.as_deref(), Some("1.25"));
    }

    #[test]
    fn referral_status_handles_missing_referrer_state() {
        let status = referral_status_from_value(
            "0x0000000000000000000000000000000000000009".to_string(),
            &serde_json::json!({}),
        );

        assert_eq!(status.referral_code, None);
        assert_eq!(status.referral_count, 0);
        assert!(status.rows()[0].contains(&"None".to_string()));
    }

    #[test]
    fn validates_referral_codes() {
        validate_referral_code("TESTNET").unwrap();
        validate_referral_code("ABC-123_ok").unwrap();
        assert_eq!(validate_referral_code("").unwrap_err().exit_code(), 13);
        assert_eq!(validate_referral_code(" ABC ").unwrap_err().exit_code(), 13);
        assert_eq!(
            validate_referral_code("bad code").unwrap_err().exit_code(),
            13
        );
        assert_eq!(
            validate_referral_code("abcdefghijklmnopqrstuvwxyz0123456789")
                .unwrap_err()
                .exit_code(),
            13
        );
    }

    #[test]
    fn referral_actions_declare_reversibility() {
        assert_eq!(
            ReferralActionKind::Set.reversibility(),
            ActionReversibility::Irreversible
        );
        assert_eq!(
            ReferralActionKind::Register.reversibility(),
            ActionReversibility::Irreversible
        );
    }

    #[test]
    fn resolve_default_referral_returns_testnet_fallback() {
        let _guard = env_guard();
        unsafe {
            std::env::remove_var("HYPERLIQUID_DEFAULT_REFERRAL_CODE");
        }
        assert_eq!(
            resolve_default_referral_code_from_config(true, None),
            Some("TESTNET".to_string())
        );
    }

    #[test]
    fn resolve_default_referral_returns_none_on_mainnet_without_defaults() {
        let _guard = env_guard();
        unsafe {
            std::env::remove_var("HYPERLIQUID_DEFAULT_REFERRAL_CODE");
        }
        assert_eq!(resolve_default_referral_code_from_config(false, None), None);
    }

    #[test]
    fn resolve_default_referral_prefers_env_var() {
        let _guard = env_guard();
        unsafe {
            std::env::remove_var("HYPERLIQUID_DEFAULT_REFERRAL_CODE");
            std::env::set_var("HYPERLIQUID_DEFAULT_REFERRAL_CODE", "MYCODE");
        }
        assert_eq!(
            resolve_default_referral_code_from_config(false, None),
            Some("MYCODE".to_string())
        );
        assert_eq!(
            resolve_default_referral_code_from_config(true, None),
            Some("MYCODE".to_string())
        );
    }

    #[test]
    fn resolve_default_referral_trims_env_var() {
        let _guard = env_guard();
        unsafe {
            std::env::remove_var("HYPERLIQUID_DEFAULT_REFERRAL_CODE");
            std::env::set_var("HYPERLIQUID_DEFAULT_REFERRAL_CODE", "  CODE  ");
        }
        assert_eq!(
            resolve_default_referral_code_from_config(false, None),
            Some("CODE".to_string())
        );
    }
}
