use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::command_metadata::{
    CatalogArgMetadata, CatalogCommandMetadata, command_metadata, normalize_arg,
};

#[derive(Debug, Clone, Serialize)]
pub struct CommandRegistry {
    commands: Vec<CommandContract>,
}

impl CommandRegistry {
    pub fn load() -> Result<Self, anyhow::Error> {
        let catalog: Catalog = serde_json::from_str(include_str!("command_catalog.json"))?;
        let commands = catalog
            .commands
            .into_iter()
            .map(CommandContract::from_catalog)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { commands })
    }

    pub fn commands(&self) -> &[CommandContract] {
        &self.commands
    }

    pub fn find_path(&self, path: &[&str]) -> Option<&CommandContract> {
        self.commands.iter().find(|command| {
            command
                .command_path
                .iter()
                .map(String::as_str)
                .eq(path.iter().copied())
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandContract {
    pub command: String,
    pub command_path: Vec<String>,
    pub aliases: Vec<String>,
    pub group: String,
    pub description: String,
    pub auth_required: bool,
    pub dangerous: bool,
    pub lifecycle: Lifecycle,
    pub risk: Risk,
    pub mutability: Mutability,
    pub dry_run: DryRunPolicy,
    pub raw_payload: RawPayloadPolicy,
    pub confirmation: ConfirmationPolicy,
    pub transport: Vec<Transport>,
    pub ows_signer: OwsSupport,
    pub output_contract: OutputContract,
    pub one_of_required: Vec<Vec<String>>,
    pub inputs: Vec<InputContract>,
}

impl CommandContract {
    fn from_catalog(command: CatalogCommand) -> Result<Self, anyhow::Error> {
        let command_path = command_parts(&command.command)?;
        let command_key = command_path.join(" ");
        let metadata = command_metadata(&command, &command_key);
        let args = command
            .args
            .into_iter()
            .map(normalize_arg)
            .collect::<Vec<_>>();
        let lifecycle = Lifecycle::from(metadata.lifecycle.as_str());
        let risk = Risk::from(metadata.risk.as_str());
        let dry_run = DryRunPolicy::from(metadata.dry_run.as_str());
        let raw_payload = RawPayloadPolicy::from(metadata.raw_payload.as_str());
        let confirmation = ConfirmationPolicy::from(metadata.confirmation.as_str());
        let ows_signer = OwsSupport::from(metadata.ows_signer.as_str());
        let transport = primary_transport(lifecycle);
        Ok(Self {
            command: command.command,
            command_path,
            aliases: metadata.aliases,
            group: command.group,
            description: command.description,
            auth_required: command.auth_required,
            dangerous: command.dangerous,
            lifecycle,
            risk,
            mutability: Mutability::from_lifecycle_and_risk(lifecycle, risk),
            dry_run,
            raw_payload,
            confirmation,
            transport,
            ows_signer,
            output_contract: output_contract(&command_key, lifecycle, dry_run),
            one_of_required: command.one_of_required,
            inputs: args.into_iter().map(InputContract::from_catalog).collect(),
        })
    }

    pub fn command_key(&self) -> String {
        self.command_path.join(" ")
    }

    /// Name of the boolean flag that answers this command's confirmation prompt,
    /// derived from the catalog args: `Some("yes")` for every command with a `yes` arg.
    pub fn confirmation_bypass_arg(&self) -> Option<&'static str> {
        self.inputs
            .iter()
            .any(|input| input.id == "yes")
            .then_some("yes")
    }

    /// Catalog input for an arg id.
    pub fn input(&self, id: &str) -> Option<&InputContract> {
        self.inputs.iter().find(|input| input.id == id)
    }

    /// Catalog description for an arg id, when it has one.
    pub fn arg_description(&self, id: &str) -> Option<&str> {
        self.input(id)
            .and_then(|input| input.description.as_deref())
    }

    pub fn metadata_json(&self) -> Value {
        let yes_support = self.confirmation_bypass_arg();
        let has_snapshot_watch = self.inputs.iter().any(|input| input.id == "watch")
            && self.inputs.iter().any(|input| input.id == "max_ticks");
        let stream_bounds = if self.output_contract == OutputContract::BoundedNdjsonStream {
            json!({
                "required_in_machine_context": true,
                "cli_args": ["max_events", "idle_timeout_ms"],
                "env": ["HYPERLIQUID_SUBSCRIBE_MAX_EVENTS"],
                "output_mode": "ndjson"
            })
        } else if has_snapshot_watch {
            json!({
                "required_in_machine_context": true,
                "required_when": {"watch": true},
                "cli_args": ["max_ticks"],
                "env": ["HYPERLIQUID_WATCH_MAX_TICKS"],
                "output_mode": "ndjson"
            })
        } else {
            Value::Null
        };
        json!({
            "aliases": self.aliases,
            "lifecycle": self.lifecycle,
            "risk": self.risk,
            "mutability": self.mutability,
            "dry_run": self.dry_run,
            "raw_payload": self.raw_payload,
            "confirmation": self.confirmation,
            "confirmation_bypass": {
                "supported": yes_support.is_some(),
                "arg": yes_support,
            },
            "ows_signer": self.ows_signer,
            "transport": self.transport,
            "output_contract": self.output_contract,
            "stream_bounds": stream_bounds,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InputContract {
    pub id: String,
    pub long: Option<String>,
    pub short: Option<String>,
    pub positional_index: Option<usize>,
    pub arg_type: String,
    pub required: bool,
    pub multiple: bool,
    pub enum_values: Vec<String>,
    pub description: Option<String>,
    pub default: Option<String>,
    pub kind: Option<InputKind>,
}

impl InputContract {
    fn from_catalog(arg: CatalogArg) -> Self {
        Self {
            kind: arg.input_kind.as_deref().map(InputKind::from),
            id: arg.id,
            long: arg.long,
            short: arg.short,
            positional_index: arg.positional_index,
            arg_type: arg.arg_type,
            required: arg.required,
            multiple: arg.multiple,
            enum_values: arg.enum_values,
            description: arg.description,
            default: arg.default,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    ReadOnly,
    Streaming,
    InteractiveLocal,
    LiveMutating,
    BlockedUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    None,
    AccountState,
    FundsMovement,
    LocalSecret,
    LocalState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mutability {
    ReadOnly,
    Streaming,
    LocalOnly,
    LiveMutating,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DryRunPolicy {
    NotSupported,
    Optional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RawPayloadPolicy {
    Unsupported,
    DryRunOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationPolicy {
    None,
    Prompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    CliProcess,
    CliInteractive,
    CliBlocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwsSupport {
    NotRequired,
    AddressSelectorSupported,
    ExperimentalFeatureGated,
    LocalOnly,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputContract {
    JsonValue,
    SchemaArrayOrObject,
    BoundedNdjsonStream,
    CommandResultOrDryRunEnvelope,
    InteractiveLocal,
    BlockedUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    SignerSelector,
    ActingAccountSelector,
    PublicUserSelector,
    RawDestinationAddress,
    ProtocolObjectAddress,
    MarketSymbol,
    VenueSelector,
    Boolean,
    Price,
    Amount,
    FilePath,
    Duration,
    JsonObject,
    Url,
    Identifier,
}

impl InputKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SignerSelector => "signer_selector",
            Self::ActingAccountSelector => "acting_account_selector",
            Self::PublicUserSelector => "public_user_selector",
            Self::RawDestinationAddress => "raw_destination_address",
            Self::ProtocolObjectAddress => "protocol_object_address",
            Self::MarketSymbol => "market_symbol",
            Self::VenueSelector => "venue_selector",
            Self::Boolean => "boolean",
            Self::Price => "price",
            Self::Amount => "amount",
            Self::FilePath => "file_path",
            Self::Duration => "duration",
            Self::JsonObject => "json_object",
            Self::Url => "url",
            Self::Identifier => "identifier",
        }
    }
}

impl From<&str> for Lifecycle {
    fn from(value: &str) -> Self {
        match value {
            "streaming" => Self::Streaming,
            "interactive_local" => Self::InteractiveLocal,
            "live_mutating" => Self::LiveMutating,
            "blocked_unsupported" => Self::BlockedUnsupported,
            _ => Self::ReadOnly,
        }
    }
}

impl From<&str> for Risk {
    fn from(value: &str) -> Self {
        match value {
            "account_state" => Self::AccountState,
            "funds_movement" => Self::FundsMovement,
            "local_secret" => Self::LocalSecret,
            "local_state" => Self::LocalState,
            _ => Self::None,
        }
    }
}

impl Mutability {
    fn from_lifecycle_and_risk(lifecycle: Lifecycle, risk: Risk) -> Self {
        match lifecycle {
            Lifecycle::Streaming => Self::Streaming,
            Lifecycle::InteractiveLocal => Self::LocalOnly,
            Lifecycle::LiveMutating => Self::LiveMutating,
            Lifecycle::BlockedUnsupported => Self::Blocked,
            Lifecycle::ReadOnly if matches!(risk, Risk::LocalSecret | Risk::LocalState) => {
                Self::LocalOnly
            }
            Lifecycle::ReadOnly => Self::ReadOnly,
        }
    }
}

impl From<&str> for DryRunPolicy {
    fn from(value: &str) -> Self {
        match value {
            "optional" => Self::Optional,
            _ => Self::NotSupported,
        }
    }
}

impl From<&str> for RawPayloadPolicy {
    fn from(value: &str) -> Self {
        match value {
            "dry_run_only" => Self::DryRunOnly,
            _ => Self::Unsupported,
        }
    }
}

impl From<&str> for ConfirmationPolicy {
    fn from(value: &str) -> Self {
        match value {
            "prompt" => Self::Prompt,
            _ => Self::None,
        }
    }
}

impl From<&str> for OwsSupport {
    fn from(value: &str) -> Self {
        match value {
            "address_selector_supported" => Self::AddressSelectorSupported,
            "experimental_feature_gated" => Self::ExperimentalFeatureGated,
            "local_only" => Self::LocalOnly,
            "not_applicable" => Self::NotApplicable,
            _ => Self::NotRequired,
        }
    }
}

impl From<&str> for InputKind {
    fn from(value: &str) -> Self {
        match value {
            "signer_selector" => Self::SignerSelector,
            "acting_account_selector" => Self::ActingAccountSelector,
            "public_user_selector" => Self::PublicUserSelector,
            "raw_destination_address" => Self::RawDestinationAddress,
            "protocol_object_address" => Self::ProtocolObjectAddress,
            "market_symbol" => Self::MarketSymbol,
            "venue_selector" => Self::VenueSelector,
            "boolean" => Self::Boolean,
            "price" => Self::Price,
            "amount" => Self::Amount,
            "file_path" => Self::FilePath,
            "duration" => Self::Duration,
            "json_object" => Self::JsonObject,
            "url" => Self::Url,
            _ => Self::Identifier,
        }
    }
}

fn command_parts(command: &str) -> Result<Vec<String>, anyhow::Error> {
    let parts = command
        .split_whitespace()
        .filter(|part| *part != "hyperliquid")
        .filter(|part| !part.starts_with('-'))
        .filter(|part| !part.starts_with('<') && !part.ends_with('>'))
        .map(str::to_string)
        .collect::<Vec<_>>();
    if parts.is_empty() {
        anyhow::bail!("catalog command '{command}' did not contain a command path");
    }
    Ok(parts)
}

fn primary_transport(lifecycle: Lifecycle) -> Vec<Transport> {
    match lifecycle {
        Lifecycle::InteractiveLocal => vec![Transport::CliInteractive],
        Lifecycle::BlockedUnsupported => vec![Transport::CliBlocked],
        _ => vec![Transport::CliProcess],
    }
}

fn output_contract(
    command_key: &str,
    lifecycle: Lifecycle,
    dry_run: DryRunPolicy,
) -> OutputContract {
    if command_key.starts_with("subscribe ") {
        OutputContract::BoundedNdjsonStream
    } else if command_key == "schema" {
        OutputContract::SchemaArrayOrObject
    } else if lifecycle == Lifecycle::InteractiveLocal {
        OutputContract::InteractiveLocal
    } else if lifecycle == Lifecycle::BlockedUnsupported {
        OutputContract::BlockedUnsupported
    } else if dry_run == DryRunPolicy::Optional {
        OutputContract::CommandResultOrDryRunEnvelope
    } else {
        OutputContract::JsonValue
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Catalog {
    commands: Vec<CatalogCommand>,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogCommand {
    command: String,
    group: String,
    #[serde(default)]
    auth_required: bool,
    #[serde(default)]
    dangerous: bool,
    #[serde(default)]
    aliases: Vec<String>,
    lifecycle: String,
    risk: String,
    dry_run: String,
    raw_payload: String,
    confirmation: String,
    ows_signer: String,
    description: String,
    #[serde(default)]
    one_of_required: Vec<Vec<String>>,
    #[serde(default)]
    args: Vec<CatalogArg>,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogArg {
    id: String,
    #[serde(default)]
    long: Option<String>,
    #[serde(default)]
    short: Option<String>,
    #[serde(default)]
    positional_index: Option<usize>,
    #[serde(default = "default_string_type")]
    arg_type: String,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    multiple: bool,
    #[serde(default)]
    enum_values: Vec<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    default: Option<String>,
    #[serde(default)]
    input_kind: Option<String>,
}

fn default_string_type() -> String {
    "string".to_string()
}

impl CatalogCommandMetadata for CatalogCommand {
    fn aliases(&self) -> &[String] {
        &self.aliases
    }

    fn lifecycle(&self) -> &str {
        &self.lifecycle
    }

    fn risk(&self) -> &str {
        &self.risk
    }

    fn dry_run(&self) -> &str {
        &self.dry_run
    }

    fn raw_payload(&self) -> &str {
        &self.raw_payload
    }

    fn confirmation(&self) -> &str {
        &self.confirmation
    }

    fn ows_signer(&self) -> &str {
        &self.ows_signer
    }
}

impl CatalogArgMetadata for CatalogArg {
    fn id(&self) -> &str {
        &self.id
    }

    fn long(&self) -> Option<&str> {
        self.long.as_deref()
    }

    fn arg_type(&self) -> &str {
        &self.arg_type
    }

    fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    fn input_kind(&self) -> Option<&str> {
        self.input_kind.as_deref()
    }

    fn set_input_kind(&mut self, input_kind: String) {
        self.input_kind = Some(input_kind);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn registry_enumerates_current_catalog_inventory() {
        let registry = CommandRegistry::load().unwrap();
        assert_eq!(registry.commands().len(), 121);
        assert!(registry.find_path(&["asset", "decode"]).is_some());
        assert!(registry.find_path(&["asset", "search"]).is_some());
        assert!(registry.find_path(&["search"]).is_some());
        assert!(registry.find_path(&["buy"]).is_some());
        assert!(registry.find_path(&["orders", "chase"]).is_some());
        assert!(registry.find_path(&["orders", "bracket"]).is_some());
        assert!(registry.find_path(&["orders", "trailing-stop"]).is_some());
        assert!(registry.find_path(&["watch", "risk"]).is_some());
        assert!(registry.find_path(&["orders", "create"]).is_some());
        assert!(registry.find_path(&["wallet", "create"]).is_some());
        assert!(registry.find_path(&["feedback"]).is_some());
        assert!(registry.find_path(&["schema"]).is_some());
    }

    #[test]
    fn registry_normalizes_option_placeholders_out_of_command_paths() {
        let registry = CommandRegistry::load().unwrap();
        assert!(
            registry
                .find_path(&["borrowlend", "supply"])
                .is_some_and(|command| command.command_key() == "borrowlend supply")
        );
    }

    #[test]
    fn registry_includes_current_command_families() {
        let registry = CommandRegistry::load().unwrap();
        let groups = registry
            .commands()
            .iter()
            .map(|command| command.group.as_str())
            .collect::<BTreeSet<_>>();

        for group in [
            "account",
            "builder",
            "market",
            "referral",
            "staking",
            "subscribe",
            "system",
            "trade",
            "transfer",
            "vault",
            "wallet",
        ] {
            assert!(groups.contains(group), "missing command group {group}");
        }
    }
}
