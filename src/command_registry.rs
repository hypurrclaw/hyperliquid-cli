//! The command contract registry, generated from the registered commands.
//!
//! The clap tree supplies names, args, defaults and help; each command's
//! [`CommandSpec`] supplies policy. Nothing here is written by hand per command.

use std::sync::LazyLock;

use clap::builder::ValueParser;
use clap::{Arg, ArgAction, Command};
use serde::Serialize;
use serde_json::{Value, json};

use crate::command_spec::{
    ArgSpec, CommandSpec, ConfirmationPolicy, DryRunPolicy, GroupSpec, InputKind, Lifecycle,
    OutputContract, OwsSupport, RawPayloadPolicy, Risk, StreamBound,
};
use crate::commands;

/// The order `schema` and the contract fixtures list commands in. A command that is
/// not listed follows in registration order, so a new command needs no entry here.
const SCHEMA_ORDER: &[&str] = &[
    "status",
    "meta",
    "perps list",
    "perps get",
    "spot list",
    "spot get",
    "book",
    "mids",
    "candles",
    "spread",
    "funding",
    "asset decode",
    "asset search",
    "search",
    "buy",
    "sell",
    "outcome-buy",
    "outcome-sell",
    "watch risk",
    "outcomes list",
    "outcomes get",
    "builder max-fee",
    "builder approved",
    "builder approve",
    "account fills",
    "account fees",
    "account rate-limit",
    "account orders",
    "account portfolio",
    "account subaccounts",
    "account portfolio-history",
    "account ledger",
    "account funding",
    "account twap-history",
    "account twap-fills",
    "account abstraction",
    "account abstraction set",
    "subaccount list",
    "subaccount create",
    "subaccount transfer",
    "subaccount spot-transfer",
    "account ls",
    "account add",
    "account set-default",
    "account remove",
    "api-wallet create",
    "api-wallet approve",
    "api-wallet list",
    "api-wallet revoke",
    "wallet create",
    "wallet import",
    "wallet show",
    "wallet address",
    "wallet reset",
    "wallet import-mnemonic",
    "wallet list",
    "wallet rename",
    "wallet delete",
    "wallet export",
    "orders open",
    "orders history",
    "orders status",
    "positions list",
    "staking validators",
    "staking summary",
    "staking rewards",
    "staking history",
    "vault list",
    "vault search",
    "vault get",
    "vault positions",
    "borrowlend rates",
    "borrowlend get",
    "borrowlend user",
    "borrowlend supply",
    "borrowlend withdraw",
    "prio status",
    "referral status",
    "orders create",
    "orders scale",
    "orders batch-create",
    "orders tpsl",
    "orders trailing-stop",
    "orders cancel",
    "orders cancel-all",
    "orders modify",
    "orders twap-create",
    "orders twap-cancel",
    "positions update-leverage",
    "positions update-margin",
    "orders schedule-cancel",
    "orders chase",
    "orders bracket",
    "transfer spot-to-perp",
    "transfer perp-to-spot",
    "transfer send",
    "transfer spot-send",
    "transfer send-asset",
    "transfer withdraw",
    "staking delegate",
    "staking undelegate",
    "staking deposit",
    "staking withdraw",
    "staking claim-rewards",
    "staking link initiate",
    "staking link finalize",
    "vault deposit",
    "vault withdraw",
    "prio bid",
    "referral set",
    "referral register",
    "subscribe trades",
    "subscribe orderbook",
    "subscribe candles",
    "subscribe all-mids",
    "subscribe order-updates",
    "subscribe fills",
    "feedback",
    "schema",
    "update",
    "setup",
];

#[derive(Debug, Clone, Serialize)]
pub struct CommandRegistry {
    commands: Vec<CommandContract>,
}

impl CommandRegistry {
    /// The registry of every registered command, built once.
    pub fn get() -> &'static Self {
        static REGISTRY: LazyLock<CommandRegistry> = LazyLock::new(CommandRegistry::build);
        &REGISTRY
    }

    fn build() -> Self {
        let groups = commands::groups();
        let mut tree = commands::command_tree();
        tree.build();
        let mut contracts = commands::entries()
            .iter()
            .map(|entry| CommandContract::generate(&tree, entry.spec, groups))
            .collect::<Vec<_>>();
        contracts.sort_by_key(|contract| {
            SCHEMA_ORDER
                .iter()
                .position(|key| *key == contract.command_key())
                .unwrap_or(SCHEMA_ORDER.len())
        });
        Self {
            commands: contracts,
        }
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
    #[serde(skip)]
    pub spec: &'static CommandSpec,
}

