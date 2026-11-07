//! One definition per command.
//!
//! A command is a type that implements [`CliCommand`]: its clap `Args`, its
//! [`CommandSpec`] (the policy the schema, help notes and the run pipeline read)
//! and its `run`. [`Entry::of`] erases the type; the list of entries in
//! `commands::entries` is the only place a command is registered. The clap tree,
//! the schema registry and dispatch are all derived from that list.

use std::pin::Pin;

use clap::{ArgMatches, Command, FromArgMatches};
use serde::Serialize;

use crate::assets::{AssetQuery, parse_asset_query};
use crate::errors::CliError;
use crate::input_hardening::validate_resource_id;
use crate::invocation::Invocation;

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

/// How a command bounds its output in machine contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamBound {
    /// `--watch` needs `--max-ticks` (or `HYPERLIQUID_WATCH_MAX_TICKS`).
    SnapshotWatch,
    /// Needs `--max-events`, `--idle-timeout-ms` or `HYPERLIQUID_SUBSCRIBE_MAX_EVENTS`.
    Subscribe,
}

/// Rejects a malformed value before the command runs.
#[derive(Debug, Clone, Copy)]
pub enum Check {
    /// `validate_resource_id` under this label.
    ResourceId(&'static str),
    /// Like `ResourceId`, but `#N` / `+N` outcome notation passes untouched.
    AssetInput(&'static str),
}

impl Check {
    fn run(self, value: &str) -> Result<(), CliError> {
        match self {
            Self::ResourceId(label) => validate_resource_id(label, value),
            Self::AssetInput(label) => {
                if matches!(parse_asset_query(value), AssetQuery::Outcome(_)) {
                    return Ok(());
                }
                validate_resource_id(label, value)
            }
        }
    }
}

/// What a command says about one of its clap args beyond what clap knows.
#[derive(Debug, Clone, Copy)]
pub struct ArgSpec {
    pub id: &'static str,
    pub kind: Option<InputKind>,
    /// Schema description when it is not the clap help text plus a period.
    pub description: Option<&'static str>,
    pub check: Option<Check>,
    /// The value is a secret: it must not arrive on the command line in machine contexts.
    pub secret: bool,
    /// The schema lists the arg as required although clap accepts its absence.
    pub schema_required: bool,
    /// The schema lists the arg as single-valued although clap repeats it.
    pub schema_single: bool,
    /// The schema lists the arg as a string although clap treats it as a flag.
    pub schema_string: bool,
}

#[must_use]
pub const fn arg(id: &'static str) -> ArgSpec {
    ArgSpec {
        id,
        kind: None,
        description: None,
        check: None,
        secret: false,
        schema_required: false,
        schema_single: false,
        schema_string: false,
    }
}

impl ArgSpec {
    #[must_use]
    pub const fn kind(mut self, kind: InputKind) -> Self {
        self.kind = Some(kind);
        self
    }

    #[must_use]
    pub const fn describe(mut self, description: &'static str) -> Self {
        self.description = Some(description);
        self
    }

    #[must_use]
    pub const fn resource_id(mut self, label: &'static str) -> Self {
        self.check = Some(Check::ResourceId(label));
        self
    }

    #[must_use]
    pub const fn asset_input(mut self, label: &'static str) -> Self {
        self.check = Some(Check::AssetInput(label));
        self
    }

    #[must_use]
    pub const fn secret(mut self) -> Self {
        self.secret = true;
        self
    }

    #[must_use]
    pub const fn schema_required(mut self) -> Self {
        self.schema_required = true;
        self
    }

    #[must_use]
    pub const fn schema_single(mut self) -> Self {
        self.schema_single = true;
        self
    }

    #[must_use]
    pub const fn schema_string(mut self) -> Self {
        self.schema_string = true;
        self
    }
}

/// Everything about a command that is not clap's own knowledge of its args.
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    pub path: &'static [&'static str],
    pub group: &'static str,
    pub about: &'static str,
    pub long_about: Option<&'static str>,
    /// Schema description.
    pub description: &'static str,
    pub auth_required: bool,
    pub dangerous: bool,
    pub lifecycle: Lifecycle,
    pub risk: Risk,
    pub dry_run: DryRunPolicy,
    pub raw_payload: RawPayloadPolicy,
    pub confirmation: ConfirmationPolicy,
    pub ows_signer: OwsSupport,
    /// Mainnet asks for confirmation; `--testnet` runs without asking.
    pub testnet_unprompted: bool,
    /// The confirmation prompt applies only when this arg is present.
    pub confirms_with_arg: Option<&'static str>,
    /// Runs without resolving the network, signer or API endpoint, so its
    /// dry-run preview names neither.
    pub local: bool,
    pub stream: Option<StreamBound>,
    /// Error text for `--dry-run` on a command that does not preview, when it is
    /// not the generic one.
    pub dry_run_refusal: Option<&'static str>,
    /// Extra spellings of the command name.
    pub cli_aliases: &'static [&'static str],
    /// Schema `command` text when it is not the path plus positionals.
    pub schema_command: Option<&'static str>,
    /// Output contract when it is not derived from the other fields.
    pub output: Option<OutputContract>,
    pub one_of_required: &'static [&'static [&'static str]],
    pub args: &'static [ArgSpec],
}

