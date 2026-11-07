//! Everything one run of the CLI needs, built once in `main` and passed by
//! reference to every handler.
//!
//! The output session, the prompting mode, the dry-run and payload inputs and
//! the asset metadata cache are known at startup. The network, API endpoint,
//! HTTP client and signer depend on the environment and config file, so they
//! resolve on first use ([`Invocation::remote`]) and local-only commands such as
//! `wallet list` never read them.

use std::path::PathBuf;
use std::sync::OnceLock;

use hypersdk::hypercore::Chain;

use crate::assets::{AssetResolver, MetadataCache};
use crate::auth::{ResolvedSigner, SignerSelection};
use crate::dry_run::{DryRunEnvelope, DryRunRunContext};
use crate::errors::CliError;
use crate::http_api::ApiClient;
use crate::mode::Mode;
use crate::network::Network;
use crate::output::{OutputFormat, Session, TableData};
use crate::settings::Settings;

/// Signer flags exactly as parsed from the command line.
#[derive(Debug, Clone, Default)]
pub struct SignerFlags {
    pub private_key: Option<String>,
    pub keystore: Option<PathBuf>,
    pub keystore_password: Option<String>,
    pub account: Option<String>,
    pub ows_signer: Option<String>,
}

/// The parsed global flags that shape an invocation.
#[derive(Debug, Clone, Default)]
pub struct InvocationFlags {
    pub testnet: bool,
    pub dry_run: bool,
    pub non_interactive: bool,
    /// `--payload-file -`: stdin carries the payload and cannot answer prompts.
    pub payload_from_stdin: bool,
    pub signer: SignerFlags,
}

/// Network facts resolved from flags, environment and config file.
pub struct Remote {
    settings: Settings,
    selection: SignerSelection,
    network: Network,
    api: ApiClient,
    signer: OnceLock<ResolvedSigner>,
}

impl std::fmt::Debug for Remote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Remote")
            .field("network", &self.network)
            .field("api", &self.api)
            .finish_non_exhaustive()
    }
}

impl Remote {
    fn resolve(flags: &InvocationFlags, announce_retries: bool) -> Result<Self, CliError> {
        let settings = Settings::from_process();
        let network = settings.network(flags.testnet)?;
        let signer = &flags.signer;
        let selection = SignerSelection::from_flags(
            &settings,
            signer.private_key.as_deref(),
            signer.keystore.clone(),
            signer.keystore_password.clone(),
            signer.account.clone(),
            signer.ows_signer.clone(),
        )?;
        let api = ApiClient::from_settings(&settings, network, announce_retries)?;
        Ok(Self {
            settings,
            selection,
            network,
            api,
            signer: OnceLock::new(),
        })
    }

    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    #[must_use]
    pub fn selection(&self) -> &SignerSelection {
        &self.selection
    }

    #[must_use]
    pub fn network(&self) -> Network {
        self.network
    }

    #[must_use]
    pub fn chain(&self) -> Chain {
        self.network.chain()
    }

    /// The one HTTP edge to the Hyperliquid API.
    #[must_use]
    pub fn api(&self) -> &ApiClient {
        &self.api
    }
}

/// The context of one CLI run.
#[derive(Debug)]
pub struct Invocation {
    session: Session,
    mode: Mode,
    flags: InvocationFlags,
    payload: Option<serde_json::Value>,
    dry_run_signs: Option<bool>,
    metadata: MetadataCache,
    remote: OnceLock<Remote>,
}

impl Invocation {
    #[must_use]
    pub fn new(session: Session, flags: InvocationFlags) -> Self {
        let mode = Mode::new(
            flags.non_interactive,
            session.format() == OutputFormat::Json,
            flags.payload_from_stdin,
            &session.env(),
        );
        Self {
            session,
            mode,
            flags,
            payload: None,
            dry_run_signs: None,
            metadata: MetadataCache::new(),
            remote: OnceLock::new(),
        }
    }

    // ── Output ──────────────────────────────────────────────────────────

    #[must_use]
    pub fn session(&self) -> &Session {
        &self.session
    }

    #[must_use]
    pub fn format(&self) -> OutputFormat {
        self.session.format()
    }

    /// Print the answer, then the "Completed in" footer for interactive runs.
    pub fn print(&self, data: &dyn TableData) {
        self.session.print(data);
    }

    /// Print the answer without the timing footer.
    pub fn print_untimed(&self, data: &dyn TableData) {
        self.session.print_untimed(data);
    }

    // ── Prompting, dry run and payload ──────────────────────────────────

    #[must_use]
    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    #[must_use]
    pub fn dry_run(&self) -> bool {
        self.flags.dry_run
    }

    /// The raw `--payload-json` / `--payload-file` document, when one was given.
    #[must_use]
    pub fn payload(&self) -> Option<&serde_json::Value> {
        self.payload.as_ref()
    }

    pub fn set_payload(&mut self, payload: Option<serde_json::Value>) {
        self.payload = payload;
    }

    /// Dry-run envelopes from here on name the network, and the signer when
    /// `signs` is set. Without this call (local-only commands) they name neither.
    pub fn attach_dry_run_context(&mut self, signs: bool) {
        self.dry_run_signs = Some(signs);
    }

