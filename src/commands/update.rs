//! `update`: replace this binary with the latest release.

use crate::invocation::Invocation;

use crate::command_spec::{CliCommand, CommandSpec, DryRunPolicy, Lifecycle, NoArgs, Risk};

pub struct Update;

impl CliCommand for Update {
    type Args = NoArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["update"],
        "system",
        "Update this binary to the latest GitHub release",
        "Update this binary from the latest GitHub release on Linux/macOS. Windows users should rerun install.sh to install the latest .zip release. Use --dry-run to preview without replacing the executable.",
    )
    .lifecycle(Lifecycle::InteractiveLocal)
    .risk(Risk::LocalState)
    .dry_run(DryRunPolicy::Optional)
    .dangerous(true)
    .local();

    async fn run(inv: &Invocation, _args: &NoArgs) -> Result<(), anyhow::Error> {
        crate::update::update(inv).await
    }
}
