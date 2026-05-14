//! API/agent wallet approval and management commands.

use std::time::Instant;

use clap::Args;
use hypersdk::Address;
use hypersdk::hypercore::types::api::ApproveAgent;
use hypersdk::hypercore::types::{Action, OkResponse, Response};
use hypersdk::hypercore::{Chain, HttpClient, PrivateKeySigner};
use serde::Serialize;

use crate::auth;
use crate::commands::actions;
use crate::commands::map_api_error;
use crate::errors::CliError;
use crate::http_api::post_exchange_json;
use crate::output::{self, OutputFormat, TableData};
use crate::signing::SelectedSigner;

const MAX_AGENT_EXPIRATION_MS: u64 = 180 * 24 * 60 * 60 * 1000;
const REVOKE_REPLACEMENT_EXPIRATION_MS: u64 = 60 * 1000;
pub const MAX_AGENT_NAME_LEN: usize = 16;

/// Arguments for `api-wallet create`.
#[derive(Args, Debug, Clone)]
pub struct CreateArgs {
    /// Human-readable API wallet name. Named agents replace prior agents with the same name.
    #[arg(long)]
    pub name: Option<String>,

    /// Expire after a duration such as 30d, 12h, or 1w. Maximum is 180d.
    #[arg(long)]
    pub expires_in: Option<String>,

    /// Approve an existing agent address instead of generating a new key.
    #[arg(long, conflicts_with = "generate")]
    pub agent_address: Option<String>,

    /// Generate a new API wallet key before approval. This is the default for create.
    #[arg(long)]
    pub generate: bool,
}

/// Arguments for `api-wallet approve`.
#[derive(Args, Debug, Clone)]
pub struct ApproveArgs {
    /// Human-readable API wallet name. Named agents replace prior agents with the same name.
    #[arg(long)]
    pub name: Option<String>,

    /// Expire after a duration such as 30d, 12h, or 1w. Maximum is 180d.
    #[arg(long)]
    pub expires_in: Option<String>,

    /// Agent wallet address to approve.
    #[arg(long, conflicts_with = "generate")]
    pub agent_address: Option<String>,

    /// Generate a new API wallet key before approval.
    #[arg(long)]
    pub generate: bool,
}

/// Arguments for `api-wallet revoke`.
#[derive(Args, Debug, Clone)]
pub struct RevokeArgs {
    /// Named API wallet to revoke by replacing it with a short-lived throwaway agent.
    #[arg(long, required = true)]
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct PreparedAgentApproval {
    pub agent_address: Address,
    pub generated_private_key: Option<String>,
    pub display_name: Option<String>,
    pub action_agent_name: Option<String>,
    pub expires_at: Option<u64>,
    pub generated: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ApiWalletActionRow {
    status: String,
    action: String,
    master_address: Option<String>,
    agent_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    action_agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<u64>,
    generated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    private_key: Option<String>,
    note: String,
    approval_action: serde_json::Value,
}

struct ApiWalletActionOutput {
    row: ApiWalletActionRow,
}

impl TableData for ApiWalletActionOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Status",
            "Action",
            "Master",
            "Agent",
            "Name",
            "Expires At",
            "Generated",
            "Private Key",
            "Note",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.row.status.clone(),
            self.row.action.clone(),
            self.row
                .master_address
                .clone()
                .unwrap_or_else(|| "signer required for live approval".to_string()),
            self.row.agent_address.clone(),
            self.row
                .name
                .clone()
                .unwrap_or_else(|| "unnamed".to_string()),
            self.row
                .expires_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "protocol default".to_string()),
            self.row.generated.to_string(),
            self.row
                .private_key
                .clone()
                .unwrap_or_else(|| "not printed".to_string()),
            self.row.note.clone(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.row).unwrap_or_else(|_| serde_json::json!({}))
    }
}

#[derive(Debug, Clone, Serialize)]
struct ApiAgentRow {
    name: String,
    address: String,
    valid_until: Option<u64>,
}

struct ApiAgentListOutput {
    master_address: String,
    rows: Vec<ApiAgentRow>,
}

impl TableData for ApiAgentListOutput {
    fn headers(&self) -> Vec<&str> {
        if self.rows.is_empty() {
            vec!["Master", "Message"]
        } else {
            vec!["Master", "Name", "Address", "Valid Until"]
        }
    }

    fn rows(&self) -> Vec<Vec<String>> {
        if self.rows.is_empty() {
            return vec![vec![
                self.master_address.clone(),
                "no API wallets found".to_string(),
            ]];
        }
        self.rows
            .iter()
            .map(|row| {
                vec![
                    self.master_address.clone(),
                    row.name.clone(),
                    row.address.clone(),
                    row.valid_until
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "protocol default".to_string()),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "master_address": self.master_address,
            "api_wallets": self.rows,
        })
    }
}