    /// Print a dry-run envelope with the run's network and signer.
    pub fn print_dry_run(&self, envelope: DryRunEnvelope) {
        let context = self.dry_run_signs.and_then(|signs| {
            let remote = self.remote.get()?;
            let signer = signs.then(|| {
                self.resolve_signer()
                    .map(|signer| signer.address().to_string())
                    .map_err(|err| err.to_string())
            });
            Some(DryRunRunContext::new(remote.network(), signer))
        });
        self.session
            .print_untimed(&envelope.with_run_context(context));
    }

    // ── Network, signer and assets ──────────────────────────────────────

    /// The network facts, resolved on first use. A failed resolution is not cached.
    pub fn remote(&self) -> Result<&Remote, CliError> {
        if let Some(remote) = self.remote.get() {
            return Ok(remote);
        }
        let remote = Remote::resolve(&self.flags, self.announces_retries())?;
        Ok(self.remote.get_or_init(|| remote))
    }

    /// An API client for `network`, for `setup`, which may pick another network
    /// than the run resolved.
    pub fn api_client_for(&self, network: Network) -> Result<ApiClient, CliError> {
        ApiClient::from_settings(self.remote()?.settings(), network, self.announces_retries())
    }

    /// Read retries are announced to a person at a terminal, never to JSON,
    /// agent, CI, non-interactive or payload-on-stdin runs.
    fn announces_retries(&self) -> bool {
        self.session.env().stderr_tty && self.mode.can_prompt()
    }

    /// The selected signer, resolved once per run. A failed resolution is not cached.
    pub fn resolve_signer(&self) -> Result<ResolvedSigner, anyhow::Error> {
        let remote = self.remote()?;
        if let Some(signer) = remote.signer.get() {
            return Ok(signer.clone());
        }
        let signer = crate::auth::resolve_signer(&remote.selection, &remote.settings, &self.mode)?;
        Ok(remote.signer.get_or_init(|| signer).clone())
    }

    /// The account whose data a read command queries: the explicit selector, else
    /// the selected signer's query address.
    pub fn resolve_user(&self, selector: Option<&str>) -> Result<hypersdk::Address, anyhow::Error> {
        match selector {
            Some(selector) => self.resolve_protocol_user(selector),
            None => Ok(self.resolve_signer()?.query_address()),
        }
    }

    pub fn resolve_protocol_user(
        &self,
        selector: &str,
    ) -> Result<hypersdk::Address, anyhow::Error> {
        Ok(crate::resolvers::resolve_protocol_user_address(
            selector,
            self.remote()?.settings(),
            &self.mode,
        )?
        .address())
    }

    pub fn resolve_acting_account(
        &self,
        selector: &str,
    ) -> Result<hypersdk::Address, anyhow::Error> {
        Ok(crate::resolvers::resolve_acting_account_address(
            selector,
            self.remote()?.settings(),
            &self.mode,
        )?
        .address())
    }

    /// Asset metadata through the per-run cache.
    pub async fn asset_resolver(&self) -> Result<AssetResolver, CliError> {
        let metadata = self.metadata.get_or_load(self.remote()?.api()).await?;
        Ok(AssetResolver::new(metadata))
    }

    /// Like [`Self::asset_resolver`], falling back to the raw exchange metadata
    /// when the SDK cannot decode the universe.
    pub async fn trading_resolver(&self) -> Result<AssetResolver, CliError> {
        match self.asset_resolver().await {
            Ok(resolver) => Ok(resolver),
            Err(_) => AssetResolver::load_raw(self.remote()?.api()).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::JsonRenderOptions;
    use crate::output::terminal::OutputEnv;

    fn invocation(format: OutputFormat, env: OutputEnv, flags: InvocationFlags) -> Invocation {
        Invocation::new(
            Session::new(format, JsonRenderOptions::default(), env),
            flags,
        )
    }

    fn terminal() -> OutputEnv {
        OutputEnv {
            stdout_tty: true,
            stderr_tty: true,
            ..OutputEnv::default()
        }
    }

    #[test]
    fn retry_notices_are_for_a_person_at_a_terminal_who_may_be_prompted() {
        let flags = InvocationFlags::default();
        assert!(invocation(OutputFormat::Pretty, terminal(), flags.clone()).announces_retries());

        let cases = [
            (
                "stderr piped",
                OutputFormat::Pretty,
                OutputEnv {
                    stderr_tty: false,
                    ..terminal()
                },
                flags.clone(),
            ),
            ("json", OutputFormat::Json, terminal(), flags.clone()),
            (
                "agent",
                OutputFormat::Pretty,
                OutputEnv {
                    agent: true,
                    ..terminal()
                },
                flags.clone(),
            ),
            (
                "ci",
                OutputFormat::Pretty,
                OutputEnv {
                    ci: true,
                    ..terminal()
                },
                flags.clone(),
            ),
            (
                "non-interactive",
                OutputFormat::Pretty,
                terminal(),
                InvocationFlags {
                    non_interactive: true,
                    ..flags.clone()
                },
            ),
            (
                "payload on stdin",
                OutputFormat::Pretty,
                terminal(),
                InvocationFlags {
                    payload_from_stdin: true,
                    ..flags
                },
            ),
        ];
        for (name, format, env, flags) in cases {
            assert!(
                !invocation(format, env, flags).announces_retries(),
                "{name}"
            );
        }
    }
}
