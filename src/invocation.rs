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
use std::time::Duration;

use hypersdk::hypercore::{Chain, HttpClient};

use crate::assets::{AssetResolver, MetadataCache};
use crate::auth::{ResolvedSigner, SignerSelection};
use crate::dry_run::{DryRunEnvelope, DryRunRunContext};
use crate::errors::{self, CliError};
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
    api_base_url_override: Option<reqwest::Url>,
    api_base_url: String,
    client: HttpClient,
    signer: OnceLock<ResolvedSigner>,
}

impl std::fmt::Debug for Remote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Remote")
            .field("network", &self.network)
            .field("api_base_url", &self.api_base_url)
            .finish_non_exhaustive()
    }
}

impl Remote {
    fn resolve(flags: &InvocationFlags) -> Result<Self, CliError> {
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
        let api_base_url_override = settings.api_base_url_override(network)?;
        let client = network.http_client(api_base_url_override.clone());
        let api_base_url = api_base_url_override.as_ref().map_or_else(
            || network.api_base_url().to_string(),
            std::string::ToString::to_string,
        );
        Ok(Self {
            settings,
            selection,
            network,
            api_base_url_override,
            api_base_url,
            client,
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

    /// The URL the API client talks to: the loopback override, else the public endpoint.
    #[must_use]
    pub fn api_base_url(&self) -> &str {
        &self.api_base_url
    }

    #[must_use]
    pub fn client(&self) -> &HttpClient {
        &self.client
    }

    #[must_use]
    pub fn has_api_base_url_override(&self) -> bool {
        self.api_base_url_override.is_some()
    }

    /// Probe a loopback API override with an `allMids` read before any command uses it.
    pub async fn validate_api_override(&self) -> Result<(), CliError> {
        let Some(api_base_url) = self.api_base_url_override.as_ref() else {
            return Ok(());
        };

        let mut url = api_base_url.clone();
        url.set_path("/info");
        url.set_query(None);

        let response = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|err| CliError::Internal(anyhow::anyhow!(err)))?
            .post(url)
            .json(&serde_json::json!({ "type": "allMids" }))
            .send()
            .await
            .map_err(|err| errors::network_error(&err, Some(Duration::from_secs(5))))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|err| errors::network_error(&err, Some(Duration::from_secs(5))))?;

        if errors::http_response_indicates_rate_limit(status.as_u16(), &body) {
            return Err(CliError::rate_limited());
        }

        if !status.is_success() {
            return Err(CliError::api_status(status, &body));
        }

        Ok(())
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
        let remote = Remote::resolve(&self.flags)?;
        Ok(self.remote.get_or_init(|| remote))
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
        let remote = self.remote()?;
        let metadata = self
            .metadata
            .get_or_load(remote.chain(), remote.client())
            .await?;
        Ok(AssetResolver::new(metadata))
    }

    /// Like [`Self::asset_resolver`], falling back to the raw exchange metadata
    /// when the SDK cannot decode the universe.
    pub async fn trading_resolver(&self) -> Result<AssetResolver, CliError> {
        match self.asset_resolver().await {
            Ok(resolver) => Ok(resolver),
            Err(_) => AssetResolver::load_raw(self.remote()?.api_base_url()).await,
        }
    }
}