pub async fn create(
    api_base_url: &str,
    chain: Chain,
    signer: Option<&SelectedSigner>,
    args: &CreateArgs,
    dry_run: bool,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let now = actions::nonce_now();
    let prepared = prepare_agent_approval(
        args.agent_address.as_deref(),
        true,
        args.generate,
        args.name.as_deref(),
        args.expires_in.as_deref(),
        now,
    )?;
    approve_prepared(
        ApprovalRuntime {
            api_base_url,
            chain,
            signer,
            dry_run,
            operation: "create",
            format,
        },
        prepared,
    )
    .await
}

pub async fn approve(
    api_base_url: &str,
    chain: Chain,
    signer: Option<&SelectedSigner>,
    args: &ApproveArgs,
    dry_run: bool,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let now = actions::nonce_now();
    let prepared = prepare_agent_approval(
        args.agent_address.as_deref(),
        false,
        args.generate,
        args.name.as_deref(),
        args.expires_in.as_deref(),
        now,
    )?;
    approve_prepared(
        ApprovalRuntime {
            api_base_url,
            chain,
            signer,
            dry_run,
            operation: "approve",
            format,
        },
        prepared,
    )
    .await
}

pub async fn list(
    client: &HttpClient,
    master_address: Address,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    let rows = client
        .api_agents(master_address)
        .await
        .map_err(map_api_error)?
        .into_iter()
        .map(|agent| ApiAgentRow {
            name: agent.name,
            address: agent.address.to_string(),
            valid_until: agent.valid_until,
        })
        .collect();
    output::print_data(
        &ApiAgentListOutput {
            master_address: master_address.to_string(),
            rows,
        },
        format,
        start.elapsed(),
    );
    Ok(())
}

pub async fn revoke(
    api_base_url: &str,
    chain: Chain,
    signer: Option<&SelectedSigner>,
    args: &RevokeArgs,
    dry_run: bool,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let now = actions::nonce_now();
    let prepared = prepare_revoke_replacement(args, now)?;

    approve_prepared(
        ApprovalRuntime {
            api_base_url,
            chain,
            signer,
            dry_run,
            operation: "revoke",
            format,
        },
        prepared,
    )
    .await
}

pub fn prepare_revoke_replacement(
    args: &RevokeArgs,
    now: u64,
) -> Result<PreparedAgentApproval, CliError> {
    let throwaway = PrivateKeySigner::random();
    let name = validated_agent_name(Some(args.name.as_str()), "--name")?.ok_or_else(|| {
        CliError::Configuration("--name is required for api-wallet revoke".to_string())
    })?;
    let expires_at = now
        .checked_add(REVOKE_REPLACEMENT_EXPIRATION_MS)
        .ok_or_else(|| CliError::Configuration("revoke expiration overflowed".to_string()))?;
    Ok(PreparedAgentApproval {
        agent_address: throwaway.address(),
        generated_private_key: None,
        display_name: Some(name.clone()),
        action_agent_name: agent_action_name(Some(name.as_str()), Some(expires_at))?,
        expires_at: Some(expires_at),
        generated: true,
    })
}

#[derive(Debug, Clone, Copy)]
struct ApprovalRuntime<'a> {
    api_base_url: &'a str,
    chain: Chain,
    signer: Option<&'a SelectedSigner>,
    dry_run: bool,
    operation: &'static str,
    format: OutputFormat,
}

async fn approve_prepared(
    runtime: ApprovalRuntime<'_>,
    prepared: PreparedAgentApproval,
) -> Result<(), anyhow::Error> {
    let nonce = actions::nonce_now();
    let approval = approval_action(runtime.chain, &prepared, nonce);
    let approval_value =
        serde_json::to_value(Action::ApproveAgent(approval.clone())).unwrap_or_default();

    if runtime.dry_run {
        let row = approval_dry_run_row(
            runtime.operation,
            runtime.signer.map(SelectedSigner::address),
            &prepared,
            approval_value,
        );
        print_action_output(row, runtime.format);
        return Ok(());
    }

    ensure_generated_key_projection_is_safe(&prepared, runtime.format)?;

    let signer = runtime.signer.ok_or(CliError::AuthRequired)?;
    let master_address = signer.address();
    let request =
        signer.sign_l1_action_sync(Action::ApproveAgent(approval), nonce, None, runtime.chain)?;
    let response = post_exchange_json::<Response>(runtime.api_base_url, &request, "").await?;
    ensure_default_response(response)?;

    let private_key = prepared.generated_private_key;
    print_action_output(
        ApiWalletActionRow {
            status: "submitted".to_string(),
            action: format!("api_wallet_{}", runtime.operation),
            master_address: Some(master_address.to_string()),
            agent_address: prepared.agent_address.to_string(),
            name: prepared.display_name,
            action_agent_name: prepared.action_agent_name,
            expires_at: prepared.expires_at,
            generated: prepared.generated,
            private_key,
            note: if runtime.operation == "revoke" {
                "Named API wallet replaced with a short-lived throwaway agent; no private key was retained."
                    .to_string()
            } else {
                "Store this private key now; it will not be shown again by this command."
                    .to_string()
            },
            approval_action: approval_value,
        },
        runtime.format,
    );
    Ok(())
}

