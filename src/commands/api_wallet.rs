//! API/agent wallet approval and management commands.

use clap::Args;
use hypersdk::Address;
use hypersdk::hypercore::types::api::ApproveAgent;
use hypersdk::hypercore::types::{Action, Response};
use hypersdk::hypercore::{Chain, PrivateKeySigner};
use serde::Serialize;

use crate::auth;
use crate::command_spec::{CliCommand, CommandSpec, GroupSpec, InputKind, OwsSupport, Risk, arg};
use crate::commands::actions;
use crate::errors::CliError;
use crate::errors::from_sdk;
use crate::invocation::Invocation;
use crate::output::{OutputFormat, TableData};
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

#[derive(Clone)]
pub struct PreparedAgentApproval {
    pub agent_address: Address,
    pub generated_private_key: Option<String>,
    pub display_name: Option<String>,
    pub action_agent_name: Option<String>,
    pub expires_at: Option<u64>,
    pub generated: bool,
}

#[derive(Clone, Serialize)]
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

impl std::fmt::Debug for PreparedAgentApproval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedAgentApproval")
            .field("agent_address", &self.agent_address)
            .field(
                "generated_private_key",
                &self.generated_private_key.as_ref().map(|_| "[redacted]"),
            )
            .field("display_name", &self.display_name)
            .field("action_agent_name", &self.action_agent_name)
            .field("expires_at", &self.expires_at)
            .field("generated", &self.generated)
            .finish()
    }
}

impl std::fmt::Debug for ApiWalletActionRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiWalletActionRow")
            .field("status", &self.status)
            .field("action", &self.action)
            .field("master_address", &self.master_address)
            .field("agent_address", &self.agent_address)
            .field("name", &self.name)
            .field("action_agent_name", &self.action_agent_name)
            .field("expires_at", &self.expires_at)
            .field("generated", &self.generated)
            .field(
                "private_key",
                &self.private_key.as_ref().map(|_| "[redacted]"),
            )
            .field("note", &self.note)
            .field("approval_action", &self.approval_action)
            .finish()
    }
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
        vec!["Master", "Name", "Address", "Valid Until"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
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

    fn empty_message(&self) -> Option<&str> {
        Some("No API wallets.")
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::json!({
            "master_address": self.master_address,
            "api_wallets": self.rows,
        })
    }
}

pub async fn create(
    inv: &Invocation,
    signer: Option<&SelectedSigner>,
    args: &CreateArgs,
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
            inv,
            signer,
            operation: "create",
        },
        prepared,
    )
    .await
}

pub async fn approve(
    inv: &Invocation,
    signer: Option<&SelectedSigner>,
    args: &ApproveArgs,
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
            inv,
            signer,
            operation: "approve",
        },
        prepared,
    )
    .await
}

pub async fn list(inv: &Invocation, master_address: Address) -> Result<(), anyhow::Error> {
    let rows = inv
        .remote()?
        .api()
        .sdk()
        .api_agents(master_address)
        .await
        .map_err(from_sdk)?
        .into_iter()
        .map(|agent| ApiAgentRow {
            name: agent.name,
            address: agent.address.to_string(),
            valid_until: agent.valid_until,
        })
        .collect();
    inv.print(&ApiAgentListOutput {
        master_address: master_address.to_string(),
        rows,
    });
    Ok(())
}

