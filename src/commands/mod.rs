//! Shared command helpers and asset resolution.
//!
//! This module provides:
//! - Asset name parsing (perps, spot, HIP-3 formats)
//! - Hyperliquid metadata loading via `hypersdk`
//! - 60-second metadata caching
//! - Fuzzy matching for "did you mean?" suggestions
//! - Shared constants

pub mod account;
pub(crate) mod actions;
pub mod api_wallet;
pub mod asset;
pub mod borrowlend;
pub mod builder;
pub mod feedback;
pub mod meta;
pub mod orderbook;
pub mod orders;
pub mod outcomes;
pub mod perps;
pub mod positions;
pub mod prio;
pub mod referral;
pub mod schema;
pub mod setup;
pub mod spot;
pub(crate) mod spot_balances;
pub mod staking;
pub mod status;
pub mod subaccounts;
pub mod transfers;
pub mod vaults;
pub mod wallet;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use hypersdk::hypercore::{self, Chain, HttpClient, PerpMarket, SpotMarket};
use serde::{Deserialize, Serialize};
use strsim::levenshtein;

use crate::config::{self, Network};
use crate::errors::CliError;
use crate::response_sanitization::labelled_untrusted_text;

/// Metadata cache lifetime for exchange metadata.
pub const METADATA_TTL: Duration = Duration::from_secs(60);

/// Parsed user asset input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetQuery {
    /// Default perpetual market symbol, e.g. `BTC`.
    Perp(String),
    /// Spot trading pair, e.g. `PURR/USDC`.
    Spot(String),
    /// HIP-3 DEX-qualified perpetual symbol, e.g. `dex:TOKEN`.
    Hip3 { dex: String, token: String },
    /// Outcome market notation, e.g. `#10` or `+10`.
    Outcome(String),
}

/// Parse the asset formats supported by the CLI.
///
/// Formats:
/// - `BTC` => default perpetual market
/// - `PURR/USDC` => spot pair
/// - `dex:TOKEN` => HIP-3 DEX perpetual market
/// - `#10` / `+10` => outcome market notation
#[must_use]
pub fn parse_asset_query(input: &str) -> AssetQuery {
    let trimmed = input.trim();

    if trimmed.starts_with('#') || trimmed.starts_with('+') {
        return AssetQuery::Outcome(trimmed.to_string());
    }

    if let Some((dex, token)) = trimmed.split_once(':') {
        return AssetQuery::Hip3 {
            dex: dex.trim().to_string(),
            token: token.trim().to_string(),
        };
    }

    if trimmed.contains('/') {
        AssetQuery::Spot(trimmed.to_string())
    } else {
        AssetQuery::Perp(trimmed.to_string())
    }
}

/// Perpetual asset metadata used for resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerpAsset {
    pub name: String,
    pub index: usize,
    pub dex: Option<String>,
    pub sz_decimals: u32,
    pub collateral: String,
}

impl PerpAsset {
    /// Build a default Hyperliquid perpetual asset.
    #[must_use]
    pub fn default_dex(name: impl Into<String>, index: usize) -> Self {
        Self {
            name: name.into(),
            index,
            dex: None,
            sz_decimals: 8,
            collateral: "USDC".to_string(),
        }
    }

    /// Build a HIP-3 DEX-qualified perpetual asset.
    #[must_use]
    pub fn hip3(dex: impl Into<String>, name: impl Into<String>, index: usize) -> Self {
        Self {
            name: name.into(),
            index,
            dex: Some(dex.into()),
            sz_decimals: 8,
            collateral: "USDC".to_string(),
        }
    }

    fn from_market(market: PerpMarket, dex: Option<String>) -> Self {
        let mut name = market.name;

        if let Some(dex_name) = dex.as_deref() {
            let prefix = format!("{dex_name}:");
            if name.starts_with(&prefix) {
                name = name[prefix.len()..].to_string();
            }
        }

        Self {
            name,
            index: market.index,
            dex,
            sz_decimals: u32::try_from(market.sz_decimals).unwrap_or_default(),
            collateral: market.collateral.name,
        }
    }

