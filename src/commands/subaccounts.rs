//! Authenticated subaccount workflows.

use std::io::{self, Write};
use std::time::Instant;

use clap::{Args, ValueEnum};
use hypersdk::hypercore::Chain;
use hypersdk::{Address, Decimal};
use rust_decimal::prelude::ToPrimitive;
use serde::Serialize;

use crate::commands::actions;
use crate::dry_run::ActionReversibility;
use crate::errors::CliError;
use crate::output::{self, OutputFormat, TableData};
use crate::signing::SelectedSigner;

const USDC_WIRE_SCALE: u64 = 1_000_000;

/// Arguments for `subaccount create`.
#[derive(Args, Debug, Clone)]
pub struct CreateArgs {
    /// Display name for the new subaccount
    #[arg(long)]
    pub name: String,
}

/// Arguments for `subaccount transfer`.
#[derive(Args, Debug, Clone)]
pub struct TransferArgs {
    /// Subaccount acting-account selector: subaccount address, stored account alias, or stored account id
    #[arg(long)]
    pub subaccount: String,

    /// Amount of USDC to transfer
    #[arg(long, allow_hyphen_values = true)]
    pub amount: Decimal,

    /// Transfer direction relative to the subaccount
    #[arg(long, value_enum)]
    pub direction: TransferDirection,

    /// Skip the confirmation prompt
    #[arg(short = 'y', long)]
    pub yes: bool,
}

/// Arguments for `subaccount spot-transfer`.
#[derive(Args, Debug, Clone)]
pub struct SpotTransferArgs {
    /// Subaccount acting-account selector: subaccount address, stored account alias, or stored account id
    #[arg(long)]
    pub subaccount: String,

    /// Spot token identifier, for example PURR:0xc4bf3f870c0e9465323c0b6ed28096c2
    #[arg(long)]
    pub token: String,

    /// Token amount to transfer
    #[arg(long, allow_hyphen_values = true)]
    pub amount: Decimal,

    /// Transfer direction relative to the subaccount
    #[arg(long, value_enum)]
    pub direction: TransferDirection,

    /// Skip the confirmation prompt
    #[arg(short = 'y', long)]
    pub yes: bool,
}

/// Subaccount transfer direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferDirection {
    Deposit,
    Withdraw,
}

