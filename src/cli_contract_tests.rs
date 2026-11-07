//! Policy invariants over the generated command registry.
//!
//! The registry is built from the clap tree and each command's `CommandSpec`, so
//! names, flags and defaults cannot drift. What can still go wrong is policy that
//! contradicts itself or names args the command does not have.

use hyperliquid_cli::command_registry::CommandRegistry;
use hyperliquid_cli::command_spec::{ConfirmationPolicy, StreamBound};

fn problems_or_pass(title: &str, problems: Vec<String>) {
    assert!(
        problems.is_empty(),
        "{title}: {} problem(s)\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

#[test]
fn every_schema_input_has_a_description() {
    let problems = CommandRegistry::get()
        .commands()
        .iter()
        .flat_map(|contract| {
            contract
                .inputs
                .iter()
                .filter(|input| {
                    input
                        .description
                        .as_deref()
                        .is_none_or(|description| description.trim().is_empty())
                })
                .map(|input| {
                    format!(
                        "{}: arg `{}` has no description",
                        contract.command_key(),
                        input.id
                    )
                })
        })
        .collect();
    problems_or_pass("args without descriptions", problems);
}

#[test]
fn testnet_unprompted_commands_prompt_on_mainnet() {
    let problems = CommandRegistry::get()
        .commands()
        .iter()
        .filter(|contract| {
            contract.spec.testnet_unprompted
                && (contract.spec.confirmation != ConfirmationPolicy::Prompt
                    || contract.confirmation_bypass_arg().is_none())
        })
        .map(|contract| {
            format!(
                "{}: skips the testnet prompt but has no mainnet prompt or --yes",
                contract.command_key()
            )
        })
        .collect();
    problems_or_pass("testnet policy", problems);
}

#[test]
fn bounded_commands_define_the_args_that_bound_them() {
    let mut problems = Vec::new();
    for contract in CommandRegistry::get().commands() {
        let required: &[&str] = match contract.spec.stream {
            Some(StreamBound::SnapshotWatch) => &["watch", "max_ticks"],
            Some(StreamBound::Subscribe) => &["max_events", "idle_timeout_ms"],
            None => &[],
        };
        for id in required {
            if contract.input(id).is_none() {
                problems.push(format!("{}: missing `{id}`", contract.command_key()));
            }
        }
    }
    problems_or_pass("stream bounds", problems);
}

#[test]
fn policy_args_are_args_of_their_command() {
    let mut problems = Vec::new();
    for contract in CommandRegistry::get().commands() {
        if let Some(id) = contract.spec.confirms_with_arg
            && contract.input(id).is_none()
        {
            problems.push(format!(
                "{}: confirms_with_arg `{id}`",
                contract.command_key()
            ));
        }
        for group in contract.spec.one_of_required {
            for id in *group {
                if contract.input(id).is_none() {
                    problems.push(format!(
                        "{}: one_of_required `{id}`",
                        contract.command_key()
                    ));
                }
            }
        }
    }
    problems_or_pass("policy args", problems);
}