impl CommandSpec {
    /// A read-only command that needs no signer and has no preview.
    #[must_use]
    pub const fn read(
        path: &'static [&'static str],
        group: &'static str,
        about: &'static str,
        description: &'static str,
    ) -> Self {
        Self {
            path,
            group,
            about,
            long_about: None,
            description,
            auth_required: false,
            dangerous: false,
            lifecycle: Lifecycle::ReadOnly,
            risk: Risk::None,
            dry_run: DryRunPolicy::NotSupported,
            raw_payload: RawPayloadPolicy::Unsupported,
            confirmation: ConfirmationPolicy::None,
            ows_signer: OwsSupport::NotRequired,
            testnet_unprompted: false,
            confirms_with_arg: None,
            local: false,
            stream: None,
            dry_run_refusal: None,
            cli_aliases: &[],
            schema_command: None,
            output: None,
            one_of_required: &[],
            args: &[],
        }
    }

    /// A signed exchange action that moves funds, with a dry-run preview.
    #[must_use]
    pub const fn signed(
        path: &'static [&'static str],
        group: &'static str,
        about: &'static str,
        description: &'static str,
    ) -> Self {
        let mut spec = Self::read(path, group, about, description);
        spec.auth_required = true;
        spec.dangerous = true;
        spec.lifecycle = Lifecycle::LiveMutating;
        spec.risk = Risk::FundsMovement;
        spec.dry_run = DryRunPolicy::Optional;
        spec.raw_payload = RawPayloadPolicy::DryRunOnly;
        spec.ows_signer = OwsSupport::ExperimentalFeatureGated;
        spec
    }

    #[must_use]
    pub const fn long_about(mut self, text: &'static str) -> Self {
        self.long_about = Some(text);
        self
    }

    #[must_use]
    pub const fn lifecycle(mut self, lifecycle: Lifecycle) -> Self {
        self.lifecycle = lifecycle;
        self
    }

    #[must_use]
    pub const fn risk(mut self, risk: Risk) -> Self {
        self.risk = risk;
        self
    }

    #[must_use]
    pub const fn dry_run(mut self, policy: DryRunPolicy) -> Self {
        self.dry_run = policy;
        self
    }

    #[must_use]
    pub const fn raw_payload(mut self, policy: RawPayloadPolicy) -> Self {
        self.raw_payload = policy;
        self
    }

    #[must_use]
    pub const fn confirmation(mut self, policy: ConfirmationPolicy) -> Self {
        self.confirmation = policy;
        self
    }

    #[must_use]
    pub const fn prompt(self) -> Self {
        self.confirmation(ConfirmationPolicy::Prompt)
    }

    #[must_use]
    pub const fn ows(mut self, support: OwsSupport) -> Self {
        self.ows_signer = support;
        self
    }

    #[must_use]
    pub const fn auth(mut self, required: bool) -> Self {
        self.auth_required = required;
        self
    }

    #[must_use]
    pub const fn dangerous(mut self, dangerous: bool) -> Self {
        self.dangerous = dangerous;
        self
    }

    #[must_use]
    pub const fn testnet_unprompted(mut self) -> Self {
        self.testnet_unprompted = true;
        self
    }

    #[must_use]
    pub const fn confirms_with_arg(mut self, id: &'static str) -> Self {
        self.confirms_with_arg = Some(id);
        self
    }

    #[must_use]
    pub const fn local(mut self) -> Self {
        self.local = true;
        self
    }

    #[must_use]
    pub const fn stream(mut self, bound: StreamBound) -> Self {
        self.stream = Some(bound);
        self
    }

    #[must_use]
    pub const fn dry_run_refusal(mut self, text: &'static str) -> Self {
        self.dry_run_refusal = Some(text);
        self
    }

    #[must_use]
    pub const fn cli_aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.cli_aliases = aliases;
        self
    }

    #[must_use]
    pub const fn schema_command(mut self, text: &'static str) -> Self {
        self.schema_command = Some(text);
        self
    }

    #[must_use]
    pub const fn output(mut self, contract: OutputContract) -> Self {
        self.output = Some(contract);
        self
    }

    #[must_use]
    pub const fn one_of_required(mut self, groups: &'static [&'static [&'static str]]) -> Self {
        self.one_of_required = groups;
        self
    }

    #[must_use]
    pub const fn args(mut self, args: &'static [ArgSpec]) -> Self {
        self.args = args;
        self
    }

    #[must_use]
    pub fn key(&self) -> String {
        self.path.join(" ")
    }

    #[must_use]
    pub fn arg(&self, id: &str) -> Option<&ArgSpec> {
        self.args.iter().find(|arg| arg.id == id)
    }

    /// Whether the command talks to the Hyperliquid API.
    #[must_use]
    pub fn uses_api(&self) -> bool {
        self.group != "system"
            && !matches!(
                self.lifecycle,
                Lifecycle::InteractiveLocal | Lifecycle::BlockedUnsupported
            )
            && !matches!(self.risk, Risk::LocalSecret | Risk::LocalState)
    }
}