pub fn approval_dry_run_value(
    chain: Chain,
    operation: &'static str,
    master_address: Option<Address>,
    prepared: &PreparedAgentApproval,
    nonce: u64,
) -> serde_json::Value {
    let approval_value = serde_json::to_value(Action::ApproveAgent(approval_action(
        chain, prepared, nonce,
    )))
    .unwrap_or_default();
    serde_json::to_value(approval_dry_run_row(
        operation,
        master_address,
        prepared,
        approval_value,
    ))
    .unwrap_or_else(|_| serde_json::json!({}))
}

fn approval_dry_run_row(
    operation: &'static str,
    master_address: Option<Address>,
    prepared: &PreparedAgentApproval,
    approval_value: serde_json::Value,
) -> ApiWalletActionRow {
    ApiWalletActionRow {
        status: "dry_run".to_string(),
        action: format!("api_wallet_{operation}"),
        master_address: master_address.map(|address| address.to_string()),
        agent_address: prepared.agent_address.to_string(),
        name: prepared.display_name.clone(),
        action_agent_name: prepared.action_agent_name.clone(),
        expires_at: prepared.expires_at,
        generated: prepared.generated,
        private_key: None,
        note: "Dry-run only; approveAgent action was not signed or submitted, and no private key was printed.".to_string(),
        approval_action: approval_value,
    }
}

fn ensure_generated_key_projection_is_safe(
    prepared: &PreparedAgentApproval,
    format: OutputFormat,
) -> Result<(), CliError> {
    if format == OutputFormat::Json
        && prepared.generated_private_key.is_some()
        && output::json_projection_options_enabled()
    {
        return Err(CliError::Unsupported(
            "live generated API wallet approvals print the private key exactly once; remove --select, --max-results, and --results-only"
                .to_string(),
        ));
    }
    Ok(())
}

pub fn prepare_agent_approval(
    agent_address: Option<&str>,
    generate_by_default: bool,
    generate: bool,
    name: Option<&str>,
    expires_in: Option<&str>,
    now: u64,
) -> Result<PreparedAgentApproval, CliError> {
    let display_name = validated_agent_name(name, "--name")?;
    let expires_at = expires_in
        .map(parse_expires_in)
        .transpose()?
        .map(|duration| {
            now.checked_add(duration).ok_or_else(|| {
                CliError::Configuration("agent wallet expiration overflowed".to_string())
            })
        })
        .transpose()?;
    let action_agent_name = agent_action_name(name, expires_at)?;

    if let Some(agent_address) = agent_address {
        let address = parse_address(agent_address)?;
        return Ok(PreparedAgentApproval {
            agent_address: address,
            generated_private_key: None,
            display_name: display_name.clone(),
            action_agent_name,
            expires_at,
            generated: false,
        });
    }

    if generate || generate_by_default {
        let signer = PrivateKeySigner::random();
        let private_key = auth::signer_private_key_hex(&signer);
        return Ok(PreparedAgentApproval {
            agent_address: signer.address(),
            generated_private_key: Some(private_key),
            display_name,
            action_agent_name,
            expires_at,
            generated: true,
        });
    }

    Err(CliError::Unsupported(
        "api-wallet approve requires --agent-address or --generate".to_string(),
    ))
}

pub fn parse_expires_in(raw: &str) -> Result<u64, CliError> {
    let raw = raw.trim();
    if raw.len() < 2 {
        return Err(CliError::Configuration(
            "--expires-in must use a duration like 30d, 12h, or 1w".to_string(),
        ));
    }
    let (amount, unit) = raw.split_at(raw.len() - 1);
    let amount = amount.parse::<u64>().map_err(|_| {
        CliError::Configuration("--expires-in amount must be a positive integer".to_string())
    })?;
    if amount == 0 {
        return Err(CliError::Configuration(
            "--expires-in amount must be greater than zero".to_string(),
        ));
    }
    let unit_ms = match unit {
        "m" => 60 * 1000,
        "h" => 60 * 60 * 1000,
        "d" => 24 * 60 * 60 * 1000,
        "w" => 7 * 24 * 60 * 60 * 1000,
        _ => {
            return Err(CliError::Configuration(
                "--expires-in unit must be one of m, h, d, or w".to_string(),
            ));
        }
    };
    let duration = amount
        .checked_mul(unit_ms)
        .ok_or_else(|| CliError::Configuration("--expires-in duration overflowed".to_string()))?;
    if duration > MAX_AGENT_EXPIRATION_MS {
        return Err(CliError::Configuration(
            "--expires-in cannot exceed 180d".to_string(),
        ));
    }
    Ok(duration)
}

