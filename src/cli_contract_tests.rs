//! Drift tests between the clap command tree and `command_catalog.json`.
//!
//! The catalog feeds `hyperliquid schema` and generated help, so every leaf command and
//! every non-global option or positional in clap must be described there, and the other
//! way around. Failures list each mismatch as `path: missing/extra descriptor`.

use std::collections::{BTreeMap, BTreeSet};

use clap::{Arg, Command, CommandFactory};
use hyperliquid_cli::command_registry::{CommandContract, CommandRegistry};

use crate::Cli;

type Path = Vec<String>;

/// Invocable commands keyed by their path of canonical clap names: leaves, plus commands
/// that take an optional subcommand (`account abstraction [ADDRESS]` also has `set`).
fn clap_leaves(root: &Command) -> BTreeMap<Path, &Command> {
    fn walk<'a>(cmd: &'a Command, path: &mut Path, out: &mut BTreeMap<Path, &'a Command>) {
        let mut has_children = false;
        for sub in cmd.get_subcommands().filter(|sub| sub.get_name() != "help") {
            has_children = true;
            path.push(sub.get_name().to_string());
            walk(sub, path, out);
            path.pop();
        }
        let invocable = !has_children || !cmd.is_subcommand_required_set();
        if invocable && !path.is_empty() {
            out.insert(path.clone(), cmd);
        }
    }
    let mut out = BTreeMap::new();
    walk(root, &mut Vec::new(), &mut out);
    out
}

/// Resolve a catalog path (which may use visible aliases such as `transfers`) to the
/// canonical clap path.
fn canonical_path(root: &Command, path: &[String]) -> Option<Path> {
    let mut cmd = root;
    let mut canonical = Vec::new();
    for part in path {
        cmd = cmd.get_subcommands().find(|sub| {
            sub.get_name() == part || sub.get_all_aliases().any(|alias| alias == part)
        })?;
        canonical.push(cmd.get_name().to_string());
    }
    Some(canonical)
}

fn clap_descriptor(arg: &Arg) -> String {
    if arg.is_positional() {
        // clap indexes positionals from 1; the catalog from 0.
        let index = arg.get_index().map_or(0, |index| index - 1);
        return format!("<{}> (positional {index})", arg.get_id());
    }
    describe_flag(arg.get_long(), arg.get_short())
}

fn catalog_descriptor(contract: &CommandContract, id: &str) -> String {
    let input = contract
        .input(id)
        .expect("input id comes from the contract");
    if let Some(index) = input.positional_index {
        return format!("<{}> (positional {index})", input.id);
    }
    describe_flag(
        input.long.as_deref(),
        input
            .short
            .as_deref()
            .and_then(|short| short.chars().next()),
    )
}

fn describe_flag(long: Option<&str>, short: Option<char>) -> String {
    match (long, short) {
        (Some(long), Some(short)) => format!("--{long} (-{short})"),
        (Some(long), None) => format!("--{long}"),
        (None, Some(short)) => format!("-{short}"),
        (None, None) => "<unnamed option>".to_string(),
    }
}

/// Non-global options and positionals. Propagated globals keep their global flag after
/// `build()`; a local positional that reuses a global's id (`account`) does not.
fn clap_descriptors(cmd: &Command) -> BTreeSet<String> {
    cmd.get_arguments()
        .filter(|arg| !arg.is_global_set())
        .filter(|arg| !matches!(arg.get_id().as_str(), "help" | "version"))
        .map(clap_descriptor)
        .collect()
}

fn catalog_descriptors(contract: &CommandContract) -> BTreeSet<String> {
    contract
        .inputs
        .iter()
        .map(|input| catalog_descriptor(contract, &input.id))
        .collect()
}

fn fail_if_any(title: &str, problems: Vec<String>) {
    assert!(
        problems.is_empty(),
        "{title}: {} problem(s)\n  {}\n\nFix src/command_catalog.json (or the clap definition) so both agree.",
        problems.len(),
        problems.join("\n  ")
    );
}

/// The clap tree after `build()`, which assigns positional indexes.
fn built_root() -> Command {
    let mut root = Cli::command();
    root.build();
    root
}

fn registry() -> CommandRegistry {
    CommandRegistry::load().expect("embedded command catalog parses")
}

#[test]
fn catalog_leaves_and_clap_leaves_are_the_same_set() {
    let root = built_root();
    let clap = clap_leaves(&root);
    let registry = registry();

    let mut problems = Vec::new();
    let mut catalog_paths = BTreeSet::new();
    for contract in registry.commands() {
        match canonical_path(&root, &contract.command_path) {
            Some(path) if clap.contains_key(&path) => {
                catalog_paths.insert(path);
            }
            Some(_) => problems.push(format!(
                "`{}` is in the catalog but is not an invocable command in clap",
                contract.command_key()
            )),
            None => problems.push(format!(
                "`{}` is in the catalog but does not exist in clap",
                contract.command_key()
            )),
        }
    }
    for path in clap.keys() {
        if !catalog_paths.contains(path) {
            problems.push(format!(
                "`{}` exists in clap but is missing from the catalog",
                path.join(" ")
            ));
        }
    }
    fail_if_any("catalog and clap command trees differ", problems);
}

#[test]
fn catalog_args_match_clap_args_for_every_leaf() {
    let root = built_root();
    let clap = clap_leaves(&root);
    let registry = registry();

    let mut problems = Vec::new();
    for contract in registry.commands() {
        let Some(path) = canonical_path(&root, &contract.command_path) else {
            continue;
        };
        let Some(cmd) = clap.get(&path) else {
            continue;
        };
        let expected = clap_descriptors(cmd);
        let actual = catalog_descriptors(contract);
        for missing in expected.difference(&actual) {
            problems.push(format!(
                "{}: clap has {missing}, catalog does not",
                contract.command_key()
            ));
        }
        for extra in actual.difference(&expected) {
            problems.push(format!(
                "{}: catalog has {extra}, clap does not",
                contract.command_key()
            ));
        }
    }
    fail_if_any("catalog args differ from clap args", problems);
}

#[test]
fn every_catalog_arg_has_a_description() {
    let problems = registry()
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
    fail_if_any("catalog args without descriptions", problems);
}

#[test]
fn confirmation_bypass_is_supported_exactly_when_clap_has_a_yes_flag() {
    let root = built_root();
    let clap = clap_leaves(&root);
    let registry = registry();

    let mut problems = Vec::new();
    for contract in registry.commands() {
        let Some(path) = canonical_path(&root, &contract.command_path) else {
            continue;
        };
        let Some(cmd) = clap.get(&path) else {
            continue;
        };
        let clap_has_yes = cmd
            .get_arguments()
            .any(|arg| arg.get_id().as_str() == "yes");
        let metadata = contract.metadata_json();
        let bypass = &metadata["confirmation_bypass"];
        let supported = bypass["supported"].as_bool().unwrap_or(false);
        let arg = bypass["arg"].as_str();
        if clap_has_yes != supported {
            problems.push(format!(
                "{}: clap yes flag = {clap_has_yes}, catalog confirmation_bypass.supported = {supported}",
                contract.command_key()
            ));
        }
        if supported && arg != Some("yes") {
            problems.push(format!(
                "{}: confirmation_bypass.arg is {arg:?}, expected \"yes\"",
                contract.command_key()
            ));
        }
        if !supported && arg.is_some() {
            problems.push(format!(
                "{}: confirmation_bypass.arg is {arg:?} but bypass is unsupported",
                contract.command_key()
            ));
        }
    }
    fail_if_any("confirmation bypass metadata differs from clap", problems);
}