pub async fn revoke(
    inv: &Invocation,
    signer: Option<&SelectedSigner>,
    args: &RevokeArgs,
) -> Result<(), anyhow::Error> {
    let now = actions::nonce_now();
    let prepared = prepare_revoke_replacement(args, now)?;

    approve_prepared(
        ApprovalRuntime {
            inv,
            signer,
            operation: "revoke",
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
    inv: &'a Invocation,
    signer: Option<&'a SelectedSigner>,
    operation: &'static str,
}

async fn approve_prepared(
    runtime: ApprovalRuntime<'_>,
    prepared: PreparedAgentApproval,
) -> Result<(), anyhow::Error> {
    let inv = runtime.inv;
    let remote = inv.remote()?;
    let chain = remote.chain();
    let nonce = actions::nonce_now();
    let approval = approval_action(chain, &prepared, nonce);
    let approval_value =
        serde_json::to_value(Action::ApproveAgent(approval.clone())).unwrap_or_default();

    if inv.dry_run() {
        let row = approval_dry_run_row(
            runtime.operation,
            runtime.signer.map(SelectedSigner::address),
            &prepared,
            approval_value,
        );
        print_action_output(inv, row);
        return Ok(());
    }

    ensure_generated_key_projection_is_safe(&prepared, inv)?;

    let signer = runtime.signer.ok_or(CliError::AuthRequired)?;
    let master_address = signer.address();
    let request = signer.sign_l1_action_sync(Action::ApproveAgent(approval), nonce, None, chain)?;
    let response = remote.api().exchange::<Response>(&request).await?;
    actions::ensure_default_response(response, "approveAgent action failed")?;

    let private_key = prepared.generated_private_key;
    print_action_output(
        inv,
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
    inv: &Invocation,
) -> Result<(), CliError> {
    if inv.format() == OutputFormat::Json
        && prepared.generated_private_key.is_some()
        && inv.session().json_options().is_active()
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

fn print_action_output(inv: &Invocation, row: ApiWalletActionRow) {
    inv.print_untimed(&ApiWalletActionOutput { row });
}

pub const GROUP: GroupSpec = GroupSpec::new(&["api-wallet"], "Approve and manage API wallets")
    .long_about("Approve and manage API wallets.\n\
\n\
API wallets, also called agent wallets, can sign trading actions on behalf of a master account, but cannot withdraw. Account data queries must use the master or subaccount address, not the API wallet address.")
    .aliases(&["api-wallets"])
    .schema_aliases();

#[derive(Args, Debug, Clone)]
pub struct ApiWalletListArgs {
    /// Master address, wallet name, or wallet id. Defaults to the selected signer.
    pub account: Option<String>,
}

pub struct ApiWalletCreate;

impl CliCommand for ApiWalletCreate {
    type Args = CreateArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["api-wallet", "create"],
        "wallet",
        "Generate or approve an API wallet",
        "Generate or approve a Hyperliquid API/agent wallet. When the CLI generates a local agent keypair, it prints the private key exactly once before approval.",
    )
    .long_about("Generate or approve an API wallet.\n\
\n\
By default this command generates a new local API wallet keypair, signs an approveAgent action for that address with the selected master account, and prints the generated private key exactly once unless command is running in dry-run.")
    .args(&[
        arg("name").describe("Optional API wallet name."),
        arg("expires_in").kind(InputKind::Duration).describe("Expiration duration such as 30d, 12h, or 1w. Requires name."),
        arg("agent_address"),
        arg("generate").kind(InputKind::Boolean).describe("Generate a new API wallet key before approval."),
    ]);

    fn validate(args: &CreateArgs) -> Result<(), CliError> {
        validate_api_wallet_args(args.name.as_deref(), args.agent_address.as_deref())
    }

    async fn run(inv: &Invocation, args: &CreateArgs) -> Result<(), anyhow::Error> {
        let signer = api_wallet_approval_signer(inv)?;
        create(inv, signer.as_ref(), args).await
    }
}

pub struct ApiWalletApprove;

impl CliCommand for ApiWalletApprove {
    type Args = ApproveArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["api-wallet", "approve"],
        "wallet",
        "Approve an existing or generated API wallet address",
        "Approve an existing or generated API/agent wallet address for the selected master account.",
    )
    .args(&[
        arg("name").describe("Optional API wallet name."),
        arg("expires_in").kind(InputKind::Duration).describe("Expiration duration such as 30d, 12h, or 1w. Requires name."),
        arg("agent_address"),
        arg("generate").kind(InputKind::Boolean),
    ]);

    fn validate(args: &ApproveArgs) -> Result<(), CliError> {
        validate_api_wallet_args(args.name.as_deref(), args.agent_address.as_deref())
    }

    async fn run(inv: &Invocation, args: &ApproveArgs) -> Result<(), anyhow::Error> {
        let signer = api_wallet_approval_signer(inv)?;
        approve(inv, signer.as_ref(), args).await
    }
}

pub struct ApiWalletList;

impl CliCommand for ApiWalletList {
    type Args = ApiWalletListArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["api-wallet", "list"],
        "wallet",
        "List API wallets approved by a master account",
        "List API wallets approved by a master account. Stored agent accounts query under their master address.",
    )
    .risk(Risk::AccountState)
    .ows(OwsSupport::ExperimentalFeatureGated)
    .auth(true)
    .cli_aliases(&["ls"])
    .schema_command("hyperliquid api-wallet list")
    .args(&[
        arg("account").kind(InputKind::SignerSelector).resource_id("account selector"),
    ]);

    async fn run(inv: &Invocation, args: &ApiWalletListArgs) -> Result<(), anyhow::Error> {
        let master_address = match args.account.as_deref() {
            Some(account) => inv.resolve_protocol_user(account)?,
            None => inv.resolve_signer()?.query_address(),
        };
        list(inv, master_address).await
    }
}

pub struct ApiWalletRevoke;

impl CliCommand for ApiWalletRevoke {
    type Args = RevokeArgs;

    const SPEC: CommandSpec = CommandSpec::signed(
        &["api-wallet", "revoke"],
        "wallet",
        "Revoke an API wallet by replacing it with a short-lived throwaway agent",
        "Revoke a named API wallet by replacing it with a short-lived throwaway agent.",
    )
    .args(&[arg("name")
        .describe("Named API wallet to revoke.")
        .resource_id("API wallet name")]);

    async fn run(inv: &Invocation, args: &RevokeArgs) -> Result<(), anyhow::Error> {
        if args.name.chars().count() > MAX_AGENT_NAME_LEN {
            return Err(CliError::Configuration(format!(
                "--name must be between 1 and {} characters",
                MAX_AGENT_NAME_LEN
            ))
            .into());
        }
        let signer = api_wallet_approval_signer(inv)?;
        revoke(inv, signer.as_ref(), args).await
    }
}

fn api_wallet_approval_signer(
    inv: &Invocation,
) -> Result<Option<crate::signing::SelectedSigner>, anyhow::Error> {
    if inv.dry_run() {
        return Ok(None);
    }
    let resolved = inv.resolve_signer()?;
    if resolved.query_address() != resolved.address() {
        return Err(CliError::Unsupported(
            "approveAgent must be signed by the master account; select the master wallet, not a stored API wallet"
                .to_string(),
        )
        .into());
    }
    Ok(Some(resolved.selected_signer()))
}

fn validate_api_wallet_args(
    name: Option<&str>,
    agent_address: Option<&str>,
) -> Result<(), CliError> {
    use crate::input_hardening::validate_resource_id;
    use MAX_AGENT_NAME_LEN;

    if let Some(name) = name {
        validate_resource_id("API wallet name", name)?;
        let trimmed = name.trim();
        if !trimmed.is_empty() && trimmed.chars().count() > MAX_AGENT_NAME_LEN {
            return Err(CliError::Configuration(format!(
                "--name must be between 1 and {MAX_AGENT_NAME_LEN} characters"
            )));
        }
    }
    if let Some(agent_address) = agent_address {
        validate_resource_id("agent address", agent_address)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Session;

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
        let rendered = Session::for_test(OutputFormat::Json).render(&ApiWalletActionOutput { row });
        assert!(!rendered.contains("private_key"));
        assert!(
            !rendered.contains("000000000000000000000000000000000000000000000000000000000000000")
        );
    }
}
