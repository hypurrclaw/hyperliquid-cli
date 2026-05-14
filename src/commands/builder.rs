//! Builder fee approval and status commands.
//!
//! Commands:
//! - `hyperliquid builder max-fee --user <ADDRESS> --builder <ADDRESS>`
//! - `hyperliquid builder approved --user <ADDRESS>`
//! - `hyperliquid builder approve --builder <ADDRESS> --max-fee-rate <PERCENT>`

use std::io::{self, Write};
use std::time::Instant;

use alloy::sol;
use clap::Args;
use futures::future::try_join_all;
use hypersdk::Address;
use hypersdk::Decimal;
use hypersdk::hypercore::Chain;
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;

use crate::commands::actions;
use crate::dry_run::ActionReversibility;
use crate::errors::CliError;
use crate::http_api::post_info_json;
use crate::output::{OutputFormat, TableData, colors};
use crate::signing::SelectedSigner;

/// Compile-time default builder address captured from the build environment.
///
/// Set `HYPERLIQUID_DEFAULT_BUILDER_ADDRESS` while building the binary to bake in
/// a default builder for `orders create` when users don't pass `--builder`.
/// A runtime env var with the same name still overrides this value.
pub const DEFAULT_BUILDER_ADDRESS: &str = match option_env!("HYPERLIQUID_DEFAULT_BUILDER_ADDRESS") {
    Some(address) => address,
    None => "",
};
/// Compile-time default builder fee rate captured from the build environment
/// (percent string, e.g. "0.001%"). Must be paired with
/// `HYPERLIQUID_DEFAULT_BUILDER_ADDRESS`.
pub const DEFAULT_BUILDER_FEE_RATE: &str = match option_env!("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE")
{
    Some(fee_rate) => fee_rate,
    None => "",
};

sol! {
    struct ApproveBuilderFee {
        string hyperliquidChain;
        string maxFeeRate;
        address builder;
        uint64 nonce;
    }
}

#[derive(Args, Debug, Clone)]
pub struct MaxFeeArgs {
    /// User address, stored account alias, or stored account id
    #[arg(long)]
    pub user: String,

    /// Builder address
    #[arg(long)]
    pub builder: String,
}

#[derive(Args, Debug, Clone)]
pub struct ApprovedArgs {
    /// User address, stored account alias, or stored account id
    #[arg(long)]
    pub user: String,
}

#[derive(Args, Debug, Clone)]
pub struct ApproveArgs {
    /// Builder address
    #[arg(long)]
    pub builder: String,

    /// Maximum builder fee rate as a percent string, e.g. 0.001%
    #[arg(long)]
    pub max_fee_rate: String,