/// A command group such as `orders`: a name with children and no behavior.
#[derive(Debug, Clone, Copy)]
pub struct GroupSpec {
    pub path: &'static [&'static str],
    pub about: &'static str,
    pub long_about: Option<&'static str>,
    /// Hidden alternate spellings.
    pub aliases: &'static [&'static str],
    /// The schema lists `aliases` on every command under the group.
    pub schema_aliases: bool,
}

impl GroupSpec {
    #[must_use]
    pub const fn new(path: &'static [&'static str], about: &'static str) -> Self {
        Self {
            path,
            about,
            long_about: None,
            aliases: &[],
            schema_aliases: false,
        }
    }

    #[must_use]
    pub const fn long_about(mut self, text: &'static str) -> Self {
        self.long_about = Some(text);
        self
    }

    #[must_use]
    pub const fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }

    #[must_use]
    pub const fn schema_aliases(mut self) -> Self {
        self.schema_aliases = true;
        self
    }
}

/// A command without arguments.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct NoArgs;

pub type RunFuture<'a> = Pin<Box<dyn Future<Output = Result<(), anyhow::Error>> + 'a>>;

pub trait CliCommand: 'static {
    type Args: clap::Args + FromArgMatches + 'static;

    const SPEC: CommandSpec;

    /// Rejections that need more than a per-arg [`Check`]. Runs right after them.
    fn validate(_args: &Self::Args) -> Result<(), CliError> {
        Ok(())
    }

    fn run<'a>(
        inv: &'a Invocation,
        args: &'a Self::Args,
    ) -> impl Future<Output = Result<(), anyhow::Error>> + 'a;
}

/// The parsed args of the selected command.
pub trait Bound {
    fn validate(&self) -> Result<(), CliError>;
    fn run<'a>(&'a self, inv: &'a Invocation) -> RunFuture<'a>;
}

struct Parsed<C: CliCommand>(C::Args);

impl<C: CliCommand> Bound for Parsed<C> {
    fn validate(&self) -> Result<(), CliError> {
        C::validate(&self.0)
    }

