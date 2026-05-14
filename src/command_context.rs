//! Per-call command execution context.
//!
//! This is the migration boundary for typed in-process command handlers. Legacy
//! handlers still use process-global CLI setup, while migrated handlers receive
//! their output projection and execution metadata explicitly.

use std::fmt;
use std::time::Duration;

use hypersdk::hypercore::HttpClient;

use crate::output::{self, JsonRenderOptions, OutputFormat, TableData};

#[derive(Clone, Copy)]
pub struct CommandClients<'a> {
    hypercore: Option<&'a HttpClient>,
}

impl<'a> CommandClients<'a> {
    #[must_use]
    pub fn empty() -> Self {
        Self { hypercore: None }
    }

    #[must_use]
    pub fn with_hypercore(hypercore: &'a HttpClient) -> Self {
        Self {
            hypercore: Some(hypercore),
        }
    }

    #[must_use]
    pub fn hypercore(&self) -> Option<&'a HttpClient> {
        self.hypercore
    }
}

impl fmt::Debug for CommandClients<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandClients")
            .field("has_hypercore", &self.hypercore.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutputContext {
    format: OutputFormat,
    json_options: JsonRenderOptions,
}

impl CommandOutputContext {
    #[must_use]
    pub fn new(
        format: OutputFormat,
        select: Option<&str>,
        results_only: bool,
        max_results: Option<usize>,
    ) -> Self {
        Self {
            format,
            json_options: JsonRenderOptions::from_cli(select, results_only, max_results),
        }
    }

    #[must_use]
    pub fn format(&self) -> OutputFormat {
        self.format
    }

    #[must_use]
    pub fn render(&self, data: &dyn TableData) -> String {
        output::render_with_json_options(data, self.format, &self.json_options)
    }

    pub fn print(&self, data: &dyn TableData, duration: Duration) {
        println!("{}", self.render(data));
        output::print_timing(duration);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTransportPolicy {
    CliProcess,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PayloadMetadata {
    present: bool,
}

impl PayloadMetadata {
    #[must_use]
    pub fn from_presence(present: bool) -> Self {
        Self { present }
    }

    #[must_use]
    pub fn is_present(&self) -> bool {
        self.present
    }
}

#[derive(Debug, Clone)]
pub struct CommandContext<'a> {
    network: String,
    api_base_url: String,
    clients: CommandClients<'a>,
    output: CommandOutputContext,
    account_selector: Option<String>,
    dry_run: bool,
    payload: PayloadMetadata,
    transport_policy: CommandTransportPolicy,
}

impl<'a> CommandContext<'a> {
    #[must_use]
    pub fn new(
        network: impl Into<String>,
        api_base_url: impl Into<String>,
        output: CommandOutputContext,
        transport_policy: CommandTransportPolicy,
    ) -> Self {
        Self {
            network: network.into(),
            api_base_url: api_base_url.into(),
            clients: CommandClients::empty(),
            output,
            account_selector: None,
            dry_run: false,
            payload: PayloadMetadata::default(),
            transport_policy,
        }
    }

    #[must_use]
    pub fn with_clients(mut self, clients: CommandClients<'a>) -> Self {
        self.clients = clients;
        self
    }

    #[must_use]
    pub fn with_account_selector(mut self, account_selector: Option<&str>) -> Self {
        self.account_selector = account_selector.map(str::to_string);
        self
    }

    #[must_use]
    pub fn with_dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    #[must_use]
    pub fn with_payload(mut self, payload: PayloadMetadata) -> Self {
        self.payload = payload;
        self
    }

    #[must_use]
    pub fn network(&self) -> &str {
        &self.network
    }

    #[must_use]
    pub fn api_base_url(&self) -> &str {
        &self.api_base_url
    }

    #[must_use]
    pub fn hypercore_client(&self) -> Option<&'a HttpClient> {
        self.clients.hypercore()
    }

    #[must_use]
    pub fn output(&self) -> &CommandOutputContext {
        &self.output
    }

    #[must_use]
    pub fn account_selector(&self) -> Option<&str> {
        self.account_selector.as_deref()
    }

    #[must_use]
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }

    #[must_use]
    pub fn payload(&self) -> PayloadMetadata {
        self.payload
    }

    #[must_use]
    pub fn transport_policy(&self) -> CommandTransportPolicy {
        self.transport_policy
    }

    #[must_use]
    pub fn render(&self, data: &dyn TableData) -> String {
        self.output.render(data)
    }

    pub fn print(&self, data: &dyn TableData, duration: Duration) {
        self.output.print(data, duration);
    }
}