    fn display_name(&self) -> String {
        match &self.dex {
            Some(dex) => format!("{dex}:{}", self.name),
            None => self.name.clone(),
        }
    }
}

/// Spot asset metadata used for resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotAsset {
    pub symbol: String,
    pub index: usize,
    pub base: String,
    pub quote: String,
    pub base_sz_decimals: u32,
}

impl SpotAsset {
    /// Build spot asset metadata from pair details.
    #[must_use]
    pub fn new(
        symbol: impl Into<String>,
        index: usize,
        base: impl Into<String>,
        quote: impl Into<String>,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            index,
            base: base.into(),
            quote: quote.into(),
            base_sz_decimals: 8,
        }
    }

    fn from_market(market: SpotMarket) -> Self {
        let base = market.base().name.clone();
        let quote = market.quote().name.clone();
        Self {
            symbol: market.symbol(),
            index: market.index,
            base,
            quote,
            base_sz_decimals: u32::try_from(market.base().sz_decimals).unwrap_or_default(),
        }
    }

    #[must_use]
    pub fn info_coin(&self) -> String {
        if self.index == 10_000 {
            self.symbol.clone()
        } else {
            format!("@{}", self.index.saturating_sub(10_000))
        }
    }
}

/// Exchange metadata used by the resolver.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetMetadata {
    perps: Vec<PerpAsset>,
    spots: Vec<SpotAsset>,
}

impl AssetMetadata {
    /// Build metadata from already-loaded assets.
    #[must_use]
    pub fn from_assets(perps: Vec<PerpAsset>, spots: Vec<SpotAsset>) -> Self {
        Self { perps, spots }
    }

    /// Load all asset metadata from Hyperliquid using `hypersdk`.
    pub async fn load(client: &HttpClient) -> Result<Self, CliError> {
        let default_perps = client.perps().await.map_err(map_api_error)?;
        let spots = match client.spot().await {
            Ok(spots) => spots.into_iter().map(SpotAsset::from_market).collect(),
            Err(spot_err) => load_raw_spot_markets(client)
                .await
                .map(|markets| {
                    markets
                        .into_iter()
                        .map(SpotAsset::from_raw_market)
                        .collect()
                })
                .map_err(|_| map_api_error(spot_err))?,
        };
        let perps = default_perps
            .into_iter()
            .map(|market| PerpAsset::from_market(market, None))
            .collect::<Vec<_>>();

        Ok(Self { perps, spots })
    }

    /// Default perpetual assets loaded for low-latency resolver paths.
    #[must_use]
    pub fn perps(&self) -> &[PerpAsset] {
        &self.perps
    }

    /// Spot market assets.
    #[must_use]
    pub fn spots(&self) -> &[SpotAsset] {
        &self.spots
    }

