//! Command-line shell around the clap tree: parsing, help and usage-error
//! rendering, panic handling, and output-format resolution.
//!
//! `main.rs` declares the commands and boots the process; everything that
//! decides how the command line is presented to people and agents lives here.

use super::*;
use clap::parser::ValueSource;
use std::io::{IsTerminal, Write};

const ENV_FORMAT: &str = "HYPERLIQUID_FORMAT";
const REPORT_URL: &str = "https://github.com/hypurrclaw/hyperliquid-cli/issues/new";

/// Top-level commands in help order: market data, trading, accounts and
/// wallets, funds, streaming, then tools.
const ROOT_ORDER: &[&str] = &[
    "perps",
    "spot",
    "asset",
    "outcomes",
    "book",
    "mids",
    "candles",
    "spread",
    "funding",
    "search",
    "status",
    "meta",
    "orders",
    "buy",
    "sell",
    "outcome-buy",
    "outcome-sell",
    "positions",
    "account",
    "wallet",
    "api-wallet",
    "subaccount",
    "builder",
    "referral",
    "staking",
    "vault",
    "borrowlend",
    "prio",
    "transfer",
    "subscribe",
    "watch",
    "schema",
    "setup",
    "feedback",
    "update",
];

/// Hidden alternate spellings for groups. A singular name behaves like its plural.
const GROUP_ALIASES: &[(&str, &str)] = &[
    ("perps", "perp"),
    ("asset", "assets"),
    ("outcomes", "outcome"),
    ("orders", "order"),
    ("positions", "position"),
    ("transfer", "transfers"),
    ("wallet", "wallets"),
    ("account", "accounts"),
    ("subaccount", "subaccounts"),
    ("api-wallet", "api-wallets"),
    ("vault", "vaults"),
    ("builder", "builders"),
    ("referral", "referrals"),
];

pub(super) static PANIC_COMMAND: OnceLock<String> = OnceLock::new();

/// What the command line said about `--yes` for the matched command.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Confirmation {
    /// The command has a `--yes` option.
    pub supported: bool,
    /// `--yes` was passed.
    pub given: bool,
}

pub(super) struct ParsedCli {
    pub(super) cli: Cli,
    pub(super) registry_path: Option<Vec<String>>,
    /// `--format` was given on the command line (not env or a default).
    pub(super) explicit_format: bool,
    pub(super) confirmation: Confirmation,
}

pub(super) fn try_parse_cli_from<I, T>(args: I) -> Result<ParsedCli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let mut command = build_command();
    let matches = command.try_get_matches_from_mut(args)?;
    let registry_path = command_registry_path_from_matches(&matches);
    let explicit_format = matches.value_source("format") == Some(ValueSource::CommandLine);
    let confirmation = confirmation_from_matches(&matches);
    let cli = Cli::from_arg_matches(&matches)?;
    Ok(ParsedCli {
        cli,
        registry_path,
        explicit_format,
        confirmation,
    })
}

fn command_registry_path_from_matches(matches: &ArgMatches) -> Option<Vec<String>> {
    let mut path = Vec::new();
    let mut current = matches;
    while let Some((name, subcommand)) = current.subcommand() {
        path.push(name.to_string());
        current = subcommand;
    }
    (!path.is_empty()).then_some(path)
}

/// Read `--yes` from the matched command itself, so the answer can't drift
/// from the options the command actually defines.
fn confirmation_from_matches(matches: &ArgMatches) -> Confirmation {
    let mut leaf = matches;
    while let Some((_, subcommand)) = leaf.subcommand() {
        leaf = subcommand;
    }
    match leaf.try_get_one::<bool>("yes") {
        Ok(Some(given)) => Confirmation {
            supported: true,
            given: *given,
        },
        _ => Confirmation::default(),
    }
}

// ── Command tree ────────────────────────────────────────────────────────

