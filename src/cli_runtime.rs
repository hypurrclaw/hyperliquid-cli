//! The one pipeline every command runs through: validate, bound streams, check
//! the payload, dry-run and confirmation policy, resolve the API endpoint, run.
//! What each step does for a command comes from its `CommandSpec`.

use clap::{ArgMatches, FromArgMatches};
use hyperliquid_cli::command_spec::{
    CommandSpec, ConfirmationPolicy, DryRunPolicy, RawPayloadPolicy, StreamBound, check_args,
    secret_arg_supplied,
};
use hyperliquid_cli::errors::CliError;
use hyperliquid_cli::input_hardening::{
    FilePolicy, parse_json_text, read_json_file, validate_input_path, validate_resource_id,
};
use hyperliquid_cli::invocation::Invocation;
use hyperliquid_cli::output;
use hyperliquid_cli::stream::{StreamBoundArgs, require_event_bound, require_tick_bound};

use crate::Cli;
use crate::cli_shell::{Confirmation, Selected, print_help_to_stdout};

pub(super) async fn run(
    cli: &Cli,
    selected: Option<Selected>,
    confirmation: Confirmation,
    mut inv: Invocation,
) -> Result<(), anyhow::Error> {
    let Some(selected) = selected else {
        return print_help_to_stdout(&[]);
    };
    let spec = selected.entry.spec;

    validate_global_inputs(cli)?;
    check_args(spec, &selected.matches)?;
    selected.bound.validate()?;
    validate_stream_bounds(spec, &selected.matches, &inv)?;

    if cli.payload_json.is_some() || cli.payload_file.is_some() {
        validate_raw_payload_supported(spec)?;
    }
    validate_prompt_policy(cli, &inv, spec, &selected.matches, confirmation)?;
    if inv.dry_run() && spec.dry_run != DryRunPolicy::Optional {
        return Err(CliError::Unsupported(spec.dry_run_refusal.map_or_else(
            || "--dry-run is only supported for mutating commands with preview support".to_string(),
            str::to_string,
        ))
        .into());
    }
    if spec.local {
        return selected.bound.run(&inv).await;
    }

    inv.set_payload(read_payload_json(cli)?);
    if inv.payload().is_some() && !inv.dry_run() {
        return Err(CliError::Unsupported(
            "payload input currently requires --dry-run so raw payloads can be validated without side effects"
                .to_string(),
        )
        .into());
    }

    let has_api_override = inv.remote()?.api().has_override();
    if inv.dry_run() {
        // Signed exchange actions name the signer in their preview. Resolution
        // never prompts; a failure is reported inside the preview instead.
        inv.attach_dry_run_context(
            spec.auth_required
                && spec.lifecycle == hyperliquid_cli::command_spec::Lifecycle::LiveMutating,
        );
    }
    if has_api_override && spec.uses_api() {
        inv.remote()?.api().probe_override().await?;
    }

    selected.bound.run(&inv).await
}

fn validate_global_inputs(cli: &Cli) -> Result<(), CliError> {
    if let Some(account) = cli.account.as_deref() {
        validate_resource_id("account selector", account)?;
    }
    if let Some(path) = cli.payload_file.as_deref() {
        validate_input_path(path)?;
    }
    Ok(())
}

fn validate_raw_payload_supported(spec: &CommandSpec) -> Result<(), CliError> {
    if spec.raw_payload == RawPayloadPolicy::DryRunOnly {
        return Ok(());
    }
    Err(CliError::Unsupported(format!(
        "payload input is not supported for {}",
        spec.key()
    )))
}

fn validate_prompt_policy(
    cli: &Cli,
    inv: &Invocation,
    spec: &CommandSpec,
    matches: &ArgMatches,
    confirmation: Confirmation,
) -> Result<(), CliError> {
    if machine_context(inv) && secret_arg_supplied(spec, matches) {
        return Err(CliError::Unsupported(
            "argv secret input is not supported in machine-readable contexts; use an interactive prompt or a future safe stdin/file secret input"
                .to_string(),
        ));
    }
    if inv.dry_run()
        || inv.mode().can_prompt()
        || spec.confirmation != ConfirmationPolicy::Prompt
        || (cli.testnet && spec.testnet_unprompted)
        || confirmation.given
        || !confirmation_applies(spec, matches)
    {
        return Ok(());
    }

    Err(confirmation_required_error(spec, confirmation))
}