impl CommandContract {
    fn generate(tree: &Command, spec: &'static CommandSpec, groups: &[GroupSpec]) -> Self {
        let node = find_node(tree, spec.path);
        let mut args = node
            .get_arguments()
            .filter(|arg| !matches!(arg.get_id().as_str(), "help" | "version"))
            .collect::<Vec<_>>();
        for listed in spec.args {
            assert!(
                args.iter().any(|arg| arg.get_id() == listed.id),
                "command {} lists arg {} that clap does not define",
                spec.key(),
                listed.id
            );
        }
        args.sort_by_key(|arg| {
            spec.args
                .iter()
                .position(|listed| listed.id == arg.get_id())
                .unwrap_or(spec.args.len())
        });
        let usage = spec.schema_command.map_or_else(
            || {
                let mut positionals = args
                    .iter()
                    .filter(|arg| arg.is_positional())
                    .collect::<Vec<_>>();
                positionals.sort_by_key(|arg| arg.get_index());
                positionals
                    .iter()
                    .fold(format!("hyperliquid {}", spec.key()), |usage, arg| {
                        format!("{usage} <{}>", arg.get_id().as_str().to_uppercase())
                    })
            },
            str::to_string,
        );
        let aliases = groups
            .iter()
            .find(|group| group.path == &spec.path[..1] && group.schema_aliases)
            .map(|group| {
                group
                    .aliases
                    .iter()
                    .map(|alias| (*alias).to_string())
                    .collect()
            })
            .unwrap_or_default();
        Self {
            command: usage,
            command_path: spec.path.iter().map(|part| (*part).to_string()).collect(),
            aliases,
            group: spec.group.to_string(),
            description: spec.description.to_string(),
            auth_required: spec.auth_required,
            dangerous: spec.dangerous,
            lifecycle: spec.lifecycle,
            risk: spec.risk,
            mutability: Mutability::from_lifecycle_and_risk(spec.lifecycle, spec.risk),
            dry_run: spec.dry_run,
            raw_payload: spec.raw_payload,
            confirmation: spec.confirmation,
            transport: primary_transport(spec.lifecycle),
            ows_signer: spec.ows_signer,
            output_contract: output_contract(spec),
            one_of_required: spec
                .one_of_required
                .iter()
                .map(|required| required.iter().map(|id| (*id).to_string()).collect())
                .collect(),
            inputs: args
                .into_iter()
                .map(|arg| InputContract::generate(arg, spec.arg(arg.get_id().as_str())))
                .collect(),
            spec,
        }
    }

    pub fn command_key(&self) -> String {
        self.command_path.join(" ")
    }