/// The clap command tree with the pieces derive can't express: one help style
/// at every depth, `-v`, task-ordered top-level commands, aliases, and help
/// text filled from the command catalog.
fn build_command() -> clap::Command {
    let registry = CommandRegistry::load().ok();
    let mut command = Cli::command()
        .arg(
            clap::Arg::new("help")
                .short('h')
                .long("help")
                .global(true)
                .action(clap::ArgAction::HelpLong)
                .help("Print help")
                .help_heading(GLOBAL_OPTIONS),
        )
        .arg(
            clap::Arg::new("version")
                .short('v')
                .short_alias('V')
                .long("version")
                .global(true)
                .action(clap::ArgAction::Version)
                .help("Print version")
                .help_heading(GLOBAL_OPTIONS),
        );

    let names: Vec<String> = command
        .get_subcommands()
        .map(|sub| sub.get_name().to_string())
        .collect();
    for name in names {
        let order = ROOT_ORDER
            .iter()
            .position(|candidate| *candidate == name)
            .unwrap_or(ROOT_ORDER.len());
        let aliases: Vec<&'static str> = GROUP_ALIASES
            .iter()
            .filter(|(group, _)| *group == name)
            .map(|(_, alias)| *alias)
            .collect();
        let path = vec![name.clone()];
        command = command.mut_subcommand(&name, |sub| {
            let mut sub = sub.display_order(order);
            for alias in aliases {
                sub = sub.alias(alias);
            }
            decorate_command(sub, &path, registry.as_ref())
        });
    }
    let shared_help: Vec<(String, String)> = command
        .find_subcommand("orders")
        .and_then(|orders| orders.find_subcommand("create"))
        .map(|create| {
            create
                .get_arguments()
                .filter_map(|arg| {
                    arg.get_help()
                        .map(|help| (arg.get_id().to_string(), help.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    inherit_option_help(command, &shared_help)
}

/// Order options shared by `buy`, `sell`, `orders chase`, and others have the
/// same meaning everywhere. Give each the help text `orders create` defines
/// when the catalog and the command itself have none.
fn inherit_option_help(mut command: clap::Command, shared: &[(String, String)]) -> clap::Command {
    let undocumented: Vec<String> = command
        .get_arguments()
        .filter(|arg| arg.get_help().is_none() && arg.get_long_help().is_none())
        .map(|arg| arg.get_id().to_string())
        .collect();
    for id in undocumented {
        if let Some((_, help)) = shared.iter().find(|(shared_id, _)| *shared_id == id) {
            let help = help.clone();
            command = command.mut_arg(&id, |arg| arg.help(help));
        }
    }
    let children: Vec<String> = command
        .get_subcommands()
        .map(|sub| sub.get_name().to_string())
        .collect();
    for child in children {
        command = command.mut_subcommand(&child, |sub| inherit_option_help(sub, shared));
    }
    command
}

/// Add `ls`/`list` aliases, fill missing option help from the catalog, and
/// append catalog-derived notes to commands that change state.
fn decorate_command(
    mut command: clap::Command,
    path: &[String],
    registry: Option<&CommandRegistry>,
) -> clap::Command {
    let children: Vec<String> = command
        .get_subcommands()
        .map(|sub| sub.get_name().to_string())
        .collect();
    for child in children {
        let mut child_path = path.to_vec();
        child_path.push(child.clone());
        command =
            command.mut_subcommand(&child, |sub| decorate_command(sub, &child_path, registry));
    }

    match command.get_name() {
        "list" => command = command.alias("ls"),
        "ls" => command = command.alias("list"),
        _ => {}
    }

    let path_refs: Vec<&str> = path.iter().map(String::as_str).collect();
    let Some(contract) = registry.and_then(|registry| registry.find_path(&path_refs)) else {
        return command;
    };

    let undocumented: Vec<String> = command
        .get_arguments()
        .filter(|arg| arg.get_help().is_none() && arg.get_long_help().is_none())
        .map(|arg| arg.get_id().to_string())
        .collect();
    for id in undocumented {
        let description = contract
            .inputs
            .iter()
            .find(|input| input.id == id)
            .and_then(|input| input.description.as_deref());
        if let Some(description) = description {
            let help = description.trim_end_matches('.').to_string();
            command = command.mut_arg(&id, |arg| arg.help(help));
        }
    }

    let has_yes = command.get_arguments().any(|arg| arg.get_id() == "yes");
    if let Some(notes) = command_notes(contract, has_yes) {
        let combined = match command.get_after_long_help() {
            Some(existing) => format!("{existing}\n\n{notes}"),
            None => notes,
        };
        command = command.after_long_help(combined);
    }
    command
}

/// Whether a command changes state, locally or on the exchange.
fn command_changes_state(contract: &CommandContract) -> bool {
    matches!(
        contract.lifecycle,
        Lifecycle::LiveMutating | Lifecycle::InteractiveLocal
    ) || matches!(contract.risk, Risk::LocalSecret | Risk::LocalState)
}

/// `Notes:` block for commands that change state: risk, dry-run support, and
/// how confirmation behaves.
fn command_notes(contract: &CommandContract, has_yes: bool) -> Option<String> {
    if !command_changes_state(contract) {
        return None;
    }
    let risk = match contract.risk {
        Risk::FundsMovement => "moves funds or changes live exchange state",
        Risk::AccountState => "changes account settings on the exchange",
        Risk::LocalSecret => "reads or writes secret key material on this machine",
        Risk::LocalState => "changes local configuration or wallet state",
        Risk::None => "changes live exchange state",
    };
    let mut lines = vec!["Notes:".to_string(), format!("  Risk: {risk}.")];
    lines.push(match contract.dry_run {
        DryRunPolicy::Optional => {
            "  Dry-run: supported. Add --dry-run to preview without side effects.".to_string()
        }
        DryRunPolicy::NotSupported => "  Dry-run: not supported.".to_string(),
    });
    lines.push(match (contract.confirmation, has_yes) {
        (ConfirmationPolicy::Prompt, true) => "  Confirmation: asks before it runs. Pass --yes to answer the prompt. Runs that can't prompt (JSON output, HYPERLIQUID_AGENT, CI, --non-interactive) fail without --yes. Use --yes there only after a human approves.".to_string(),
        (ConfirmationPolicy::Prompt, false) => "  Confirmation: asks before it runs. Runs that can't prompt (JSON output, HYPERLIQUID_AGENT, CI, --non-interactive) fail instead.".to_string(),
        (ConfirmationPolicy::None, _) => {
            "  Confirmation: none. It runs as soon as you invoke it.".to_string()
        }
    });
    if cli_runtime::TESTNET_UNPROMPTED_COMMANDS.contains(&contract.command_key().as_str()) {
        lines.push(
            "  Testnet: with --testnet this command doesn't prompt and runs as soon as you invoke it."
                .to_string(),
        );
    }
    Some(lines.join("\n"))
}

// ── Help, usage errors, and exit paths ──────────────────────────────────

fn styled_text(styled: &clap::builder::StyledStr, color: bool) -> String {
    if color {
        styled.ansi().to_string()
    } else {
        styled.to_string()
    }
}

/// Write to stdout. A closed pipe (`| head`) is a normal end, not an error.
fn write_stdout(text: &str) -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

fn exit_after_stdout(text: &str) -> ! {
    let code = match write_stdout(text) {
        Ok(()) => 0,
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "Failed to write to stdout: {err}");
            1
        }
    };
    std::process::exit(code)
}

/// The deepest command along `path` that exists.
fn find_command<'a>(root: &'a clap::Command, path: &[String]) -> &'a clap::Command {
    let mut command = root;
    for name in path {
        match command.find_subcommand(name) {
            Some(sub) => command = sub,
            None => break,
        }
    }
    command
}

fn help_text_for_path(path: &[String]) -> String {
    let mut root = build_command();
    root.build();
    let mut target = find_command(&root, path).clone();
    styled_text(&target.render_long_help(), output::colors::stdout_enabled())
}

/// Print the long help for `path` (the root when empty) on stdout.
pub(super) fn print_help_to_stdout(path: &[String]) -> Result<(), anyhow::Error> {
    write_stdout(&help_text_for_path(path))
        .map_err(|err| anyhow::anyhow!("failed to write help to stdout: {err}"))
}

fn help_command(path: &[String]) -> String {
    if path.is_empty() {
        "hyperliquid --help".to_string()
    } else {
        format!("hyperliquid {} --help", path.join(" "))
    }
}

fn is_help_or_version_token(arg: &OsString) -> bool {
    matches!(
        arg.to_str(),
        Some("-h" | "--help" | "-v" | "-V" | "--version")
    )
}

/// Parse as much of the command line as possible, skipping what clap rejects.
///
/// Used only after a real parse failed, to learn which command was reached and
/// which global options were given, so errors can be rendered in the right
/// format and point at the right help page.
fn lenient_matches(raw_args: &[OsString]) -> Option<ArgMatches> {
    let mut args: Vec<OsString> = raw_args
        .iter()
        .filter(|arg| !is_help_or_version_token(arg))
        .cloned()
        .collect();
    let mut command = build_command().ignore_errors(true);
    for _ in 0..8 {
        match command.try_get_matches_from_mut(&args) {
            Ok(matches) => return Some(matches),
            Err(err) => {
                let mut indexes = offending_arg_indexes(&err, &args)?;
                indexes.sort_unstable_by(|a, b| b.cmp(a));
                for index in indexes {
                    args.remove(index);
                }
            }
        }
    }
    None
}

/// Positions in `args` of the token clap rejected (and its value, if any).
fn offending_arg_indexes(err: &clap::Error, args: &[OsString]) -> Option<Vec<usize>> {
    use clap::error::{ContextKind, ContextValue, ErrorKind};

    let context_string = |kind: ContextKind| match err.get(kind) {
        Some(ContextValue::String(value)) => Some(value.clone()),
        _ => None,
    };
    let (name, has_value) = match err.kind() {
        ErrorKind::UnknownArgument => (context_string(ContextKind::InvalidArg)?, false),
        ErrorKind::InvalidSubcommand => (context_string(ContextKind::InvalidSubcommand)?, false),
        ErrorKind::InvalidValue | ErrorKind::ValueValidation => {
            (context_string(ContextKind::InvalidArg)?, true)
        }
        _ => return None,
    };
    let name = name.split([' ', '=']).next()?.to_string();
    let with_equals = format!("{name}=");
    let index = args.iter().position(|arg| {
        arg.to_str()
            .is_some_and(|arg| arg == name || arg.starts_with(&with_equals))
    })?;
    let mut indexes = vec![index];
    let attached = args[index].to_str().is_some_and(|arg| arg.contains('='));
    if has_value && !attached && index + 1 < args.len() {
        indexes.push(index + 1);
    }
    Some(indexes)
}

/// Output format for a failed parse: an explicit `--format` wins, then
/// `HYPERLIQUID_FORMAT`, then JSON for agents and non-terminal stdout.
fn error_format(matches: Option<&ArgMatches>) -> output::OutputFormat {
    if let Some(matches) = matches
        && matches.value_source("format") == Some(ValueSource::CommandLine)
        && let Some(format) = matches.get_one::<output::OutputFormat>("format")
    {
        return *format;
    }
    match env_output_format() {
        Some(Ok(format)) => return format,
        Some(Err(_)) => return output::OutputFormat::Json,
        None => {}
    }
    if hyperliquid_cli::mode::agent_env() || !std::io::stdout().is_terminal() {
        output::OutputFormat::Json
    } else {
        output::OutputFormat::Pretty
    }
}

/// Keep only clap's best suggestion. clap lists the closest match last.
fn narrow_suggestions(err: &mut clap::Error) {
    use clap::error::{ContextKind, ContextValue};

    for kind in [
        ContextKind::SuggestedSubcommand,
        ContextKind::SuggestedArg,
        ContextKind::SuggestedValue,
    ] {
        let best = match err.get(kind) {
            Some(ContextValue::Strings(values)) if values.len() > 1 => values.last().cloned(),
            _ => None,
        };
        if let Some(best) = best {
            err.insert(kind, ContextValue::Strings(vec![best]));
        }
    }
}

/// Render a usage error: one suggestion, the usage line of the command that
/// was actually reached, and a pointer to its help.
fn usage_error_text(mut err: clap::Error, path: &[String], color: bool) -> String {
    use clap::error::{ContextKind, ContextValue};

    narrow_suggestions(&mut err);
    let mut root = build_command();
    root.build();
    if err.get(ContextKind::Usage).is_some() {
        let usage = find_command(&root, path).clone().render_usage();
        err.insert(ContextKind::Usage, ContextValue::StyledStr(usage));
    }
    let text = styled_text(&err.render(), color);
    let body = match text.rfind("\n\nFor more information") {
        Some(index) => &text[..index],
        None => text.trim_end(),
    };
    format!("{body}\n\nSee all options:\n    {}\n", help_command(path))
}

/// Print what clap reported and exit. Help and version are successes on
/// stdout; everything else is a usage error (exit 2).
pub(super) fn exit_for_clap_error(err: clap::Error, raw_args: &[OsString]) -> ! {
    use clap::error::ErrorKind;

    let lenient = lenient_matches(raw_args);
    // A partial parse may not have applied flag defaults, so read the flag fallibly.
    output::colors::init(lenient.as_ref().is_some_and(|matches| {
        matches
            .try_get_one::<bool>("no_color")
            .ok()
            .flatten()
            .copied()
            .unwrap_or(false)
    }));
    let path = lenient
        .as_ref()
        .and_then(command_registry_path_from_matches)
        .unwrap_or_default();
    match err.kind() {
        ErrorKind::DisplayHelp => exit_after_stdout(&styled_text(
            &err.render(),
            output::colors::stdout_enabled(),
        )),
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            exit_after_stdout(&help_text_for_path(&path))
        }
        ErrorKind::DisplayVersion => {
            let version = env!("CARGO_PKG_VERSION");
            let text = if error_format(lenient.as_ref()) == output::OutputFormat::Json {
                format!("{}\n", serde_json::json!({ "version": version }))
            } else {
                format!("hyperliquid {version}\n")
            };
            exit_after_stdout(&text)
        }
        _ => {
            let exit_code = err.exit_code();
            if error_format(lenient.as_ref()) == output::OutputFormat::Json {
                let text = usage_error_text(err, &path, false);
                exit_after_stdout_with_code(
                    &format!(
                        "{}\n",
                        errors::ErrorEnvelope::usage(text.trim_end()).to_json()
                    ),
                    exit_code,
                );
            }
            let text = usage_error_text(err, &path, output::colors::stderr_enabled());
            let _ = write!(std::io::stderr(), "{text}");
            std::process::exit(exit_code)
        }
    }
}

fn exit_after_stdout_with_code(text: &str, code: i32) -> ! {
    let _ = write_stdout(text);
    std::process::exit(code)
}

// ── Panics ──────────────────────────────────────────────────────────────

/// A panic from `println!`/`eprintln!` on a closed pipe.
fn is_broken_pipe_panic(message: &str) -> bool {
    message.contains("Broken pipe")
        && (message.contains("failed printing to stdout")
            || message.contains("failed printing to stderr"))
}

/// A closed pipe ends the process quietly. Any other panic is a bug: say so
/// with the command path (never argument values) and exit 1.
pub(super) fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|message| (*message).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        if is_broken_pipe_panic(&message) {
            std::process::exit(0);
        }
        let command = match PANIC_COMMAND.get() {
            Some(path) if !path.is_empty() => format!("hyperliquid {path}"),
            _ => "hyperliquid".to_string(),
        };
        let _ = write!(
            std::io::stderr(),
            "Unexpected error in {command}. This is a bug in hyperliquid-cli.\nVersion: {}\nReport it at:\n    {REPORT_URL}\n",
            env!("CARGO_PKG_VERSION"),
        );
        // Developers opt in to the panic message and location with RUST_BACKTRACE.
        if std::env::var("RUST_BACKTRACE").is_ok_and(|value| !value.is_empty() && value != "0") {
            let _ = writeln!(std::io::stderr(), "Panic: {info}");
        }
        std::process::exit(1);
    }));
}