/// `account remove` with no selector has nothing to confirm yet: its run fails
/// on the missing selector (missing input) instead of prompting for one.
fn confirmation_applies(spec: &CommandSpec, matches: &ArgMatches) -> bool {
    spec.confirms_with_arg
        .is_none_or(|id| matches.try_contains_id(id).unwrap_or(false))
}

/// Name the blocked command, then give the safe next steps: a runnable
/// `--dry-run` preview when supported, and the `--yes` rerun only for use
/// after a human approves.
fn confirmation_required_error(spec: &CommandSpec, confirmation: Confirmation) -> CliError {
    let args = replay_args(std::env::args().skip(1));
    let preview = (spec.dry_run == DryRunPolicy::Optional).then(|| {
        let mut argv = vec!["--dry-run".to_string()];
        argv.extend(args.iter().cloned());
        replay_command(&argv)
    });
    let approve = confirmation.supported.then(|| {
        let mut argv = args.clone();
        argv.push("--yes".to_string());
        replay_command(&argv)
    });
    CliError::ConfirmationRequired {
        headline: format!(
            "Can't run `{}` without confirmation in non-interactive mode.",
            spec.key()
        ),
        preview,
        approve,
    }
}

/// Options whose values are secrets and never appear in suggested commands.
const SECRET_OPTIONS: &[&str] = &["--private-key", "--keystore-password"];

/// Invocation arguments (without the program name) minus secret options.
fn replay_args(args: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut args = args.into_iter();
    let mut kept = Vec::new();
    while let Some(arg) = args.next() {
        if SECRET_OPTIONS.contains(&arg.as_str()) {
            args.next();
            continue;
        }
        if SECRET_OPTIONS
            .iter()
            .any(|option| arg.starts_with(&format!("{option}=")))
        {
            continue;
        }
        kept.push(arg);
    }
    kept
}

fn replay_command(args: &[String]) -> String {
    let mut command = String::from("hyperliquid");
    for arg in args {
        command.push(' ');
        let plain = !arg.is_empty()
            && arg
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "-_./:=@%+,#".contains(ch));
        if plain {
            command.push_str(arg);
        } else {
            command.push('\'');
            command.push_str(&arg.replace('\'', r"'\''"));
            command.push('\'');
        }
    }
    command
}

fn validate_stream_bounds(
    spec: &CommandSpec,
    matches: &ArgMatches,
    inv: &Invocation,
) -> Result<(), CliError> {
    if !machine_context(inv) {
        return Ok(());
    }
    match spec.stream {
        Some(StreamBound::SnapshotWatch) => {
            if matches.get_flag("watch") {
                require_tick_bound(matches.get_one::<usize>("max_ticks").copied())?;
            }
        }
        Some(StreamBound::Subscribe) => {
            let bounds = StreamBoundArgs::from_arg_matches(matches)
                .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?;
            require_event_bound(&bounds)?;
        }
        None => {}
    }
    Ok(())
}

fn machine_context(inv: &Invocation) -> bool {
    inv.format() == output::OutputFormat::Json || inv.session().env().agent
}

fn read_payload_json(cli: &Cli) -> Result<Option<serde_json::Value>, anyhow::Error> {
    let Some(raw) = cli.payload_json.as_deref() else {
        let Some(path) = cli.payload_file.as_deref() else {
            return Ok(None);
        };
        return read_json_file(std::path::Path::new(path), FilePolicy::payload())
            .map(Some)
            .map_err(Into::into);
    };
    parse_json_text(raw, "payload")
        .map(Some)
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::{replay_args, replay_command};

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn replay_drops_secret_options_and_quotes_for_the_shell() {
        let args = replay_args(strings(&[
            "--private-key",
            "0xsecret",
            "--keystore-password=hunter2",
            "orders",
            "create",
            "--coin",
            "BTC",
            "--cloid",
            "it's here",
        ]));
        assert_eq!(
            replay_command(&args),
            r"hyperliquid orders create --coin BTC --cloid 'it'\''s here'"
        );
    }
}