    /// Name of the boolean flag that answers this command's confirmation prompt,
    /// derived from the args: `Some("yes")` for every command with a `yes` arg.
    pub fn confirmation_bypass_arg(&self) -> Option<&'static str> {
        self.inputs
            .iter()
            .any(|input| input.id == "yes")
            .then_some("yes")
    }

    /// Input for an arg id.
    pub fn input(&self, id: &str) -> Option<&InputContract> {
        self.inputs.iter().find(|input| input.id == id)
    }

    /// Schema description for an arg id, when it has one.
    pub fn arg_description(&self, id: &str) -> Option<&str> {
        self.input(id)
            .and_then(|input| input.description.as_deref())
    }

    pub fn metadata_json(&self) -> Value {
        let yes_support = self.confirmation_bypass_arg();
        let stream_bounds = match self.spec.stream {
            Some(StreamBound::Subscribe) => json!({
                "required_in_machine_context": true,
                "cli_args": ["max_events", "idle_timeout_ms"],
                "env": ["HYPERLIQUID_SUBSCRIBE_MAX_EVENTS"],
                "output_mode": "ndjson"
            }),
            Some(StreamBound::SnapshotWatch) => json!({
                "required_in_machine_context": true,
                "required_when": {"watch": true},
                "cli_args": ["max_ticks"],
                "env": ["HYPERLIQUID_WATCH_MAX_TICKS"],
                "output_mode": "ndjson"
            }),
            None => Value::Null,
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
    fn generate(arg: &Arg, spec: Option<&ArgSpec>) -> Self {
        let flag = matches!(arg.get_action(), ArgAction::SetTrue | ArgAction::SetFalse);
        let repeats = matches!(arg.get_action(), ArgAction::Append);
        Self {
            id: arg.get_id().to_string(),
            long: arg.get_long().map(str::to_string),
            short: arg.get_short().map(String::from),
            positional_index: arg.get_index().map(|index| index - 1),
            arg_type: if spec.is_some_and(|spec| spec.schema_string) {
                "string"
            } else if flag {
                "boolean"
            } else if is_integer(arg) {
                "integer"
            } else {
                "string"
            }
            .to_string(),
            required: arg.is_required_set() || spec.is_some_and(|spec| spec.schema_required),
            multiple: repeats && !spec.is_some_and(|spec| spec.schema_single),
            enum_values: arg
                .get_possible_values()
                .iter()
                .map(|value| value.get_name().to_string())
                .collect(),
            description: spec
                .and_then(|spec| spec.description)
                .map(str::to_string)
                .or_else(|| arg.get_help().map(|help| format!("{help}."))),
            default: if flag {
                None
            } else {
                arg.get_default_values()
                    .first()
                    .map(|value| value.to_string_lossy().to_string())
            },
            kind: spec.and_then(|spec| spec.kind),
        }
    }
}

fn find_node<'a>(tree: &'a Command, path: &[&str]) -> &'a Command {
    path.iter().fold(tree, |node, name| {
        node.find_subcommand(name)
            .unwrap_or_else(|| panic!("command path {path:?} is not in the clap tree"))
    })
}

fn is_integer(arg: &Arg) -> bool {
    let actual = arg.get_value_parser().type_id();
    [
        ValueParser::from(clap::value_parser!(u8)),
        ValueParser::from(clap::value_parser!(u16)),
        ValueParser::from(clap::value_parser!(u32)),
        ValueParser::from(clap::value_parser!(u64)),
        ValueParser::from(clap::value_parser!(usize)),
        ValueParser::from(clap::value_parser!(i8)),
        ValueParser::from(clap::value_parser!(i16)),
        ValueParser::from(clap::value_parser!(i32)),
        ValueParser::from(clap::value_parser!(i64)),
        ValueParser::from(clap::value_parser!(isize)),
    ]
    .iter()
    .any(|integer| integer.type_id() == actual)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    CliProcess,
    CliInteractive,
    CliBlocked,
}

fn primary_transport(lifecycle: Lifecycle) -> Vec<Transport> {
    match lifecycle {
        Lifecycle::InteractiveLocal => vec![Transport::CliInteractive],
        Lifecycle::BlockedUnsupported => vec![Transport::CliBlocked],
        _ => vec![Transport::CliProcess],
    }
}

fn output_contract(spec: &CommandSpec) -> OutputContract {
    if let Some(contract) = spec.output {
        contract
    } else if spec.stream == Some(StreamBound::Subscribe) {
        OutputContract::BoundedNdjsonStream
    } else if spec.lifecycle == Lifecycle::InteractiveLocal {
        OutputContract::InteractiveLocal
    } else if spec.lifecycle == Lifecycle::BlockedUnsupported {
        OutputContract::BlockedUnsupported
    } else if spec.dry_run == DryRunPolicy::Optional {
        OutputContract::CommandResultOrDryRunEnvelope
    } else {
        OutputContract::JsonValue
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn registry_enumerates_every_registered_command() {
        let registry = CommandRegistry::get();
        assert_eq!(registry.commands().len(), 121);
        for path in [
            &["asset", "decode"][..],
            &["asset", "search"],
            &["search"],
            &["buy"],
            &["orders", "chase"],
            &["orders", "bracket"],
            &["orders", "trailing-stop"],
            &["watch", "risk"],
            &["orders", "create"],
            &["wallet", "create"],
            &["feedback"],
            &["schema"],
        ] {
            assert!(registry.find_path(path).is_some(), "{path:?}");
        }
    }

    #[test]
    fn registry_includes_current_command_families() {
        let groups = CommandRegistry::get()
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

    #[test]
    fn schema_order_names_only_registered_commands() {
        let registry = CommandRegistry::get();
        for key in SCHEMA_ORDER {
            let path = key.split(' ').collect::<Vec<_>>();
            assert!(registry.find_path(&path).is_some(), "{key}");
        }
    }
}