    fn all_candidate_names(&self) -> Vec<String> {
        self.perps
            .iter()
            .map(PerpAsset::display_name)
            .chain(self.spots.iter().map(|spot| spot.symbol.clone()))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
enum SpotMetaInfoRequest {
    SpotMeta,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawSpotMeta {
    universe: Vec<RawSpotUniverseItem>,
    tokens: Vec<RawSpotToken>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RawSpotUniverseItem {
    #[serde(default)]
    tokens: Vec<u32>,
    index: usize,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RawSpotToken {
    name: String,
    index: u32,
    sz_decimals: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawSpotMarket {
    pub symbol: String,
    pub index: usize,
    pub base: String,
    pub quote: String,
    pub base_sz_decimals: i64,
    pub quote_sz_decimals: i64,
}

impl SpotAsset {
    fn from_raw_market(market: RawSpotMarket) -> Self {
        Self {
            symbol: market.symbol,
            index: market.index,
            base: market.base,
            quote: market.quote,
            base_sz_decimals: u32::try_from(market.base_sz_decimals).unwrap_or_default(),
        }
    }
}

/// Load spot markets directly from raw `spotMeta`, skipping entries with broken token refs.
pub(crate) async fn load_raw_spot_markets(
    client: &HttpClient,
) -> Result<Vec<RawSpotMarket>, CliError> {
    let api_url = raw_info_base_url(client)?;
    load_raw_spot_markets_from_url(api_url).await
}

pub(crate) async fn load_raw_spot_markets_from_url(
    mut api_url: reqwest::Url,
) -> Result<Vec<RawSpotMarket>, CliError> {
    api_url.set_path("/info");

    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .tcp_nodelay(true)
        .build()
        .map_err(|err| CliError::Internal(anyhow::Error::new(err)))?;
    let response = http_client
        .post(api_url)
        .json(&SpotMetaInfoRequest::SpotMeta)
        .send()
        .await
        .map_err(|err| map_api_error(anyhow::Error::new(err)))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| map_api_error(anyhow::Error::new(err)))?;

    if !status.is_success() {
        return Err(map_api_error(anyhow::anyhow!("HTTP {status} body={text}")));
    }

    let raw: RawSpotMeta = serde_json::from_str(&text).map_err(|err| {
        CliError::Internal(anyhow::anyhow!(
            "decode failed while loading raw spot metadata: {err}; body={text}"
        ))
    })?;
    Ok(raw_spot_markets(raw))
}

pub(crate) fn raw_info_base_url(client: &HttpClient) -> Result<reqwest::Url, CliError> {
    let network = if client.chain().is_mainnet() {
        Network::Mainnet
    } else {
        Network::Testnet
    };

    if let Some(api_base_url) = config::resolve_api_base_url_override_for_network(network)
        .map_err(crate::errors::from_anyhow)?
    {
        Ok(api_base_url)
    } else if client.chain().is_mainnet() {
        Ok(hypercore::mainnet_url())
    } else {
        Ok(hypercore::testnet_url())
    }
}

fn raw_spot_markets(raw: RawSpotMeta) -> Vec<RawSpotMarket> {
    raw.universe
        .into_iter()
        .filter_map(|item| {
            let base_ref = *item.tokens.first()?;
            let quote_ref = *item.tokens.get(1)?;
            let base = raw_spot_token(&raw.tokens, base_ref)?;
            let quote = raw_spot_token(&raw.tokens, quote_ref)?;

            Some(RawSpotMarket {
                symbol: format!("{}/{}", base.name, quote.name),
                index: 10_000 + item.index,
                base: base.name.clone(),
                quote: quote.name.clone(),
                base_sz_decimals: base.sz_decimals,
                quote_sz_decimals: quote.sz_decimals,
            })
        })
        .collect()
}

fn raw_spot_token(tokens: &[RawSpotToken], token_ref: u32) -> Option<&RawSpotToken> {
    tokens.iter().find(|token| token.index == token_ref)
}

/// A metadata snapshot with the time it was fetched.
#[derive(Debug, Clone)]
pub struct CachedMetadata {
    metadata: AssetMetadata,
    fetched_at: Instant,
}

impl CachedMetadata {
    /// Create a cached metadata entry at a known instant.
    #[must_use]
    pub fn new(metadata: AssetMetadata, fetched_at: Instant) -> Self {
        Self {
            metadata,
            fetched_at,
        }
    }

    /// Return true while the entry is inside the 60-second freshness window.
    #[must_use]
    pub fn is_fresh_at(&self, now: Instant) -> bool {
        now.duration_since(self.fetched_at) <= METADATA_TTL
    }

    fn metadata(&self) -> AssetMetadata {
        self.metadata.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum MetadataCacheKey {
    Mainnet,
    Testnet,
}

impl From<Chain> for MetadataCacheKey {
    fn from(chain: Chain) -> Self {
        match chain {
            Chain::Mainnet => Self::Mainnet,
            Chain::Testnet => Self::Testnet,
        }
    }
}

/// Thread-safe 60-second metadata cache keyed by Hyperliquid network.
#[derive(Debug, Default)]
pub struct MetadataCache {
    cached: Mutex<HashMap<MetadataCacheKey, CachedMetadata>>,
}

impl MetadataCache {
    /// Create an empty metadata cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get fresh metadata from cache, or load it through `hypersdk`.
    pub async fn get_or_load(
        &self,
        chain: Chain,
        client: &HttpClient,
    ) -> Result<AssetMetadata, CliError> {
        if let Some(metadata) = self.fresh_metadata_for_chain(chain) {
            return Ok(metadata);
        }

        let metadata = AssetMetadata::load(client).await?;
        self.store_for_chain(chain, metadata.clone(), Instant::now());
        Ok(metadata)
    }

    fn store_for_chain(&self, chain: Chain, metadata: AssetMetadata, fetched_at: Instant) {
        self.cached
            .lock()
            .expect("metadata cache lock poisoned")
            .insert(
                MetadataCacheKey::from(chain),
                CachedMetadata::new(metadata, fetched_at),
            );
    }

    fn fresh_metadata_for_chain(&self, chain: Chain) -> Option<AssetMetadata> {
        self.fresh_metadata_for_chain_at(chain, Instant::now())
    }

    fn fresh_metadata_for_chain_at(&self, chain: Chain, now: Instant) -> Option<AssetMetadata> {
        self.cached
            .lock()
            .expect("metadata cache lock poisoned")
            .get(&MetadataCacheKey::from(chain))
            .filter(|cached| cached.is_fresh_at(now))
            .map(CachedMetadata::metadata)
    }
}

/// Resolved internal asset reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedAsset {
    /// Perpetual market, including HIP-3 markets when `dex` is set.
    Perp {
        name: String,
        index: usize,
        dex: Option<String>,
        sz_decimals: u32,
        collateral: String,
    },
    /// Spot market pair.
    Spot {
        symbol: String,
        index: usize,
        base: String,
        quote: String,
        base_sz_decimals: u32,
    },
}

/// Asset resolver over a metadata snapshot.
#[derive(Debug, Clone)]
pub struct AssetResolver {
    metadata: AssetMetadata,
}

impl AssetResolver {
    /// Create a resolver from loaded metadata.
    #[must_use]
    pub fn new(metadata: AssetMetadata) -> Self {
        Self { metadata }
    }

    /// Resolve according to the user's asset format.
    pub fn resolve(&self, input: &str) -> Result<ResolvedAsset, CliError> {
        match parse_asset_query(input) {
            AssetQuery::Perp(_) | AssetQuery::Hip3 { .. } => self.resolve_perp(input),
            AssetQuery::Spot(_) => self.resolve_spot(input),
            AssetQuery::Outcome(_) => {
                Err(crate::commands::outcomes::unsupported_outcome_trading_error(input))
            }
        }
    }

    /// Resolve a perpetual market symbol or HIP-3 `dex:TOKEN` input.
    pub fn resolve_perp(&self, input: &str) -> Result<ResolvedAsset, CliError> {
        match parse_asset_query(input) {
            AssetQuery::Hip3 { dex, token } => self.resolve_hip3(input, &dex, &token),
            AssetQuery::Perp(symbol) => self.resolve_default_perp(input, &symbol),
            AssetQuery::Outcome(_) => {
                Err(crate::commands::outcomes::unsupported_outcome_trading_error(input))
            }
            AssetQuery::Spot(_) => Err(self.not_found(input, self.perp_candidate_names())),
        }
    }

    /// Resolve a spot pair input.
    pub fn resolve_spot(&self, input: &str) -> Result<ResolvedAsset, CliError> {
        let pair = match parse_asset_query(input) {
            AssetQuery::Spot(pair) => pair,
            AssetQuery::Outcome(_) => {
                return Err(crate::commands::outcomes::unsupported_outcome_trading_error(input));
            }
            _ => return Err(self.not_found(input, self.spot_candidate_names())),
        };

        self.metadata
            .spots
            .iter()
            .find(|spot| eq_asset_name(&spot.symbol, &pair))
            .map(|spot| ResolvedAsset::Spot {
                symbol: spot.symbol.clone(),
                index: spot.index,
                base: spot.base.clone(),
                quote: spot.quote.clone(),
                base_sz_decimals: spot.base_sz_decimals,
            })
            .ok_or_else(|| self.not_found(input, self.spot_candidate_names()))
    }

    #[must_use]
    pub fn spot_asset_index_for_internal_id(&self, internal_id: usize) -> Option<usize> {
        self.metadata
            .spots
            .iter()
            .find(|spot| spot.index.saturating_sub(10_000) == internal_id)
            .map(|spot| spot.index)
    }

    #[must_use]
    pub fn spot_symbol_for_internal_id(&self, internal_id: usize) -> Option<&str> {
        self.metadata
            .spots
            .iter()
            .find(|spot| spot.index.saturating_sub(10_000) == internal_id)
            .map(|spot| spot.symbol.as_str())
    }

    #[must_use]
    pub fn display_coin(&self, coin: &str) -> String {
        coin.trim()
            .strip_prefix('@')
            .and_then(|value| value.parse::<usize>().ok())
            .and_then(|internal_id| self.spot_symbol_for_internal_id(internal_id))
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| coin.to_string())
    }

    #[must_use]
    pub fn perp_by_protocol_asset_id(&self, asset_id: usize) -> Option<ResolvedAsset> {
        self.metadata
            .perps
            .iter()
            .find(|asset| asset.index == asset_id)
            .map(|asset| ResolvedAsset::Perp {
                name: asset.name.clone(),
                index: asset.index,
                dex: asset.dex.clone(),
                sz_decimals: asset.sz_decimals,
                collateral: asset.collateral.clone(),
            })
    }

    #[must_use]
    pub fn spot_by_protocol_asset_id(&self, asset_id: usize) -> Option<ResolvedAsset> {
        self.metadata
            .spots
            .iter()
            .find(|spot| spot.index == asset_id)
            .map(|spot| ResolvedAsset::Spot {
                symbol: spot.symbol.clone(),
                index: spot.index,
                base: spot.base.clone(),
                quote: spot.quote.clone(),
                base_sz_decimals: spot.base_sz_decimals,
            })
    }

    #[must_use]
    pub fn perps(&self) -> &[PerpAsset] {
        self.metadata.perps()
    }

    #[must_use]
    pub fn spots(&self) -> &[SpotAsset] {
        self.metadata.spots()
    }

    fn resolve_default_perp(&self, input: &str, symbol: &str) -> Result<ResolvedAsset, CliError> {
        self.metadata
            .perps
            .iter()
            .find(|asset| asset.dex.is_none() && eq_asset_name(&asset.name, symbol))
            .map(|asset| ResolvedAsset::Perp {
                name: asset.name.clone(),
                index: asset.index,
                dex: None,
                sz_decimals: asset.sz_decimals,
                collateral: asset.collateral.clone(),
            })
            .ok_or_else(|| self.not_found(input, self.perp_candidate_names()))
    }

    fn resolve_hip3(&self, input: &str, dex: &str, token: &str) -> Result<ResolvedAsset, CliError> {
        self.metadata
            .perps
            .iter()
            .find(|asset| {
                asset
                    .dex
                    .as_deref()
                    .is_some_and(|asset_dex| eq_asset_name(asset_dex, dex))
                    && eq_asset_name(&asset.name, token)
            })
            .map(|asset| ResolvedAsset::Perp {
                name: asset.name.clone(),
                index: asset.index,
                dex: asset.dex.clone(),
                sz_decimals: asset.sz_decimals,
                collateral: asset.collateral.clone(),
            })
            .ok_or_else(|| self.not_found(input, self.perp_candidate_names()))
    }

    fn perp_candidate_names(&self) -> Vec<String> {
        self.metadata
            .perps
            .iter()
            .map(PerpAsset::display_name)
            .collect()
    }

    fn spot_candidate_names(&self) -> Vec<String> {
        self.metadata
            .spots
            .iter()
            .map(|spot| spot.symbol.clone())
            .collect()
    }

    fn not_found(&self, input: &str, candidates: Vec<String>) -> CliError {
        let matches = suggestions(input, candidates);
        if matches.is_empty() {
            CliError::AssetNotFoundNoSuggestion {
                asset: input.to_string(),
            }
        } else {
            CliError::AssetNotFound {
                asset: input.to_string(),
                suggestions: matches.join(", "),
            }
        }
    }

    /// Suggestions across all known asset names.
    #[must_use]
    pub fn suggestions(&self, input: &str) -> Vec<String> {
        suggestions(input, self.metadata.all_candidate_names())
    }

    #[must_use]
    pub fn with_perp_asset(&self, asset: PerpAsset) -> Self {
        let mut metadata = self.metadata.clone();
        if !metadata.perps.iter().any(|existing| {
            existing.index == asset.index
                || (existing.dex.as_deref().is_some_and(|dex| {
                    asset
                        .dex
                        .as_deref()
                        .is_some_and(|other| eq_asset_name(dex, other))
                }) && eq_asset_name(&existing.name, &asset.name))
        }) {
            metadata.perps.push(asset);
        }
        Self { metadata }
    }
}

/// Return up to three close fuzzy matches by Levenshtein distance.
#[must_use]
pub fn suggestions<I, S>(input: &str, candidates: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let normalized_input = normalize_asset_name(input);
    let strict_distance = strict_suggestion_threshold(&normalized_input);
    let max_distance = suggestion_threshold(&normalized_input);

    let mut scored = candidates
        .into_iter()
        .enumerate()
        .filter_map(|candidate| {
            let (position, candidate) = candidate;
            let candidate = candidate.as_ref().trim().to_string();
            if candidate.is_empty() {
                return None;
            }

            let normalized_candidate = normalize_asset_name(&candidate);
            let distance = levenshtein(&normalized_input, &normalized_candidate);
            (distance <= max_distance
                && is_rankable_suggestion(
                    &normalized_input,
                    &normalized_candidate,
                    distance,
                    strict_distance,
                ))
            .then_some((distance, position, candidate))
        })
        .collect::<Vec<_>>();

    scored.sort_by(
        |(left_distance, left_position, left), (right_distance, right_position, right)| {
            left_distance
                .cmp(right_distance)
                .then_with(|| left_position.cmp(right_position))
                .then_with(|| left.cmp(right))
        },
    );

    let mut results = Vec::new();
    for (_, _, candidate) in scored {
        if !results
            .iter()
            .any(|existing: &String| eq_asset_name(existing, &candidate))
        {
            results.push(candidate);
        }

        if results.len() == 3 {
            break;
        }
    }

    results
}

fn suggestion_threshold(input: &str) -> usize {
    let input_len = input.len();
    if input_len <= 2 {
        3
    } else if input_len >= 8 {
        input_len - 1
    } else {
        3.max(input_len / 2)
    }
}

fn strict_suggestion_threshold(input: &str) -> usize {
    let input_len = input.len();
    if input_len <= 2 {
        3
    } else {
        3.max(input_len / 2)
    }
}

fn is_rankable_suggestion(
    input: &str,
    candidate: &str,
    distance: usize,
    strict_distance: usize,
) -> bool {
    if distance <= strict_distance {
        return true;
    }

    let input_chars = input.chars().collect::<std::collections::HashSet<_>>();
    input_chars.len() > 1 && candidate.chars().any(|ch| input_chars.contains(&ch))
}

fn eq_asset_name(left: &str, right: &str) -> bool {
    normalize_asset_name(left) == normalize_asset_name(right)
}

fn normalize_asset_name(input: &str) -> String {
    input.trim().to_ascii_uppercase()
}

pub(crate) fn map_api_error(err: anyhow::Error) -> CliError {
    match err.downcast::<hypersdk::hypercore::Error>() {
        Ok(core_err) => CliError::from(core_err),
        Err(err) => {
            let raw_message = err.to_string();
            let message = labelled_untrusted_text(&raw_message);
            let lower = message.to_lowercase();

            if lower.contains("rate limit")
                || lower.contains("rate-limit")
                || lower.contains("too many requests")
                || lower.contains("http 429")
                || lower.contains("429 too many")
            {
                CliError::RateLimited
            } else if lower.contains("timeout")
                || lower.contains("timed out")
                || lower.contains("error sending request")
                || lower.contains("connection")
                || lower.contains("dns")
                || lower.contains("service unavailable")
                || lower.contains("http 5")
            {
                CliError::Unavailable(format!(
                    "Check your network connection while loading asset metadata. {message}"
                ))
            } else {
                CliError::Internal(anyhow::anyhow!("{message}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_metadata_request_errors_to_unavailable() {
        let err = map_api_error(anyhow::anyhow!("error sending request: dns error"));

        assert_eq!(err.exit_code(), 12);
        assert!(matches!(err, CliError::Unavailable(_)));
    }

    #[test]
    fn maps_metadata_rate_limit_errors_to_rate_limited() {
        let err = map_api_error(anyhow::anyhow!("rate limit exceeded"));

        assert_eq!(err.exit_code(), 11);
        assert!(matches!(err, CliError::RateLimited));
    }

    #[test]
    fn maps_metadata_http_429_errors_to_rate_limited() {
        let err = map_api_error(anyhow::anyhow!("HTTP 429 Too Many Requests"));

        assert_eq!(err.exit_code(), 11);
        assert!(matches!(err, CliError::RateLimited));
    }

    #[test]
    fn maps_metadata_internal_errors_with_untrusted_label() {
        let err = map_api_error(anyhow::anyhow!("HTTP 418 body=<script>"));

        assert_eq!(err.exit_code(), 1);
        assert!(err.to_string().contains("[untrusted remote data]"));
        assert!(err.to_string().contains("HTTP 418"));
    }

    #[test]
    fn raw_spot_markets_skip_unresolvable_token_references() {
        let raw: RawSpotMeta = serde_json::from_value(serde_json::json!({
            "tokens": [
                {"name": "USDC", "index": 0, "szDecimals": 6},
                {"name": "PURR", "index": 1, "szDecimals": 0}
            ],
            "universe": [
                {"tokens": [1, 0], "index": 0, "name": "PURR/USDC"},
                {"tokens": [9, 0], "index": 1, "name": "BADBASE/USDC"},
                {"tokens": [1, 8], "index": 2, "name": "PURR/BADQUOTE"},
                {"tokens": [1], "index": 3, "name": "SHORT"}
            ]
        }))
        .unwrap();

        let markets = raw_spot_markets(raw);

        assert_eq!(markets.len(), 1);
        assert_eq!(markets[0].symbol, "PURR/USDC");
        assert_eq!(markets[0].index, 10_000);
        assert_eq!(markets[0].base_sz_decimals, 0);
        assert_eq!(markets[0].quote_sz_decimals, 6);
    }

    fn fixture_metadata(asset_name: &str, asset_index: usize) -> AssetMetadata {
        AssetMetadata::from_assets(
            vec![PerpAsset::default_dex(asset_name, asset_index)],
            Vec::new(),
        )
    }

    #[test]
    fn metadata_cache_entries_are_independent_per_chain() {
        let cache = MetadataCache::new();
        let now = Instant::now();
        let mainnet_metadata = fixture_metadata("BTC", 0);
        let testnet_metadata = fixture_metadata("TESTBTC", 10);

        cache.store_for_chain(Chain::Mainnet, mainnet_metadata.clone(), now);
        cache.store_for_chain(Chain::Testnet, testnet_metadata.clone(), now);

        assert_eq!(
            cache.fresh_metadata_for_chain_at(Chain::Mainnet, now),
            Some(mainnet_metadata)
        );
        assert_eq!(
            cache.fresh_metadata_for_chain_at(Chain::Testnet, now),
            Some(testnet_metadata)
        );
    }

    #[test]
    fn metadata_cache_ttl_is_applied_per_chain() {
        let cache = MetadataCache::new();
        let now = Instant::now();
        let stale_mainnet_fetched_at = now - METADATA_TTL - Duration::from_secs(1);
        let fresh_testnet_fetched_at = now - METADATA_TTL + Duration::from_secs(1);
        let mainnet_metadata = fixture_metadata("BTC", 0);
        let testnet_metadata = fixture_metadata("TESTBTC", 10);

        cache.store_for_chain(Chain::Mainnet, mainnet_metadata, stale_mainnet_fetched_at);
        cache.store_for_chain(
            Chain::Testnet,
            testnet_metadata.clone(),
            fresh_testnet_fetched_at,
        );

        assert_eq!(cache.fresh_metadata_for_chain_at(Chain::Mainnet, now), None);
        assert_eq!(
            cache.fresh_metadata_for_chain_at(Chain::Testnet, now),
            Some(testnet_metadata)
        );
    }
}