impl TransferDirection {
    pub fn is_deposit(self) -> bool {
        matches!(self, Self::Deposit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubaccountActionKind {
    Create,
    TransferUsdc,
    TransferSpot,
}

impl SubaccountActionKind {
    #[must_use]
    pub fn reversibility(self) -> ActionReversibility {
        match self {
            Self::Create => ActionReversibility::Irreversible,
            Self::TransferUsdc | Self::TransferSpot => ActionReversibility::PartiallyReversible,
        }
    }
}

impl std::fmt::Display for TransferDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deposit => write!(f, "deposit"),
            Self::Withdraw => write!(f, "withdraw"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct SubaccountActionConfirmation {
    action: String,
    status: String,
    network: String,
    signer: String,
    acting_as: String,
    reversibility: String,
    vault_address: Option<String>,
    subaccount: Option<String>,
    name: Option<String>,
    direction: Option<String>,
    amount: Option<String>,
    usd: Option<u64>,
    token: Option<String>,
}

struct SubaccountActionOutput {
    rows: Vec<SubaccountActionConfirmation>,
}

impl TableData for SubaccountActionOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Action",
            "Status",
            "Network",
            "Signer",
            "Acting As",
            "Reversibility",
            "Vault Address",
            "Subaccount",
            "Name",
            "Direction",
            "Amount",
            "USD Units",
            "Token",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.action.clone(),
                    row.status.clone(),
                    row.network.clone(),
                    row.signer.clone(),
                    row.acting_as.clone(),
                    row.reversibility.clone(),
                    row.vault_address.clone().unwrap_or_default(),
                    row.subaccount.clone().unwrap_or_default(),
                    row.name.clone().unwrap_or_default(),
                    row.direction.clone().unwrap_or_default(),
                    row.amount.clone().unwrap_or_default(),
                    row.usd.map(|usd| usd.to_string()).unwrap_or_default(),
                    row.token.clone().unwrap_or_default(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateSubaccountAction {
    #[serde(rename = "type")]
    action_type: &'static str,
    name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubaccountTransferAction {
    #[serde(rename = "type")]
    action_type: &'static str,
    sub_account_user: String,
    is_deposit: bool,
    usd: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SubaccountSpotTransferAction {
    #[serde(rename = "type")]
    action_type: &'static str,
    sub_account_user: String,
    is_deposit: bool,
    token: String,
    amount: String,
}

pub fn validate_create_args(args: &CreateArgs) -> Result<(), CliError> {
    if args.name.trim().is_empty() {
        return Err(CliError::Configuration(
            "subaccount name cannot be empty".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_transfer_args(args: &TransferArgs) -> Result<(), CliError> {
    usdc_to_wire_units(args.amount)?;
    Ok(())
}

pub fn validate_spot_transfer_args(args: &SpotTransferArgs) -> Result<(), CliError> {
    validate_positive_amount(args.amount, "amount")?;
    if args.token.trim().is_empty() {
        return Err(CliError::Configuration("token cannot be empty".to_string()));
    }
    Ok(())
}

pub fn create_dry_run_value(chain: Chain, args: &CreateArgs) -> serde_json::Value {
    serde_json::json!({
        "name": args.name.trim(),
        "network": chain.to_string(),
        "reversibility": SubaccountActionKind::Create.reversibility(),
    })
}

pub fn transfer_dry_run_value(
    chain: Chain,
    subaccount: Address,
    args: &TransferArgs,
) -> Result<serde_json::Value, CliError> {
    let usd = usdc_to_wire_units(args.amount)?;
    Ok(serde_json::json!({
        "subaccount": subaccount.to_string(),
        "amount": args.amount.to_string(),
        "direction": args.direction.to_string(),
        "is_deposit": args.direction.is_deposit(),
        "usd": usd,
        "asset": "USDC",
        "network": chain.to_string(),
        "reversibility": SubaccountActionKind::TransferUsdc.reversibility(),
    }))
}

pub fn spot_transfer_dry_run_value(
    chain: Chain,
    subaccount: Address,
    args: &SpotTransferArgs,
) -> Result<serde_json::Value, CliError> {
    validate_spot_transfer_args(args)?;
    Ok(serde_json::json!({
        "subaccount": subaccount.to_string(),
        "token": args.token.trim(),
        "amount": args.amount.to_string(),
        "direction": args.direction.to_string(),
        "is_deposit": args.direction.is_deposit(),
        "asset": args.token.trim(),
        "network": chain.to_string(),
        "reversibility": SubaccountActionKind::TransferSpot.reversibility(),
    }))
}

pub fn usdc_to_wire_units(amount: Decimal) -> Result<u64, CliError> {
    validate_positive_amount(amount, "amount")?;
    let scaled = amount
        .checked_mul(Decimal::from(USDC_WIRE_SCALE))
        .ok_or_else(|| CliError::Configuration("amount is too large".to_string()))?;
    let normalized = scaled.normalize();
    if normalized.scale() != 0 {
        return Err(CliError::Configuration(
            "subaccount USDC transfer amount supports at most 6 decimal places".to_string(),
        ));
    }
    normalized
        .to_u64()
        .ok_or_else(|| CliError::Configuration("amount is too large".to_string()))
}

/// Create a new subaccount signed by the configured master account.
pub async fn create(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    args: &CreateArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    validate_create_args(args)?;
    let start = Instant::now();
    let action = CreateSubaccountAction {
        action_type: "createSubAccount",
        name: args.name.trim().to_string(),
    };

    actions::send_raw_l1_json_action(
        api_base_url,
        chain,
        signer,
        &action,
        actions::RawL1ActionMetadata::new(actions::nonce_now()),
        "create subaccount failed",
    )
    .await?;

    let signer_address = signer.address().to_string();
    print_action(
        SubaccountActionConfirmation {
            action: "create".to_string(),
            status: "submitted".to_string(),
            network: chain.to_string(),
            signer: signer_address,
            acting_as: signer.query_address().to_string(),
            reversibility: format_reversibility(SubaccountActionKind::Create.reversibility())
                .to_string(),
            vault_address: None,
            subaccount: None,
            name: Some(args.name.trim().to_string()),
            direction: None,
            amount: None,
            usd: None,
            token: None,
        },
        format,
        start,
    );
    Ok(())
}

/// Transfer USDC between the configured master account and a subaccount.
pub async fn transfer(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    subaccount: Address,
    args: &TransferArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    validate_transfer_args(args)?;
    let start = Instant::now();
    let usd = usdc_to_wire_units(args.amount)?;
    if !args.yes {
        confirm_transfer(
            &transfer_prompt(subaccount, args),
            "subaccount transfer aborted",
            format,
        )?;
    }

    let action = SubaccountTransferAction {
        action_type: "subAccountTransfer",
        sub_account_user: subaccount.to_string().to_lowercase(),
        is_deposit: args.direction.is_deposit(),
        usd,
    };
    actions::send_raw_l1_json_action(
        api_base_url,
        chain,
        signer,
        &action,
        actions::RawL1ActionMetadata::new(actions::nonce_now()),
        "subaccount transfer failed",
    )
    .await?;

    let signer_address = signer.address().to_string();
    print_action(
        SubaccountActionConfirmation {
            action: "transfer".to_string(),
            status: "confirmed".to_string(),
            network: chain.to_string(),
            signer: signer_address,
            acting_as: signer.query_address().to_string(),
            reversibility: format_reversibility(SubaccountActionKind::TransferUsdc.reversibility())
                .to_string(),
            vault_address: None,
            subaccount: Some(subaccount.to_string()),
            name: None,
            direction: Some(args.direction.to_string()),
            amount: Some(args.amount.normalize().to_string()),
            usd: Some(usd),
            token: Some("USDC".to_string()),
        },
        format,
        start,
    );
    Ok(())
}

/// Transfer a spot token between the configured master account and a subaccount.
pub async fn spot_transfer(
    api_base_url: &str,
    chain: Chain,
    signer: &SelectedSigner,
    subaccount: Address,
    args: &SpotTransferArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    validate_spot_transfer_args(args)?;
    let start = Instant::now();
    if !args.yes {
        confirm_transfer(
            &spot_transfer_prompt(subaccount, args),
            "subaccount spot transfer aborted",
            format,
        )?;
    }

    let amount = args.amount.normalize().to_string();
    let action = SubaccountSpotTransferAction {
        action_type: "subAccountSpotTransfer",
        sub_account_user: subaccount.to_string().to_lowercase(),
        is_deposit: args.direction.is_deposit(),
        token: args.token.trim().to_string(),
        amount: amount.clone(),
    };
    actions::send_raw_l1_json_action(
        api_base_url,
        chain,
        signer,
        &action,
        actions::RawL1ActionMetadata::new(actions::nonce_now()),
        "subaccount spot transfer failed",
    )
    .await?;

    let signer_address = signer.address().to_string();
    print_action(
        SubaccountActionConfirmation {
            action: "spot-transfer".to_string(),
            status: "confirmed".to_string(),
            network: chain.to_string(),
            signer: signer_address,
            acting_as: signer.query_address().to_string(),
            reversibility: format_reversibility(SubaccountActionKind::TransferSpot.reversibility())
                .to_string(),
            vault_address: None,
            subaccount: Some(subaccount.to_string()),
            name: None,
            direction: Some(args.direction.to_string()),
            amount: Some(amount),
            usd: None,
            token: Some(args.token.trim().to_string()),
        },
        format,
        start,
    );
    Ok(())
}

fn validate_positive_amount(amount: Decimal, name: &'static str) -> Result<(), CliError> {
    if amount <= Decimal::ZERO {
        return Err(CliError::Configuration(format!(
            "{name} must be greater than zero"
        )));
    }
    Ok(())
}

fn transfer_prompt(subaccount: Address, args: &TransferArgs) -> String {
    if args.direction.is_deposit() {
        format!(
            "Transfer {} USDC to subaccount {}? [y/N] ",
            args.amount, subaccount
        )
    } else {
        format!(
            "Transfer {} USDC from subaccount {}? [y/N] ",
            args.amount, subaccount
        )
    }
}

fn spot_transfer_prompt(subaccount: Address, args: &SpotTransferArgs) -> String {
    if args.direction.is_deposit() {
        format!(
            "Transfer {} {} to subaccount {}? [y/N] ",
            args.amount, args.token, subaccount
        )
    } else {
        format!(
            "Transfer {} {} from subaccount {}? [y/N] ",
            args.amount, args.token, subaccount
        )
    }
}

fn confirm_transfer(
    prompt: &str,
    abort_message: &'static str,
    format: OutputFormat,
) -> Result<(), CliError> {
    let mut stderr = io::stderr();
    let prompt = if format == OutputFormat::Pretty {
        output::colors::yellow(prompt)
    } else {
        prompt.to_string()
    };
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
        Err(CliError::Configuration(abort_message.to_string()))
    }
}

fn print_action(row: SubaccountActionConfirmation, format: OutputFormat, start: Instant) {
    output::print_data(
        &SubaccountActionOutput { rows: vec![row] },
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
    fn usdc_amount_converts_to_six_decimal_wire_units() {
        assert_eq!(usdc_to_wire_units(Decimal::from(10)).unwrap(), 10_000_000);
        assert_eq!(usdc_to_wire_units(Decimal::new(1, 6)).unwrap(), 1);
        assert_eq!(
            usdc_to_wire_units(Decimal::new(123_456_789, 6)).unwrap(),
            123_456_789
        );
    }

    #[test]
    fn usdc_amount_rejects_fractional_wire_units() {
        let err = usdc_to_wire_units(Decimal::new(1, 7)).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("at most 6 decimal places"));
    }

    #[test]
    fn subaccount_actions_declare_reversibility() {
        assert_eq!(
            SubaccountActionKind::Create.reversibility(),
            ActionReversibility::Irreversible
        );
        assert_eq!(
            SubaccountActionKind::TransferUsdc.reversibility(),
            ActionReversibility::PartiallyReversible
        );
        assert_eq!(
            SubaccountActionKind::TransferSpot.reversibility(),
            ActionReversibility::PartiallyReversible
        );
    }
}