    fn run<'a>(&'a self, inv: &'a Invocation) -> RunFuture<'a> {
        Box::pin(C::run(inv, &self.0))
    }
}

/// A registered command with its type erased.
pub struct Entry {
    pub spec: &'static CommandSpec,
    augment: fn(Command) -> Command,
    bind: fn(&ArgMatches) -> Result<Box<dyn Bound>, clap::Error>,
}

impl Entry {
    #[must_use]
    pub fn of<C: CliCommand>() -> Self {
        Self {
            spec: &C::SPEC,
            augment: <C::Args as clap::Args>::augment_args,
            bind: |matches| {
                Ok(Box::new(Parsed::<C>(C::Args::from_arg_matches(matches)?)) as Box<dyn Bound>)
            },
        }
    }

    pub fn bind(&self, matches: &ArgMatches) -> Result<Box<dyn Bound>, clap::Error> {
        (self.bind)(matches)
    }
}

/// Rejects malformed values of the args that carry a [`Check`].
pub fn check_args(spec: &CommandSpec, matches: &ArgMatches) -> Result<(), CliError> {
    for arg in spec.args {
        let Some(check) = arg.check else { continue };
        let Ok(Some(values)) = matches.try_get_raw(arg.id) else {
            continue;
        };
        for value in values {
            check.run(&value.to_string_lossy())?;
        }
    }
    Ok(())
}

/// Whether a secret arg of the command was typed on the command line.
#[must_use]
pub fn secret_arg_supplied(spec: &CommandSpec, matches: &ArgMatches) -> bool {
    spec.args.iter().any(|arg| {
        arg.secret && matches.value_source(arg.id) == Some(clap::parser::ValueSource::CommandLine)
    })
}

struct Node<'a> {
    name: &'static str,
    entry: Option<&'a Entry>,
    group: Option<&'a GroupSpec>,
    children: Vec<Node<'a>>,
}

impl<'a> Node<'a> {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            entry: None,
            group: None,
            children: Vec::new(),
        }
    }

    fn at(&mut self, path: &[&'static str]) -> &mut Node<'a> {
        let Some((first, rest)) = path.split_first() else {
            return self;
        };
        let index = match self.children.iter().position(|child| child.name == *first) {
            Some(index) => index,
            None => {
                self.children.push(Node::new(first));
                self.children.len() - 1
            }
        };
        self.children[index].at(rest)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(self.name);
        if let Some(entry) = self.entry {
            let spec = entry.spec;
            command = (entry.augment)(command).about(spec.about);
            if let Some(long_about) = spec.long_about {
                command = command.long_about(long_about);
            }
            for alias in spec.cli_aliases {
                command = command.alias(*alias);
            }
        } else if let Some(group) = self.group {
            command = command.about(group.about);
            if let Some(long_about) = group.long_about {
                command = command.long_about(long_about);
            }
            for alias in group.aliases {
                command = command.alias(*alias);
            }
            command = command
                .subcommand_required(true)
                .arg_required_else_help(true);
        }
        for (order, child) in self.children.iter().enumerate() {
            command = command.subcommand(child.command().display_order(order));
        }
        command
    }
}

/// The clap subcommand tree of `entries`, in registration order, under a bare
/// `hyperliquid` command. Groups come from `groups`.
#[must_use]
pub fn command_tree(entries: &[Entry], groups: &[GroupSpec]) -> Command {
    let mut root = Node::new("hyperliquid");
    for entry in entries {
        let node = root.at(entry.spec.path);
        assert!(
            node.entry.is_none(),
            "command {} is registered twice",
            entry.spec.key()
        );
        node.entry = Some(entry);
    }
    for group in groups {
        let node = root.at(group.path);
        assert!(
            node.entry.is_none() && !node.children.is_empty(),
            "group {} has no commands or is itself a command",
            group.path.join(" ")
        );
        node.group = Some(group);
    }
    let mut command = Command::new("hyperliquid");
    for (order, child) in root.children.iter().enumerate() {
        command = command.subcommand(child.command().display_order(order));
    }
    command
}
