//! Staking public queries and authenticated staking actions.

use std::time::{Duration, Instant};

use alloy::sol;
use clap::Args;
use hypersdk::hypercore::Chain;
use hypersdk::{Address, Decimal};
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use crate::commands::actions;
use crate::dry_run::ActionReversibility;
use crate::errors::CliError;
use crate::http_api::post_info_json;
use crate::output::{self, JsonValueOutput, OutputFormat, TableData};
use crate::signing::SelectedSigner;

const HYPE_WEI_SCALE: u64 = 100_000_000;

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

const STAKING_LINK_WARNING: &str = "Staking-link changes fee discount attribution between trading and staking accounts. Finalization may be permanent; verify both account addresses and control before signing.";

/// Arguments for `staking delegate` and `staking undelegate`.
#[derive(Args, Debug, Clone)]
pub struct DelegateArgs {
    /// Validator address to delegate to or undelegate from
    #[arg(long)]
    pub validator: String,

    /// Amount of HYPE to delegate or undelegate
    #[arg(long, allow_hyphen_values = true)]
    pub amount: Decimal,
}

/// Arguments for `staking deposit` and `staking withdraw`.
#[derive(Args, Debug, Clone)]
pub struct AmountArgs {
    /// Amount of HYPE to move
    #[arg(long, allow_hyphen_values = true)]
    pub amount: Decimal,
}

/// Arguments for `staking link initiate` and `staking link finalize`.
#[derive(Args, Debug, Clone)]
pub struct LinkArgs {
    /// Protocol user address to link. For initiate, the staking user; for finalize, the trading user.
    #[arg(long)]
    pub user: String,

    /// Confirm live execution without prompting
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StakingActionKind {
    Delegate,
    Undelegate,
    Deposit,
    Withdraw,
    ClaimRewards,
    LinkInitiate,
    LinkFinalize,
}

impl StakingActionKind {
    #[must_use]
    pub fn reversibility(self) -> ActionReversibility {
        match self {
            Self::Delegate | Self::Undelegate | Self::Deposit | Self::Withdraw => {
                ActionReversibility::PartiallyReversible
            }
            Self::ClaimRewards | Self::LinkInitiate | Self::LinkFinalize => {
                ActionReversibility::Irreversible
            }
        }
    }