fn approval_action(chain: Chain, prepared: &PreparedAgentApproval, nonce: u64) -> ApproveAgent {
    ApproveAgent {
        signature_chain_id: chain.arbitrum_id().to_owned(),
        hyperliquid_chain: chain,
        agent_address: prepared.agent_address,
        agent_name: prepared.action_agent_name.clone(),
        nonce,
    }
}

fn agent_action_name(
    name: Option<&str>,
    expires_at: Option<u64>,
) -> Result<Option<String>, CliError> {
    let name = validated_agent_name(name, "--name")?;
    match (name, expires_at) {
        (Some(name), Some(expires_at)) => Ok(Some(format!("{name} valid_until {expires_at}"))),
        (Some(name), None) => Ok(Some(name)),
        (None, Some(_)) => Err(CliError::Configuration(
            "--expires-in requires --name because Hyperliquid encodes agent expiration in agentName"
                .to_string(),
        )),
        (None, None) => Ok(None),
    }
}

fn validated_agent_name(
    name: Option<&str>,
    flag: &'static str,
) -> Result<Option<String>, CliError> {
    let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok(None);
    };
    if name.chars().count() > MAX_AGENT_NAME_LEN {
        return Err(CliError::Configuration(format!(
            "{flag} must be between 1 and {MAX_AGENT_NAME_LEN} characters"
        )));
    }
    Ok(Some(name.to_string()))
}

fn parse_address(raw: &str) -> Result<Address, CliError> {
    raw.trim()
        .parse::<Address>()
        .map_err(|_| CliError::Unsupported(format!("Invalid address: {}", raw.trim())))
}

fn ensure_default_response(response: Response) -> Result<(), CliError> {
    match response {
        Response::Ok(OkResponse::Default) => Ok(()),
        Response::Err(err) => Err(actions::map_exchange_error(
            err,
            "approveAgent action failed",
        )),
        other => Err(CliError::Internal(anyhow::anyhow!(
            "approveAgent action failed: unexpected response type: {other:?}"
        ))),
    }
}

fn print_action_output(row: ApiWalletActionRow, format: OutputFormat) {
    output::print_data_no_timing(&ApiWalletActionOutput { row }, format);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_agent_expiration_durations() {
        assert_eq!(parse_expires_in("30d").unwrap(), 30 * 24 * 60 * 60 * 1000);
        assert_eq!(parse_expires_in("12h").unwrap(), 12 * 60 * 60 * 1000);
        assert_eq!(parse_expires_in("1w").unwrap(), 7 * 24 * 60 * 60 * 1000);
        assert!(parse_expires_in("0d").is_err());
        assert!(parse_expires_in("181d").is_err());
        assert!(parse_expires_in("5x").is_err());
    }

    #[test]
    fn expiration_is_encoded_in_agent_name() {
        let prepared = prepare_agent_approval(
            Some("0x0000000000000000000000000000000000000001"),
            false,
            false,
            Some("bot"),
            Some("1d"),
            1_000,
        )
        .unwrap();
        assert_eq!(prepared.display_name.as_deref(), Some("bot"));
        assert_eq!(
            prepared.action_agent_name.as_deref(),
            Some("bot valid_until 86401000")
        );
        assert_eq!(prepared.expires_at, Some(86_401_000));
    }

    #[test]
    fn expiration_requires_a_name() {
        let err = prepare_agent_approval(
            Some("0x0000000000000000000000000000000000000001"),
            false,
            false,
            None,
            Some("1d"),
            1_000,
        )
        .unwrap_err();
        assert!(err.to_string().contains("--expires-in requires --name"));
    }

    #[test]
    fn dry_run_output_does_not_expose_generated_private_key() {
        let row = ApiWalletActionRow {
            status: "dry_run".to_string(),
            action: "api_wallet_create".to_string(),
            master_address: None,
            agent_address: "0x0000000000000000000000000000000000000001".to_string(),
            name: Some("bot".to_string()),
            action_agent_name: Some("bot".to_string()),
            expires_at: None,
            generated: true,
            private_key: None,
            note: "safe".to_string(),
            approval_action: serde_json::json!({"type": "approveAgent"}),
        };
        let rendered = output::render(&ApiWalletActionOutput { row }, OutputFormat::Json);
        assert!(!rendered.contains("private_key"));
        assert!(
            !rendered.contains("000000000000000000000000000000000000000000000000000000000000000")
        );
    }
}