    /// Confirm live execution without prompting
    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct BuilderMaxFeeRow {
    user: String,
    builder: String,
    max_fee_tenths_bps: u64,
    max_fee_rate: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderMaxFeeOutput {
    row: BuilderMaxFeeRow,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct BuilderApprovedRow {
    user: String,
    builder: String,
    max_fee_tenths_bps: u64,
    max_fee_rate: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderApprovedOutput {
    rows: Vec<BuilderApprovedRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderMaxFeeResult {
    pub output: BuilderMaxFeeOutput,
    pub elapsed: std::time::Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderApprovedResult {
    pub output: BuilderApprovedOutput,
    pub elapsed: std::time::Duration,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct BuilderApproveRow {
    status: String,
    action: String,
    signer: String,
    query_address: String,
    builder: String,
    max_fee_rate: String,
    max_fee_tenths_bps: u64,
    network: String,
    reversibility: String,
}

struct BuilderApproveOutput {
    row: BuilderApproveRow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuilderActionKind {
    ApproveFee,
}

impl BuilderActionKind {
    #[must_use]
    pub fn reversibility(self) -> ActionReversibility {
        match self {
            Self::ApproveFee => ActionReversibility::Reversible,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MaxBuilderFeeRequest {
    #[serde(rename = "type")]
    request_type: &'static str,
    user: Address,
    builder: Address,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApprovedBuildersRequest {
    #[serde(rename = "type")]
    request_type: &'static str,
    user: Address,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApproveBuilderFeeMessage {
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    max_fee_rate: String,
    builder: Address,
    nonce: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApproveBuilderFeeAction {
    #[serde(rename = "type")]
    action_type: &'static str,
    signature_chain_id: String,
    hyperliquid_chain: Chain,
    max_fee_rate: String,
    builder: Address,
    nonce: u64,
}

impl TableData for BuilderMaxFeeOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["User", "Builder", "Max Fee", "Tenths Bps"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.row.user.clone(),
            self.row.builder.clone(),
            self.row.max_fee_rate.clone(),
            self.row.max_fee_tenths_bps.to_string(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.row).unwrap_or_else(|_| serde_json::json!({}))
    }
}

impl TableData for BuilderApprovedOutput {
    fn headers(&self) -> Vec<&str> {
        if self.rows.is_empty() {
            vec!["Message"]
        } else {
            vec!["User", "Builder", "Max Fee", "Tenths Bps"]
        }
    }

    fn rows(&self) -> Vec<Vec<String>> {
        if self.rows.is_empty() {
            return vec![vec!["No approved builders found".to_string()]];
        }
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.user.clone(),
                    row.builder.clone(),
                    row.max_fee_rate.clone(),
                    row.max_fee_tenths_bps.to_string(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

impl TableData for BuilderApproveOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Status",
            "Action",
            "Signer",
            "Query Address",
            "Builder",
            "Max Fee",
            "Tenths Bps",
            "Network",
            "Reversibility",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.row.status.clone(),
            self.row.action.clone(),
            self.row.signer.clone(),
            self.row.query_address.clone(),
            self.row.builder.clone(),
            self.row.max_fee_rate.clone(),
            self.row.max_fee_tenths_bps.to_string(),
            self.row.network.clone(),
            self.row.reversibility.clone(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.row).unwrap_or_else(|_| serde_json::json!({}))
    }
}

pub async fn max_fee(
    api_base_url: &str,
    user: Address,
    builder: Address,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = max_fee_query(api_base_url, user, builder).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

pub async fn approved(
    api_base_url: &str,
    user: Address,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = approved_query(api_base_url, user).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

pub async fn max_fee_query(
    api_base_url: &str,
    user: Address,
    builder: Address,
) -> Result<BuilderMaxFeeResult, anyhow::Error> {
    let start = Instant::now();
    let max_fee_tenths_bps = load_max_fee(api_base_url, user, builder).await?;
    Ok(BuilderMaxFeeResult {
        output: BuilderMaxFeeOutput {
            row: BuilderMaxFeeRow {
                user: user.to_string(),
                builder: builder.to_string(),
                max_fee_tenths_bps,
                max_fee_rate: percent_from_tenths_bps(max_fee_tenths_bps),
            },
        },
        elapsed: start.elapsed(),
    })
}

pub async fn approved_query(
    api_base_url: &str,
    user: Address,
) -> Result<BuilderApprovedResult, anyhow::Error> {
    let start = Instant::now();
    let builders = post_info_json::<Vec<Address>>(
        api_base_url,
        &ApprovedBuildersRequest {
            request_type: "approvedBuilders",
            user,
        },
        "loading approved builders",
    )
    .await?;

    let rows = try_join_all(builders.into_iter().map(|builder| async move {
        let max_fee_tenths_bps = load_max_fee(api_base_url, user, builder).await?;
        Ok::<_, CliError>(BuilderApprovedRow {
            user: user.to_string(),
            builder: builder.to_string(),
            max_fee_tenths_bps,
            max_fee_rate: percent_from_tenths_bps(max_fee_tenths_bps),
        })
    }))
    .await?;

    Ok(BuilderApprovedResult {
        output: BuilderApprovedOutput { rows },
        elapsed: start.elapsed(),
    })
}

pub fn approve_dry_run_value(
    chain: Chain,
    signer: Option<Address>,
    query_address: Option<Address>,
    builder: Address,
    args: &ApproveArgs,
) -> Result<serde_json::Value, CliError> {
    let max_fee_tenths_bps = validate_max_fee_rate(&args.max_fee_rate)?;
    Ok(serde_json::json!({
        "network": chain.to_string(),
        "signer": signer.map(|address| address.to_string()),
        "query_address": query_address.map(|address| address.to_string()),
        "builder": builder.to_string(),
        "max_fee_rate": args.max_fee_rate,
        "max_fee_tenths_bps": max_fee_tenths_bps,
        "reversibility": BuilderActionKind::ApproveFee.reversibility(),
        "action": {
            "type": "approveBuilderFee",
            "hyperliquidChain": chain.to_string(),
            "signatureChainId": chain.arbitrum_id().to_string(),
            "builder": builder.to_string(),
            "maxFeeRate": args.max_fee_rate,
        }
    }))
}

async fn load_max_fee(
    api_base_url: &str,
    user: Address,
    builder: Address,
) -> Result<u64, CliError> {
    post_info_json::<u64>(
        api_base_url,
        &MaxBuilderFeeRequest {
            request_type: "maxBuilderFee",
            user,
            builder,
        },
        "loading max builder fee",
    )
    .await
}

pub async fn approve(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    builder: Address,
    args: &ApproveArgs,
    require_confirmation: bool,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let max_fee_tenths_bps = validate_max_fee_rate(&args.max_fee_rate)?;
    if require_confirmation && !args.yes {
        confirm_builder_approval(builder, &args.max_fee_rate, max_fee_tenths_bps, format)?;
    }

    let start = Instant::now();
    submit_approval(api_base_url, chain, signer, builder, &args.max_fee_rate).await?;

    crate::output::print_data(
        &BuilderApproveOutput {
            row: approve_output_row(
                chain,
                signer,
                builder,
                &args.max_fee_rate,
                max_fee_tenths_bps,
            ),
        },
        format,
        start.elapsed(),
    );
    Ok(())
}

pub async fn submit_approval(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    builder: Address,
    max_fee_rate: &str,
) -> Result<u64, anyhow::Error> {
    let max_fee_tenths_bps = validate_max_fee_rate(max_fee_rate)?;
    let nonce = actions::nonce_now();
    let message = ApproveBuilderFeeMessage {
        signature_chain_id: chain.arbitrum_id().to_string(),
        hyperliquid_chain: chain,
        max_fee_rate: max_fee_rate.to_string(),
        builder,
        nonce,
    };
    let signature = actions::sign_user_action::<ApproveBuilderFee>(signer, chain, &message)?;
    let action = ApproveBuilderFeeAction {
        action_type: "approveBuilderFee",
        signature_chain_id: chain.arbitrum_id().to_string(),
        hyperliquid_chain: chain,
        max_fee_rate: max_fee_rate.to_string(),
        builder,
        nonce,
    };
    actions::send_user_signed_json_action(
        api_base_url,
        serde_json::to_value(action).map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?,
        nonce,
        signature,
    )
    .await?;
    Ok(max_fee_tenths_bps)
}

fn approve_output_row(
    chain: Chain,
    signer: &SelectedSigner,
    builder: Address,
    max_fee_rate: &str,
    max_fee_tenths_bps: u64,
) -> BuilderApproveRow {
    BuilderApproveRow {
        status: "submitted".to_string(),
        action: "approve-builder-fee".to_string(),
        signer: signer.address().to_string(),
        query_address: signer.query_address().to_string(),
        builder: builder.to_string(),
        max_fee_rate: max_fee_rate.to_string(),
        max_fee_tenths_bps,
        network: chain.to_string(),
        reversibility: format_reversibility(BuilderActionKind::ApproveFee.reversibility())
            .to_string(),
    }
}

pub fn parse_builder_address(raw: &str) -> Result<Address, CliError> {
    let address = raw
        .parse::<Address>()
        .map_err(|_| CliError::Unsupported(format!("Invalid builder address: {raw}")))?;
    if address == Address::ZERO {
        return Err(CliError::Unsupported(
            "builder address cannot be the zero address".to_string(),
        ));
    }
    Ok(address)
}

/// Resolve the default builder address and fee rate.
///
/// Priority: `HYPERLIQUID_DEFAULT_BUILDER_ADDRESS` /
/// `HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE` env vars → config file
/// `default_builder_address` / `default_builder_fee_rate` → compile-time
/// `DEFAULT_BUILDER_ADDRESS` / `DEFAULT_BUILDER_FEE_RATE`.
/// Returns `Ok(None)` when nothing is configured.
/// Returns `Err` when a source sets only one side or contains invalid values.
pub fn resolve_default_builder_fee() -> Result<Option<(Address, u64)>, CliError> {
    let env_address = std::env::var("HYPERLIQUID_DEFAULT_BUILDER_ADDRESS")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let env_fee = std::env::var("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if env_address.is_some() || env_fee.is_some() {
        return parse_default_builder_pair(
            env_address,
            env_fee,
            "HYPERLIQUID_DEFAULT_BUILDER_ADDRESS",
            "HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE",
        );
    }

    let config = crate::config::load_config()
        .map_err(|err| CliError::Configuration(format!("failed to load config: {err}")))?;
    resolve_default_builder_fee_from_config(config.as_ref())
}

pub fn resolve_default_builder_fee_from_config(
    config: Option<&crate::config::Config>,
) -> Result<Option<(Address, u64)>, CliError> {
    let env_address = std::env::var("HYPERLIQUID_DEFAULT_BUILDER_ADDRESS")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let env_fee = std::env::var("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if env_address.is_some() || env_fee.is_some() {
        return parse_default_builder_pair(
            env_address,
            env_fee,
            "HYPERLIQUID_DEFAULT_BUILDER_ADDRESS",
            "HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE",
        );
    }

    let config_address = config
        .and_then(|config| config.default_builder_address.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let config_fee = config
        .and_then(|config| config.default_builder_fee_rate.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    if config_address.is_some() || config_fee.is_some() {
        return parse_default_builder_pair(
            config_address,
            config_fee,
            "default_builder_address",
            "default_builder_fee_rate",
        );
    }

    let const_address = if DEFAULT_BUILDER_ADDRESS.is_empty() {
        None
    } else {
        Some(DEFAULT_BUILDER_ADDRESS.to_string())
    };
    let const_fee = if DEFAULT_BUILDER_FEE_RATE.is_empty() {
        None
    } else {
        Some(DEFAULT_BUILDER_FEE_RATE.to_string())
    };
    parse_default_builder_pair(
        const_address,
        const_fee,
        "DEFAULT_BUILDER_ADDRESS",
        "DEFAULT_BUILDER_FEE_RATE",
    )
}

fn parse_default_builder_pair(
    address_str: Option<String>,
    fee_str: Option<String>,
    address_label: &str,
    fee_label: &str,
) -> Result<Option<(Address, u64)>, CliError> {
    match (address_str, fee_str) {
        (None, None) => Ok(None),
        (Some(_), None) => Err(CliError::Configuration(format!(
            "{address_label} is set but {fee_label} is not; both must be configured"
        ))),
        (None, Some(_)) => Err(CliError::Configuration(format!(
            "{fee_label} is set but {address_label} is not; both must be configured"
        ))),
        (Some(address), Some(fee)) => {
            let address = parse_builder_address(&address).map_err(|err| {
                CliError::Configuration(format!("invalid {address_label}: {err}"))
            })?;
            let fee_tenths_bps = validate_max_fee_rate(&fee)
                .map_err(|err| CliError::Configuration(format!("invalid {fee_label}: {err}")))?;
            Ok(Some((address, fee_tenths_bps)))
        }
    }
}

pub fn validate_max_fee_rate(raw: &str) -> Result<u64, CliError> {
    let Some(percent) = raw.strip_suffix('%') else {
        return Err(CliError::Unsupported(
            "builder max fee rate must be a percent string such as 0.001%".to_string(),
        ));
    };
    if percent.trim() != percent || percent.is_empty() {
        return Err(CliError::Unsupported(
            "builder max fee rate must not contain whitespace".to_string(),
        ));
    }
    let value = percent.parse::<Decimal>().map_err(|_| {
        CliError::Unsupported("builder max fee rate must be a decimal percent".to_string())
    })?;
    if value < Decimal::ZERO {
        return Err(CliError::Unsupported(
            "builder max fee rate cannot be negative".to_string(),
        ));
    }
    if value > Decimal::ONE {
        return Err(CliError::Unsupported(
            "builder max fee rate cannot exceed 1%".to_string(),
        ));
    }
    let tenths_bps = value * Decimal::from(1000);
    if tenths_bps.fract() != Decimal::ZERO {
        return Err(CliError::Unsupported(
            "builder max fee rate must be a multiple of 0.001%".to_string(),
        ));
    }
    tenths_bps.to_u64().ok_or_else(|| {
        CliError::Unsupported("builder max fee rate is outside supported bounds".to_string())
    })
}

pub(crate) fn percent_from_tenths_bps(value: u64) -> String {
    let percent = Decimal::from(value) / Decimal::from(1000);
    format!("{percent}%")
}

fn confirm_builder_approval(
    builder: Address,
    max_fee_rate: &str,
    max_fee_tenths_bps: u64,
    format: OutputFormat,
) -> Result<(), CliError> {
    let prompt = format!(
        "Approve builder {builder} for max fee {max_fee_rate} ({max_fee_tenths_bps} tenths bps)? [y/N] "
    );
    let prompt = if format == OutputFormat::Pretty {
        colors::yellow(&prompt)
    } else {
        prompt
    };
    let mut stderr = io::stderr();
    write!(stderr, "{prompt}").map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    stderr
        .flush()
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
    if matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(())
    } else {
        Err(CliError::Configuration(
            "builder fee approval aborted".to_string(),
        ))
    }
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
    fn max_fee_rate_percent_converts_to_tenths_bps() {
        assert_eq!(validate_max_fee_rate("0%").unwrap(), 0);
        assert_eq!(validate_max_fee_rate("0.001%").unwrap(), 1);
        assert_eq!(validate_max_fee_rate("0.01%").unwrap(), 10);
        assert_eq!(validate_max_fee_rate("0.1%").unwrap(), 100);
        assert_eq!(validate_max_fee_rate("1%").unwrap(), 1000);
    }

    #[test]
    fn max_fee_rate_rejects_ambiguous_or_over_limit_values() {
        assert!(validate_max_fee_rate("0.0001%").is_err());
        assert!(validate_max_fee_rate("1.001%").is_err());
        assert!(validate_max_fee_rate("1").is_err());
        assert!(validate_max_fee_rate("-0.001%").is_err());
    }

    #[test]
    fn builder_actions_declare_reversibility() {
        assert_eq!(
            BuilderActionKind::ApproveFee.reversibility(),
            ActionReversibility::Reversible
        );
    }

    #[test]
    fn resolve_default_builder_fee_uses_compile_time_defaults_when_env_absent() {
        let _guard = env_guard();
        unsafe {
            std::env::remove_var("HYPERLIQUID_DEFAULT_BUILDER_ADDRESS");
            std::env::remove_var("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE");
        }

        let result = resolve_default_builder_fee_from_config(None);
        match (
            DEFAULT_BUILDER_ADDRESS.is_empty(),
            DEFAULT_BUILDER_FEE_RATE.is_empty(),
        ) {
            (true, true) => assert!(result.unwrap().is_none()),
            (false, false) => assert!(result.unwrap().is_some()),
            _ => assert!(result.is_err()),
        }
    }

    #[test]
    fn resolve_default_builder_fee_errs_when_only_address_set() {
        let _guard = env_guard();
        unsafe {
            std::env::remove_var("HYPERLIQUID_DEFAULT_BUILDER_ADDRESS");
            std::env::remove_var("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE");
            std::env::set_var(
                "HYPERLIQUID_DEFAULT_BUILDER_ADDRESS",
                "0x0000000000000000000000000000000000000001",
            );
        }
        let result = resolve_default_builder_fee();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("FEE_RATE"));
    }

    #[test]
    fn resolve_default_builder_fee_errs_when_only_fee_set() {
        let _guard = env_guard();
        unsafe {
            std::env::remove_var("HYPERLIQUID_DEFAULT_BUILDER_ADDRESS");
            std::env::remove_var("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE");
            std::env::set_var("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE", "0.001%");
        }
        let result = resolve_default_builder_fee();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("BUILDER_ADDRESS"));
    }

    #[test]
    fn resolve_default_builder_fee_errs_on_invalid_address() {
        let _guard = env_guard();
        unsafe {
            std::env::remove_var("HYPERLIQUID_DEFAULT_BUILDER_ADDRESS");
            std::env::remove_var("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE");
            std::env::set_var("HYPERLIQUID_DEFAULT_BUILDER_ADDRESS", "not-an-address");
            std::env::set_var("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE", "0.001%");
        }
        assert!(resolve_default_builder_fee().is_err());
    }

    #[test]
    fn resolve_default_builder_fee_errs_on_invalid_fee() {
        let _guard = env_guard();
        unsafe {
            std::env::remove_var("HYPERLIQUID_DEFAULT_BUILDER_ADDRESS");
            std::env::remove_var("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE");
            std::env::set_var(
                "HYPERLIQUID_DEFAULT_BUILDER_ADDRESS",
                "0x0000000000000000000000000000000000000001",
            );
            std::env::set_var("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE", "not-a-percent");
        }
        assert!(resolve_default_builder_fee().is_err());
    }

    #[test]
    fn resolve_default_builder_fee_succeeds_with_valid_env_vars() {
        let _guard = env_guard();
        unsafe {
            std::env::remove_var("HYPERLIQUID_DEFAULT_BUILDER_ADDRESS");
            std::env::remove_var("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE");
            std::env::set_var(
                "HYPERLIQUID_DEFAULT_BUILDER_ADDRESS",
                "0x0000000000000000000000000000000000000001",
            );
            std::env::set_var("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE", "0.001%");
        }
        let result = resolve_default_builder_fee().unwrap().unwrap();
        assert_eq!(
            result.0.to_string(),
            "0x0000000000000000000000000000000000000001"
        );
        assert_eq!(result.1, 1); // 0.001% = 1 tenth bps
    }
}

/// Serializes env-var tests to avoid races. Restores variables on drop.
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
                std::env::remove_var("HYPERLIQUID_DEFAULT_BUILDER_ADDRESS");
                std::env::remove_var("HYPERLIQUID_DEFAULT_BUILDER_FEE_RATE");
            }
        }
    }
    EnvRestore { _guard: guard }
}