    #[must_use]
    pub fn would_execute(self) -> &'static str {
        match self {
            Self::Delegate => "delegate_hype",
            Self::Undelegate => "undelegate_hype",
            Self::Deposit => "deposit_hype_to_staking",
            Self::Withdraw => "queue_hype_staking_withdrawal",
            Self::ClaimRewards => "claim_staking_rewards",
            Self::LinkInitiate | Self::LinkFinalize => "link_staking_user",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DelegatorSummary {
    #[serde(with = "rust_decimal::serde::str")]
    pub delegated: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub undelegated: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub total_pending_withdrawal: Decimal,
    pub n_pending_withdrawals: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DelegationRow {
    pub validator: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    pub locked_until_timestamp: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RewardRow {
    pub time: u64,
    pub source: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub total_amount: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ValidatorSummary {
    pub validator: String,
    pub signer: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub n_recent_blocks: Option<u64>,
    pub stake: u64,
    pub is_jailed: bool,
    pub unjailable_after: Option<u64>,
    pub is_active: bool,
    #[serde(with = "rust_decimal::serde::str")]
    pub commission: Decimal,
    #[serde(default)]
    pub stats: Vec<(String, ValidatorStats)>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidatorStats {
    #[serde(with = "rust_decimal::serde::str")]
    pub uptime_fraction: Decimal,
    #[serde(with = "rust_decimal::serde::str")]
    pub predicted_apr: Decimal,
    pub n_samples: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StakingSummaryOutput {
    address: String,
    summary: DelegatorSummary,
    pending_rewards: Decimal,
    delegations: Vec<DelegationRow>,
    rewards: Vec<RewardRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewardsOutput {
    rows: Vec<RewardRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatorsOutput {
    rows: Vec<ValidatorSummary>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StakingQueryResult<T> {
    pub output: T,
    pub elapsed: Duration,
}

struct ActionConfirmationContext<'a> {
    chain: Chain,
    signer: &'a SelectedSigner,
    action_kind: StakingActionKind,
}

#[derive(Debug, Clone, Serialize)]
struct ActionConfirmation {
    action: String,
    status: String,
    network: String,
    signer: String,
    acting_as: String,
    reversibility: String,
    #[serde(with = "rust_decimal::serde::str")]
    amount: Decimal,
    asset: String,
    validator: Option<String>,
    note: Option<String>,
}

struct ActionOutput {
    row: ActionConfirmation,
}

#[derive(Debug, Clone, Serialize)]
struct LinkStakingConfirmation {
    status: String,
    action: String,
    phase: String,
    acting_as: String,
    signer: String,
    user: String,
    is_finalize: bool,
    network: String,
    reversibility: String,
    warning: String,
}

struct LinkStakingOutput {
    row: LinkStakingConfirmation,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InfoRequest<'a> {
    #[serde(rename = "type")]
    request_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<&'a str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenDelegateMessage {
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    validator: Address,
    wei: u64,
    is_undelegate: bool,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TokenDelegateAction {
    #[serde(rename = "type")]
    action_type: &'static str,
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    validator: Address,
    wei: u64,
    is_undelegate: bool,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CDepositMessage {
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    wei: u64,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CDepositAction {
    #[serde(rename = "type")]
    action_type: &'static str,
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    wei: u64,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CWithdrawMessage {
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    wei: u64,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CWithdrawAction {
    #[serde(rename = "type")]
    action_type: &'static str,
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    wei: u64,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LinkStakingUserMessage {
    hyperliquid_chain: Chain,
    user: Address,
    is_finalize: bool,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LinkStakingUserAction {
    #[serde(rename = "type")]
    action_type: &'static str,
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    user: Address,
    is_finalize: bool,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaimRewardsAction {
    #[serde(rename = "type")]
    action_type: &'static str,
}

impl TableData for StakingSummaryOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Address",
            "Delegated",
            "Undelegated",
            "Pending Withdrawal",
            "Pending Withdrawals",
            "Pending Rewards",
            "Delegations",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.address.clone(),
            self.summary.delegated.to_string(),
            self.summary.undelegated.to_string(),
            self.summary.total_pending_withdrawal.to_string(),
            self.summary.n_pending_withdrawals.to_string(),
            self.pending_rewards.to_string(),
            self.delegations.len().to_string(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "address": self.address,
            "delegated": self.summary.delegated,
            "undelegated": self.summary.undelegated,
            "total_pending_withdrawal": self.summary.total_pending_withdrawal,
            "n_pending_withdrawals": self.summary.n_pending_withdrawals,
            "pending_rewards": self.pending_rewards,
            "delegations": self.delegations.iter().map(delegation_json).collect::<Vec<_>>(),
            "rewards": self.rewards.iter().map(reward_json).collect::<Vec<_>>(),
        })
    }
}

impl TableData for RewardsOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Time", "Source", "Total Amount"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.time.to_string(),
                    row.source.clone(),
                    row.total_amount.to_string(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::Value::Array(self.rows.iter().map(reward_json).collect())
    }
}

impl TableData for ValidatorsOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Name",
            "Validator",
            "Stake",
            "Commission",
            "APR",
            "Active",
            "Jailed",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.name.clone(),
                    row.validator.clone(),
                    format_hype_wei(row.stake),
                    row.commission.to_string(),
                    row.stats
                        .iter()
                        .find(|(period, _)| period == "day")
                        .or_else(|| row.stats.first())
                        .map(|(_, stats)| stats.predicted_apr.to_string())
                        .unwrap_or_default(),
                    row.is_active.to_string(),
                    row.is_jailed.to_string(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::Value::Array(
            self.rows
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "validator": row.validator,
                        "signer": row.signer,
                        "name": row.name,
                        "description": row.description,
                        "n_recent_blocks": row.n_recent_blocks,
                        "stake": row.stake,
                        "stake_hype": format_hype_wei(row.stake),
                        "is_jailed": row.is_jailed,
                        "unjailable_after": row.unjailable_after,
                        "is_active": row.is_active,
                        "commission": row.commission,
                        "stats": row.stats.iter().map(|(period, stats)| serde_json::json!({
                            "period": period,
                            "uptime_fraction": stats.uptime_fraction,
                            "predicted_apr": stats.predicted_apr,
                            "n_samples": stats.n_samples,
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect(),
        )
    }
}

impl TableData for ActionOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Action",
            "Status",
            "Network",
            "Signer",
            "Acting As",
            "Reversibility",
            "Amount",
            "Asset",
            "Validator",
            "Note",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.row.action.clone(),
            self.row.status.clone(),
            self.row.network.clone(),
            self.row.signer.clone(),
            self.row.acting_as.clone(),
            self.row.reversibility.clone(),
            self.row.amount.to_string(),
            self.row.asset.clone(),
            self.row.validator.clone().unwrap_or_default(),
            self.row.note.clone().unwrap_or_default(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.row).unwrap_or_else(|_| serde_json::json!({}))
    }
}

impl TableData for LinkStakingOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Status",
            "Action",
            "Phase",
            "Acting As",
            "Signer",
            "User",
            "Is Finalize",
            "Network",
            "Reversibility",
            "Warning",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.row.status.clone(),
            self.row.action.clone(),
            self.row.phase.clone(),
            self.row.acting_as.clone(),
            self.row.signer.clone(),
            self.row.user.clone(),
            self.row.is_finalize.to_string(),
            self.row.network.clone(),
            self.row.reversibility.clone(),
            self.row.warning.clone(),
        ]]
    }

    fn pretty_rows(&self) -> Vec<Vec<String>> {
        self.rows()
            .into_iter()
            .map(|mut row| {
                if row.len() > 7 {
                    row[7] = output::colors::yellow(&row[7]);
                }
                row
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.row).unwrap_or_else(|_| serde_json::json!({}))
    }
}

/// Show staking balance, delegations, and reward summary for an address.
pub async fn summary(
    api_base_url: &str,
    address: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = summary_query(api_base_url, address).await?;
    output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Show staking balance, delegations, and reward summary without printing it.
pub async fn summary_query(
    api_base_url: &str,
    address: &str,
) -> Result<StakingQueryResult<StakingSummaryOutput>, anyhow::Error> {
    let start = Instant::now();
    validate_address(address)?;
    let summary = delegator_summary(api_base_url, address).await?;
    let delegations = delegations(api_base_url, address).await?;
    let rewards = rewards_rows(api_base_url, address).await?;
    let pending_rewards = rewards.iter().map(|row| row.total_amount).sum();
    Ok(StakingQueryResult {
        output: StakingSummaryOutput {
            address: address.to_string(),
            summary,
            pending_rewards,
            delegations,
            rewards,
        },
        elapsed: start.elapsed(),
    })
}

/// List validator summaries.
pub async fn validators(api_base_url: &str, format: OutputFormat) -> Result<(), anyhow::Error> {
    let result = validators_query(api_base_url).await?;
    output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// List validator summaries without printing them.
pub async fn validators_query(
    api_base_url: &str,
) -> Result<StakingQueryResult<ValidatorsOutput>, anyhow::Error> {
    let start = Instant::now();
    let rows = post_info::<Vec<ValidatorSummary>>(
        api_base_url,
        &InfoRequest {
            request_type: "validatorSummaries",
            user: None,
        },
    )
    .await?;
    Ok(StakingQueryResult {
        output: ValidatorsOutput { rows },
        elapsed: start.elapsed(),
    })
}

/// Show staking reward rows for an address.
pub async fn rewards(
    api_base_url: &str,
    address: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = rewards_query(api_base_url, address).await?;
    output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Show staking reward rows without printing them.
pub async fn rewards_query(
    api_base_url: &str,
    address: &str,
) -> Result<StakingQueryResult<RewardsOutput>, anyhow::Error> {
    let start = Instant::now();
    validate_address(address)?;
    let rows = rewards_rows(api_base_url, address).await?;
    Ok(StakingQueryResult {
        output: RewardsOutput { rows },
        elapsed: start.elapsed(),
    })
}

/// Show staking delegation and withdrawal history for an address.
pub async fn history(
    api_base_url: &str,
    address: &str,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = history_query(api_base_url, address).await?;
    output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

/// Show staking delegation and withdrawal history without printing it.
pub async fn history_query(
    api_base_url: &str,
    address: &str,
) -> Result<StakingQueryResult<JsonValueOutput>, anyhow::Error> {
    let start = Instant::now();
    validate_address(address)?;
    let value = post_info_json::<serde_json::Value>(
        api_base_url,
        &InfoRequest {
            request_type: "delegatorHistory",
            user: Some(address),
        },
        "loading staking history",
    )
    .await?;
    Ok(StakingQueryResult {
        output: JsonValueOutput::new(value, "no staking history found"),
        elapsed: start.elapsed(),
    })
}

/// Delegate HYPE to a validator.
pub async fn delegate(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    args: &DelegateArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    token_delegate(api_base_url, chain, signer, args, false, format).await
}

/// Undelegate HYPE from a validator.
pub async fn undelegate(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    args: &DelegateArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    token_delegate(api_base_url, chain, signer, args, true, format).await
}

/// Move HYPE from spot to staking balance.
pub async fn deposit(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    args: &AmountArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    let wei = validate_amount_to_wei(args.amount)?;
    let nonce = actions::nonce_now();
    let signature_chain_id = chain.arbitrum_id().to_string();
    let message = CDepositMessage {
        signature_chain_id: signature_chain_id.clone(),
        hyperliquid_chain: chain,
        wei,
        nonce,
    };
    let signature = actions::sign_user_action::<CDeposit>(signer, chain, &message)?;
    let action = CDepositAction {
        action_type: "cDeposit",
        signature_chain_id,
        hyperliquid_chain: chain,
        wei,
        nonce,
    };
    actions::send_user_signed_json_action(
        api_base_url,
        serde_json::to_value(action).map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?,
        nonce,
        signature,
    )
    .await?;
    print_action_confirmation(
        ActionConfirmationContext {
            chain,
            signer,
            action_kind: StakingActionKind::Deposit,
        },
        "deposit",
        args.amount,
        None,
        None,
        format,
        start,
    );
    Ok(())
}

/// Queue HYPE withdrawal from staking to spot balance.
pub async fn withdraw(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    args: &AmountArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    let wei = validate_amount_to_wei(args.amount)?;
    let nonce = actions::nonce_now();
    let signature_chain_id = chain.arbitrum_id().to_string();
    let message = CWithdrawMessage {
        signature_chain_id: signature_chain_id.clone(),
        hyperliquid_chain: chain,
        wei,
        nonce,
    };
    let signature = actions::sign_user_action::<CWithdraw>(signer, chain, &message)?;
    let action = CWithdrawAction {
        action_type: "cWithdraw",
        signature_chain_id,
        hyperliquid_chain: chain,
        wei,
        nonce,
    };
    actions::send_user_signed_json_action(
        api_base_url,
        serde_json::to_value(action).map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?,
        nonce,
        signature,
    )
    .await?;
    print_action_confirmation(
        ActionConfirmationContext {
            chain,
            signer,
            action_kind: StakingActionKind::Withdraw,
        },
        "withdraw",
        args.amount,
        None,
        Some("7-day withdrawal queue before funds become available in spot".to_string()),
        format,
        start,
    );
    Ok(())
}

/// Claim available staking rewards.
pub async fn claim_rewards(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    actions::send_raw_l1_json_action(
        api_base_url,
        chain,
        signer,
        &ClaimRewardsAction {
            action_type: "claimRewards",
        },
        actions::RawL1ActionMetadata::new(actions::nonce_now()),
        "claim staking rewards",
    )
    .await?;
    print_action_confirmation(
        ActionConfirmationContext {
            chain,
            signer,
            action_kind: StakingActionKind::ClaimRewards,
        },
        "claim-rewards",
        Decimal::ZERO,
        None,
        None,
        format,
        start,
    );
    Ok(())
}

/// Build a stable dry-run preview for staking-link actions.
pub fn link_dry_run_value(
    chain: Chain,
    args: &LinkArgs,
    is_finalize: bool,
) -> Result<serde_json::Value, CliError> {
    let user = validate_link_user_address(&args.user)?;
    let action_kind = if is_finalize {
        StakingActionKind::LinkFinalize
    } else {
        StakingActionKind::LinkInitiate
    };
    Ok(serde_json::json!({
        "phase": link_phase(is_finalize),
        "user": user.to_string(),
        "is_finalize": is_finalize,
        "network": chain.to_string(),
        "reversibility": action_kind.reversibility(),
        "warning": STAKING_LINK_WARNING,
        "verified_shape": {
            "source": "@nktkas/hyperliquid v0.31.0 linkStakingUser",
            "signature_type": "HyperliquidTransaction:LinkStakingUser",
            "eip712_fields": ["hyperliquidChain", "user", "isFinalize", "nonce"]
        },
        "action": {
            "type": "linkStakingUser",
            "signatureChainId": chain.arbitrum_id().to_string(),
            "hyperliquidChain": chain.to_string(),
            "user": user.to_string(),
            "isFinalize": is_finalize,
            "nonce": "current_timestamp_ms"
        }
    }))
}

pub fn delegate_dry_run_value(
    chain: Chain,
    args: &DelegateArgs,
    action_kind: StakingActionKind,
) -> Result<serde_json::Value, CliError> {
    validate_delegate_args(args)?;
    match action_kind {
        StakingActionKind::Delegate | StakingActionKind::Undelegate => {}
        _ => {
            return Err(CliError::Internal(anyhow::anyhow!(
                "invalid staking delegation action kind"
            )));
        }
    };
    Ok(serde_json::json!({
        "validator": args.validator,
        "amount": args.amount.to_string(),
        "asset": "HYPE",
        "network": chain.to_string(),
        "reversibility": action_kind.reversibility(),
    }))
}

pub fn amount_dry_run_value(
    chain: Chain,
    args: &AmountArgs,
    action_kind: StakingActionKind,
) -> Result<serde_json::Value, CliError> {
    validate_amount_args(args)?;
    let note = match action_kind {
        StakingActionKind::Deposit => None,
        StakingActionKind::Withdraw => {
            Some("7-day withdrawal queue before funds become available in spot")
        }
        _ => {
            return Err(CliError::Internal(anyhow::anyhow!(
                "invalid staking amount action kind"
            )));
        }
    };
    let mut value = serde_json::json!({
        "amount": args.amount.to_string(),
        "asset": "HYPE",
        "network": chain.to_string(),
        "reversibility": action_kind.reversibility(),
    });
    if let Some(note) = note
        && let Some(object) = value.as_object_mut()
    {
        object.insert("note".to_string(), serde_json::json!(note));
    }
    Ok(value)
}

pub fn claim_rewards_dry_run_value(chain: Chain) -> serde_json::Value {
    serde_json::json!({
        "asset": "HYPE",
        "network": chain.to_string(),
        "reversibility": StakingActionKind::ClaimRewards.reversibility(),
    })
}

/// Link staking and trading accounts for fee discount attribution.
pub async fn link_staking_user(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    args: &LinkArgs,
    is_finalize: bool,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    let user = validate_link_user_address(&args.user)?;
    if !args.yes {
        confirm_staking_link(user, is_finalize, format)?;
    }

    let nonce = actions::nonce_now();
    let signature_chain_id = chain.arbitrum_id().to_string();
    let message = LinkStakingUserMessage {
        hyperliquid_chain: chain,
        user,
        is_finalize,
        nonce,
    };
    let signature = actions::sign_user_action::<LinkStakingUser>(signer, chain, &message)?;
    let action = LinkStakingUserAction {
        action_type: "linkStakingUser",
        signature_chain_id,
        hyperliquid_chain: chain,
        user,
        is_finalize,
        nonce,
    };
    actions::send_user_signed_json_action(
        api_base_url,
        serde_json::to_value(action).map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?,
        nonce,
        signature,
    )
    .await?;

    output::print_data(
        &LinkStakingOutput {
            row: LinkStakingConfirmation {
                status: "submitted".to_string(),
                action: "staking-link".to_string(),
                phase: link_phase(is_finalize).to_string(),
                acting_as: signer.query_address().to_string(),
                signer: signer.address().to_string(),
                user: user.to_string(),
                is_finalize,
                network: chain.to_string(),
                reversibility: format_reversibility(
                    action_kind_for_link(is_finalize).reversibility(),
                )
                .to_string(),
                warning: STAKING_LINK_WARNING.to_string(),
            },
        },
        format,
        start.elapsed(),
    );
    Ok(())
}

pub fn validate_delegate_args(args: &DelegateArgs) -> Result<(), CliError> {
    validate_validator_address(&args.validator)?;
    validate_amount_to_wei(args.amount)?;
    Ok(())
}

pub fn validate_amount_args(args: &AmountArgs) -> Result<(), CliError> {
    validate_amount_to_wei(args.amount)?;
    Ok(())
}

pub fn validate_link_args(args: &LinkArgs) -> Result<(), CliError> {
    validate_link_user_address(&args.user)?;
    Ok(())
}

async fn token_delegate(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    args: &DelegateArgs,
    is_undelegate: bool,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    let validator = validate_validator_address(&args.validator)?;
    let wei = validate_amount_to_wei(args.amount)?;
    let nonce = actions::nonce_now();
    let signature_chain_id = chain.arbitrum_id().to_string();
    let message = TokenDelegateMessage {
        signature_chain_id: signature_chain_id.clone(),
        hyperliquid_chain: chain,
        validator,
        wei,
        is_undelegate,
        nonce,
    };
    let signature = actions::sign_user_action::<TokenDelegate>(signer, chain, &message)?;
    let action = TokenDelegateAction {
        action_type: "tokenDelegate",
        signature_chain_id,
        hyperliquid_chain: chain,
        validator,
        wei,
        is_undelegate,
        nonce,
    };
    actions::send_user_signed_json_action(
        api_base_url,
        serde_json::to_value(action).map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?,
        nonce,
        signature,
    )
    .await?;

    let action_name = if is_undelegate {
        "undelegate"
    } else {
        "delegate"
    };
    print_action_confirmation(
        ActionConfirmationContext {
            chain,
            signer,
            action_kind: if is_undelegate {
                StakingActionKind::Undelegate
            } else {
                StakingActionKind::Delegate
            },
        },
        action_name,
        args.amount,
        Some(args.validator.clone()),
        None,
        format,
        start,
    );
    Ok(())
}

async fn delegator_summary(
    api_base_url: &str,
    address: &str,
) -> Result<DelegatorSummary, CliError> {
    post_info(
        api_base_url,
        &InfoRequest {
            request_type: "delegatorSummary",
            user: Some(address),
        },
    )
    .await
}

async fn delegations(api_base_url: &str, address: &str) -> Result<Vec<DelegationRow>, CliError> {
    post_info(
        api_base_url,
        &InfoRequest {
            request_type: "delegations",
            user: Some(address),
        },
    )
    .await
}

async fn rewards_rows(api_base_url: &str, address: &str) -> Result<Vec<RewardRow>, CliError> {
    post_info(
        api_base_url,
        &InfoRequest {
            request_type: "delegatorRewards",
            user: Some(address),
        },
    )
    .await
}

async fn post_info<T: for<'de> Deserialize<'de>>(
    api_base_url: &str,
    request: &impl Serialize,
) -> Result<T, CliError> {
    post_info_json(api_base_url, request, "").await
}

fn validate_address(address: &str) -> Result<Address, CliError> {
    address
        .parse::<Address>()
        .map_err(|_| CliError::Unsupported(format!("Invalid address: {address}")))
}

fn validate_validator_address(address: &str) -> Result<Address, CliError> {
    let parsed = validate_address(address)?;
    if parsed == Address::ZERO {
        return Err(CliError::Configuration(
            "validator address must not be the zero address".to_string(),
        ));
    }
    Ok(parsed)
}

fn validate_link_user_address(address: &str) -> Result<Address, CliError> {
    let parsed = validate_address(address)?;
    if parsed == Address::ZERO {
        return Err(CliError::Configuration(
            "staking-link user address must not be the zero address".to_string(),
        ));
    }
    Ok(parsed)
}

fn validate_amount_to_wei(amount: Decimal) -> Result<u64, CliError> {
    if amount <= Decimal::ZERO {
        return Err(CliError::Configuration(
            "amount must be greater than zero".to_string(),
        ));
    }
    let wei = amount * Decimal::from(HYPE_WEI_SCALE);
    if wei.fract() != Decimal::ZERO {
        return Err(CliError::Configuration(
            "amount supports at most 8 decimal places".to_string(),
        ));
    }
    wei.to_u64()
        .ok_or_else(|| CliError::Configuration("amount is too large".to_string()))
}

fn format_hype_wei(wei: u64) -> String {
    (Decimal::from(wei) / Decimal::from(HYPE_WEI_SCALE)).to_string()
}

fn delegation_json(row: &DelegationRow) -> serde_json::Value {
    serde_json::json!({
        "validator": row.validator,
        "amount": row.amount,
        "locked_until_timestamp": row.locked_until_timestamp,
    })
}

fn reward_json(row: &RewardRow) -> serde_json::Value {
    serde_json::json!({
        "time": row.time,
        "source": row.source,
        "total_amount": row.total_amount,
    })
}

fn link_phase(is_finalize: bool) -> &'static str {
    if is_finalize { "finalize" } else { "initiate" }
}

fn action_kind_for_link(is_finalize: bool) -> StakingActionKind {
    if is_finalize {
        StakingActionKind::LinkFinalize
    } else {
        StakingActionKind::LinkInitiate
    }
}

fn confirm_staking_link(
    user: Address,
    is_finalize: bool,
    format: OutputFormat,
) -> Result<(), CliError> {
    let phase = link_phase(is_finalize);
    let prompt = format!(
        "{warning}\nSubmit staking link {phase} for user {user}? [y/N] ",
        warning = STAKING_LINK_WARNING
    );
    let prompt = if format == OutputFormat::Pretty {
        output::colors::yellow(&prompt)
    } else {
        prompt
    };
    use std::io::Write;
    let mut stderr = std::io::stderr();
    write!(stderr, "{prompt}").map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    stderr
        .flush()
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    if matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(())
    } else {
        Err(CliError::Configuration(
            "staking-link confirmation required; action cancelled. Rerun with --yes for deliberate automation.".to_string(),
        ))
    }
}

fn print_action_confirmation(
    context: ActionConfirmationContext<'_>,
    action: &str,
    amount: Decimal,
    validator: Option<String>,
    note: Option<String>,
    format: OutputFormat,
    start: Instant,
) {
    output::print_data(
        &ActionOutput {
            row: ActionConfirmation {
                action: action.to_string(),
                status: "submitted".to_string(),
                network: context.chain.to_string(),
                signer: context.signer.address().to_string(),
                acting_as: context.signer.query_address().to_string(),
                reversibility: format_reversibility(context.action_kind.reversibility())
                    .to_string(),
                amount,
                asset: "HYPE".to_string(),
                validator,
                note,
            },
        },
        format,
        start.elapsed(),
    );
}

fn format_reversibility(reversibility: ActionReversibility) -> &'static str {
    match reversibility {
        ActionReversibility::Reversible => "reversible",
        ActionReversibility::PartiallyReversible => "partially_reversible",
        ActionReversibility::Irreversible => "irreversible",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amount_to_wei_uses_eight_decimal_places() {
        assert_eq!(
            validate_amount_to_wei("1.5".parse::<Decimal>().unwrap()).unwrap(),
            150_000_000
        );
        assert_eq!(
            validate_amount_to_wei("0.00000001".parse::<Decimal>().unwrap()).unwrap(),
            1
        );
    }

    #[test]
    fn amount_to_wei_rejects_zero_negative_and_over_precise_amounts() {
        assert!(validate_amount_to_wei(Decimal::ZERO).is_err());
        assert!(validate_amount_to_wei("-1".parse::<Decimal>().unwrap()).is_err());
        assert!(validate_amount_to_wei("0.000000001".parse::<Decimal>().unwrap()).is_err());
    }

    #[test]
    fn validator_address_rejects_zero_address() {
        let err =
            validate_validator_address("0x0000000000000000000000000000000000000000").unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn staking_actions_declare_reversibility() {
        assert_eq!(
            StakingActionKind::Delegate.reversibility(),
            ActionReversibility::PartiallyReversible
        );
        assert_eq!(
            StakingActionKind::Undelegate.reversibility(),
            ActionReversibility::PartiallyReversible
        );
        assert_eq!(
            StakingActionKind::Deposit.reversibility(),
            ActionReversibility::PartiallyReversible
        );
        assert_eq!(
            StakingActionKind::Withdraw.reversibility(),
            ActionReversibility::PartiallyReversible
        );
        assert_eq!(
            StakingActionKind::ClaimRewards.reversibility(),
            ActionReversibility::Irreversible
        );
        assert_eq!(
            StakingActionKind::LinkInitiate.reversibility(),
            ActionReversibility::Irreversible
        );
        assert_eq!(
            StakingActionKind::LinkFinalize.reversibility(),
            ActionReversibility::Irreversible
        );
    }
}