// ── Output format resolution ────────────────────────────────────────────

/// `HYPERLIQUID_FORMAT`, when set to a non-empty value.
fn env_output_format() -> Option<Result<output::OutputFormat, errors::CliError>> {
    let raw = std::env::var(ENV_FORMAT).ok()?;
    if raw.trim().is_empty() {
        return None;
    }
    Some(parse_env_output_format(&raw))
}

/// An explicit `--format` wins over everything. Otherwise `HYPERLIQUID_FORMAT`,
/// then JSON for agents and non-terminal stdout, then the option's default.
pub(super) fn resolve_output_format(
    explicit_format: bool,
    cli_format: output::OutputFormat,
) -> Result<output::OutputFormat, errors::CliError> {
    if explicit_format {
        return Ok(cli_format);
    }
    if let Some(format) = env_output_format() {
        return format;
    }
    if hyperliquid_cli::mode::agent_env() || !std::io::stdout().is_terminal() {
        return Ok(output::OutputFormat::Json);
    }
    Ok(cli_format)
}

fn parse_env_output_format(raw: &str) -> Result<output::OutputFormat, errors::CliError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "pretty" => Ok(output::OutputFormat::Pretty),
        "table" => Ok(output::OutputFormat::Table),
        "json" => Ok(output::OutputFormat::Json),
        other => Err(errors::CliError::Configuration(format!(
            "{ENV_FORMAT} must be one of pretty, table, or json; got '{other}'"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_runtime::{
        command_accepts_mutating_preview, command_contract_for_path, command_uses_api,
    };

    /// Split an example line into words, honoring single and double quotes.
    fn split_command_line(line: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut current = String::new();
        let mut quote: Option<char> = None;
        let mut started = false;
        for ch in line.chars() {
            match (quote, ch) {
                (Some(open), c) if c == open => quote = None,
                (Some(_), c) => current.push(c),
                (None, '\'' | '"') => {
                    quote = Some(ch);
                    started = true;
                }
                (None, c) if c.is_whitespace() => {
                    if started || !current.is_empty() {
                        words.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                (None, c) => current.push(c),
            }
        }
        if started || !current.is_empty() {
            words.push(current);
        }
        words
    }

    fn collect_examples(command: &clap::Command, out: &mut Vec<String>) {
        for text in [
            command.get_about(),
            command.get_long_about(),
            command.get_after_help(),
            command.get_after_long_help(),
        ]
        .into_iter()
        .flatten()
        {
            let text = text.to_string();
            let mut in_examples = false;
            for line in text.lines() {
                let line = line.trim();
                if line == "Examples:" {
                    in_examples = true;
                } else if in_examples {
                    if line.starts_with("hyperliquid ") {
                        out.push(line.to_string());
                    } else {
                        in_examples = false;
                    }
                }
            }
        }
        for sub in command.get_subcommands() {
            collect_examples(sub, out);
        }
    }

    #[test]
    fn every_help_example_parses() {
        let mut examples = Vec::new();
        collect_examples(&build_command(), &mut examples);
        assert!(
            examples.len() >= 25,
            "expected help examples, found {}",
            examples.len()
        );
        for example in examples {
            let words = split_command_line(&example);
            if let Err(err) = try_parse_cli_from(&words) {
                panic!("help example doesn't parse: {example}\n{err}");
            }
        }
    }

    fn visit_commands(
        command: &clap::Command,
        path: &mut Vec<String>,
        f: &mut dyn FnMut(&clap::Command, &[String]),
    ) {
        f(command, path);
        for sub in command.get_subcommands() {
            path.push(sub.get_name().to_string());
            visit_commands(sub, path, f);
            path.pop();
        }
    }

    #[test]
    fn every_state_changing_command_documents_its_risk() {
        let registry = CommandRegistry::load().unwrap();
        let root = build_command();
        let mut checked = 0;
        visit_commands(&root, &mut Vec::new(), &mut |command, path| {
            let refs: Vec<&str> = path.iter().map(String::as_str).collect();
            let Some(contract) = registry.find_path(&refs) else {
                return;
            };
            if !command_changes_state(contract) {
                return;
            }
            checked += 1;
            let help = command
                .get_after_long_help()
                .map(|help| help.to_string())
                .unwrap_or_default();
            assert!(
                help.contains("Notes:") && help.contains("Risk:") && help.contains("Dry-run:"),
                "{} is missing notes",
                path.join(" ")
            );
        });
        assert!(
            checked >= 40,
            "only {checked} state-changing commands checked"
        );
    }

    #[test]
    fn orders_create_notes_state_that_testnet_does_not_prompt() {
        let root = build_command();
        let create = find_command(&root, &["orders".to_string(), "create".to_string()]);
        let help = create.get_after_long_help().unwrap().to_string();
        assert!(
            help.contains("with --testnet this command doesn't prompt"),
            "{help}"
        );
        assert!(help.contains("Pass --yes"), "{help}");
    }

    #[test]
    fn every_option_has_help_text() {
        let root = build_command();
        let mut missing = Vec::new();
        visit_commands(&root, &mut Vec::new(), &mut |command, path| {
            for arg in command.get_arguments() {
                if arg.is_hide_set() {
                    continue;
                }
                if arg.get_help().is_none() && arg.get_long_help().is_none() {
                    missing.push(format!("{} {}", path.join(" "), arg.get_id()));
                }
            }
        });
        assert!(missing.is_empty(), "options without help: {missing:?}");
    }

    #[test]
    fn root_commands_are_all_ordered() {
        let root = build_command();
        for sub in root.get_subcommands() {
            assert!(
                ROOT_ORDER.contains(&sub.get_name()),
                "{} is missing from ROOT_ORDER",
                sub.get_name()
            );
        }
    }

    #[test]
    fn explicit_format_is_read_from_parsed_matches() {
        for (argv, explicit, format) in [
            (
                &["hyperliquid", "-fjson", "status"][..],
                true,
                output::OutputFormat::Json,
            ),
            (
                &["hyperliquid", "-ftable", "status"][..],
                true,
                output::OutputFormat::Table,
            ),
            (
                &["hyperliquid", "status", "--format=pretty"][..],
                true,
                output::OutputFormat::Pretty,
            ),
            (
                &["hyperliquid", "status"][..],
                false,
                output::OutputFormat::Pretty,
            ),
        ] {
            let parsed = try_parse_cli_from(argv.iter().copied()).unwrap();
            assert_eq!(parsed.explicit_format, explicit, "{argv:?}");
            assert_eq!(parsed.cli.format, format, "{argv:?}");
        }
    }

    #[test]
    fn failed_parse_still_reads_explicit_format() {
        let args = |items: &[&str]| items.iter().map(OsString::from).collect::<Vec<_>>();
        let lenient = lenient_matches(&args(&["hyperliquid", "--format=pretty", "--bogus"]));
        assert_eq!(error_format(lenient.as_ref()), output::OutputFormat::Pretty);
        let lenient = lenient_matches(&args(&["hyperliquid", "-f=table", "--bogus"]));
        assert_eq!(error_format(lenient.as_ref()), output::OutputFormat::Table);
        let lenient = lenient_matches(&args(&["hyperliquid", "-fjson", "--bogus"]));
        assert_eq!(error_format(lenient.as_ref()), output::OutputFormat::Json);
        let lenient = lenient_matches(&args(&["hyperliquid", "--format=json", "--bogus"]));
        assert_eq!(error_format(lenient.as_ref()), output::OutputFormat::Json);
    }

    #[test]
    fn lenient_parse_finds_the_command_that_was_reached() {
        let args = ["hyperliquid", "orders", "crete"].map(OsString::from);
        let lenient = lenient_matches(&args).unwrap();
        assert_eq!(
            command_registry_path_from_matches(&lenient),
            Some(vec!["orders".to_string()])
        );
    }

    #[test]
    fn unknown_subcommand_shows_one_suggestion_and_its_group_help() {
        let err = try_parse_cli_from(["hyperliquid", "--no-update-check", "orders", "cancl"])
            .err()
            .unwrap();
        let text = usage_error_text(err, &["orders".to_string()], false);
        assert_eq!(text.matches("tip:").count(), 1, "{text}");
        assert!(text.contains("'cancel'"), "{text}");
        assert!(!text.contains("--no-update-check"), "{text}");
        assert!(text.contains("Usage: hyperliquid orders"), "{text}");
        assert!(
            text.trim_end()
                .ends_with("See all options:\n    hyperliquid orders --help"),
            "{text}"
        );
        assert!(!text.contains("For more information"), "{text}");
    }

    #[test]
    fn usage_line_shows_the_command_path_not_consumed_globals() {
        let err =
            try_parse_cli_from(["hyperliquid", "--no-update-check", "--form", "json", "mids"])
                .err()
                .unwrap();
        let text = usage_error_text(err, &[], false);
        assert!(!text.contains("--no-update-check"), "{text}");
        assert!(text.contains("Usage: hyperliquid [OPTIONS]"), "{text}");
        assert!(text.trim_end().ends_with("hyperliquid --help"), "{text}");
    }

    #[test]
    fn short_and_long_help_are_identical() {
        let render = |flag: &str| {
            try_parse_cli_from(["hyperliquid", "orders", "create", flag])
                .err()
                .unwrap()
                .render()
                .to_string()
        };
        assert_eq!(render("-h"), render("--help"));
    }

    #[test]
    fn version_flags_work_at_every_depth() {
        use clap::error::ErrorKind;
        for argv in [
            &["hyperliquid", "-v"][..],
            &["hyperliquid", "-V"][..],
            &["hyperliquid", "--version"][..],
            &["hyperliquid", "orders", "--version"][..],
            &["hyperliquid", "orders", "create", "--version"][..],
        ] {
            let err = try_parse_cli_from(argv.iter().copied()).err().unwrap();
            assert_eq!(err.kind(), ErrorKind::DisplayVersion, "{argv:?}");
        }
    }

    #[test]
    fn group_without_action_asks_for_help() {
        use clap::error::ErrorKind;
        let err = try_parse_cli_from(["hyperliquid", "orders"]).err().unwrap();
        assert_eq!(
            err.kind(),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn list_has_ls_alias_and_ls_has_list_alias() {
        for argv in [
            &["hyperliquid", "perps", "ls"][..],
            &["hyperliquid", "positions", "ls"][..],
            &["hyperliquid", "account", "list"][..],
            &["hyperliquid", "account", "ls"][..],
            &["hyperliquid", "order", "open"][..],
            &["hyperliquid", "position", "list"][..],
            &["hyperliquid", "perp", "list"][..],
            &["hyperliquid", "outcome", "list"][..],
            &["hyperliquid", "wallets", "list"][..],
            &["hyperliquid", "accounts", "ls"][..],
        ] {
            try_parse_cli_from(argv.iter().copied())
                .unwrap_or_else(|err| panic!("{argv:?} should parse: {err}"));
        }
    }

    #[test]
    fn confirmation_comes_from_the_matched_command() {
        let parse = |argv: &[&str]| {
            try_parse_cli_from(argv.iter().copied())
                .unwrap()
                .confirmation
        };
        let with_yes = parse(&["hyperliquid", "wallet", "export", "main", "--yes"]);
        assert!(with_yes.supported && with_yes.given);
        let without = parse(&["hyperliquid", "wallet", "export", "main"]);
        assert!(without.supported && !without.given);
        let none = parse(&["hyperliquid", "wallet", "show"]);
        assert!(!none.supported && !none.given);
        let setup = parse(&["hyperliquid", "setup", "--yes"]);
        assert!(setup.supported && setup.given);
    }

    #[test]
    fn broken_pipe_panics_are_recognized() {
        assert!(is_broken_pipe_panic(
            "failed printing to stdout: Broken pipe (os error 32)"
        ));
        assert!(is_broken_pipe_panic(
            "failed printing to stderr: Broken pipe (os error 32)"
        ));
        assert!(!is_broken_pipe_panic("index out of bounds"));
        assert!(!is_broken_pipe_panic(
            "failed printing to stdout: Interrupted"
        ));
    }

    #[test]
    fn root_help_lists_environment_and_no_hype() {
        let help = help_text_for_path(&[]);
        assert!(help.contains("Environment:"), "{help}");
        for name in [
            "HYPERLIQUID_FORMAT",
            "HYPERLIQUID_KEYSTORE_PASSWORD",
            "OWS_PASSPHRASE",
            "CI",
        ] {
            assert!(help.contains(name), "{name} missing from root help");
        }
        assert!(!help.to_lowercase().contains("production-grade"), "{help}");
        assert!(help.contains("Global Options:"), "{help}");
    }

    #[test]
    fn cli_command_paths_resolve_to_registry_contracts() {
        for (argv, expected_path) in [
            (
                &["hyperliquid", "api-wallet", "list"][..],
                "api-wallet list",
            ),
            (&["hyperliquid", "orders", "history"][..], "orders history"),
            (
                &["hyperliquid", "orders", "cancel", "123"][..],
                "orders cancel",
            ),
            (&["hyperliquid", "wallet", "show"][..], "wallet show"),
            (
                &[
                    "hyperliquid",
                    "feedback",
                    "--scenario-json",
                    "{\"command\":\"mids\",\"actual\":\"ok\"}",
                ][..],
                "feedback",
            ),
            (
                &["hyperliquid", "subscribe", "fills"][..],
                "subscribe fills",
            ),
        ] {
            let parsed = try_parse_cli_from(argv.iter().copied()).unwrap();
            let contract = command_contract_for_path(parsed.registry_path.as_deref())
                .unwrap()
                .unwrap_or_else(|| panic!("missing registry contract for {argv:?}"));

            assert_eq!(contract.command_key(), expected_path);
        }
    }

    #[test]
    fn cli_policy_uses_registry_mutating_preview_and_api_contracts() {
        let api_wallet_list = command_contract_for_path(
            try_parse_cli_from(["hyperliquid", "api-wallet", "list"])
                .unwrap()
                .registry_path
                .as_deref(),
        )
        .unwrap();
        assert!(!command_accepts_mutating_preview(api_wallet_list.as_ref()));
        assert!(command_uses_api(api_wallet_list.as_ref()));

        let orders_cancel = command_contract_for_path(
            try_parse_cli_from(["hyperliquid", "orders", "cancel", "123"])
                .unwrap()
                .registry_path
                .as_deref(),
        )
        .unwrap();
        assert!(command_accepts_mutating_preview(orders_cancel.as_ref()));
        assert!(command_uses_api(orders_cancel.as_ref()));

        let wallet_show = command_contract_for_path(
            try_parse_cli_from(["hyperliquid", "wallet", "show"])
                .unwrap()
                .registry_path
                .as_deref(),
        )
        .unwrap();
        assert!(!command_accepts_mutating_preview(wallet_show.as_ref()));
        assert!(!command_uses_api(wallet_show.as_ref()));
    }
}
