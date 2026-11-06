//! Exchange metadata snapshots: the two loaders (hypersdk, raw `/info`), the
//! raw spot-meta fallback, the HIP-3 live lookup and the 60 s cache.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use hypersdk::hypercore::{self, Chain, Dex, HttpClient, PerpMarket, SpotMarket};
use serde::Deserialize;

use crate::commands::map_api_error;
use crate::errors::CliError;
use crate::http_api::post_info_json;
use crate::network::Network;
use crate::settings::Settings;

use super::asset_id;
use super::market::{PerpAsset, SpotAsset};
use super::query::eq_asset_name;

/// Metadata cache lifetime for exchange metadata.
pub const METADATA_TTL: Duration = Duration::from_secs(60);

/// Exchange metadata used by the resolver.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetMetadata {
    pub(super) perps: Vec<PerpAsset>,
    pub(super) spots: Vec<SpotAsset>,
}

impl AssetMetadata {
    #[must_use]
    pub fn from_assets(perps: Vec<PerpAsset>, spots: Vec<SpotAsset>) -> Self {
        Self { perps, spots }
    }

    /// Load default-dex perps and all spot markets through `hypersdk`.
    ///
    /// HIP-3 markets are not loaded here; they are looked up live on demand.
    pub async fn load(client: &HttpClient) -> Result<Self, CliError> {
        let default_perps = client.perps().await.map_err(map_api_error)?;
        let spots = load_spot_markets(client)
            .await?
            .into_iter()
            .map(spot_asset_from_listing)
            .collect::<Result<Vec<_>, _>>()?;
        let perps = default_perps
            .into_iter()
            .map(|market| perp_asset_from_market(market, None))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { perps, spots })
    }

    /// Load default-dex perps, every HIP-3 DEX and all spot markets from the raw
    /// `/info` endpoints.
    ///
    /// Used when the `hypersdk` loader cannot decode a network's metadata and by
    /// `asset search`/`asset decode`, which need every HIP-3 market.
    ///
    /// A HIP-3 DEX whose metadata fails to load is skipped, so it resolves as
    /// "asset not found" rather than failing the whole load.
    pub async fn load_raw(api_base_url: &str) -> Result<Self, CliError> {
        #[derive(Deserialize)]
        struct MetaResponse {
            universe: Vec<MetaPerp>,
            #[serde(rename = "collateralToken")]
            collateral_token: Option<usize>,
        }

        #[derive(Deserialize)]
        struct MetaPerp {
            name: String,
            #[serde(rename = "szDecimals")]
            sz_decimals: u32,
        }

        #[derive(Deserialize)]
        struct RawPerpDex {
            name: String,
        }

        let meta = post_info_json::<MetaResponse>(
            api_base_url,
            &serde_json::json!({ "type": "meta" }),
            "loading perpetual metadata",
        )
        .await?;
        let spot_meta = load_raw_spot_meta(api_base_url).await?;

        let default_collateral = spot_meta.collateral_name(meta.collateral_token);
        let mut perps = meta
            .universe
            .into_iter()
            .enumerate()
            .map(|(index, perp)| PerpAsset {
                name: perp.name,
                index,
                dex: None,
                sz_decimals: perp.sz_decimals,
                collateral: default_collateral.clone(),
            })
            .collect::<Vec<_>>();

        if let Ok(dexes) = post_info_json::<Vec<Option<RawPerpDex>>>(
            api_base_url,
            &serde_json::json!({ "type": "perpDexs" }),
            "loading HIP-3 DEX metadata",
        )
        .await
        {
            for (dex_slot, dex) in dexes.into_iter().enumerate() {
                let Some(dex) = dex else { continue };
                let Ok(meta) = post_info_json::<MetaResponse>(
                    api_base_url,
                    &serde_json::json!({ "type": "meta", "dex": dex.name.clone() }),
                    "loading HIP-3 perpetual metadata",
                )
                .await
                else {
                    continue;
                };
                let prefix = format!("{}:", dex.name);
                let collateral = spot_meta.collateral_name(meta.collateral_token);
                perps.extend(meta.universe.into_iter().enumerate().map(|(index, perp)| {
                    let name = perp
                        .name
                        .strip_prefix(&prefix)
                        .unwrap_or(perp.name.as_str())
                        .to_string();
                    PerpAsset {
                        name,
                        index: asset_id::hip3_asset_index(dex_slot, index),
                        dex: Some(dex.name.clone()),
                        sz_decimals: perp.sz_decimals,
                        collateral: collateral.clone(),
                    }
                }));
            }
        }

        let spots = spot_meta
            .into_listings()
            .into_iter()
            .map(spot_asset_from_listing)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { perps, spots })
    }

    #[must_use]
    pub fn perps(&self) -> &[PerpAsset] {
        &self.perps
    }

    #[must_use]
    pub fn spots(&self) -> &[SpotAsset] {
        &self.spots
    }

    pub(super) fn all_candidate_names(&self) -> Vec<String> {
        self.perps
            .iter()
            .map(PerpAsset::display_name)
            .chain(self.spots.iter().map(|spot| spot.symbol.clone()))
            .collect()
    }

    /// Whether `asset` is already listed, by protocol id or by DEX and name.
    pub(super) fn has_perp(&self, asset: &PerpAsset) -> bool {
        self.perps.iter().any(|existing| {
            existing.index == asset.index
                || (matches!(
                    (&existing.dex, &asset.dex),
                    (Some(left), Some(right)) if eq_asset_name(left, right)
                ) && eq_asset_name(&existing.name, &asset.name))
        })
    }
}

fn sz_decimals(value: i64, market: &str) -> Result<u32, CliError> {
    u32::try_from(value).map_err(|_| {
        CliError::Internal(anyhow::anyhow!(
            "exchange metadata for {market} has an invalid size precision: {value}"
        ))
    })
}

/// Convert an SDK perp market, dropping the `dex:` prefix HIP-3 names carry.
pub(super) fn perp_asset_from_market(
    market: PerpMarket,
    dex: Option<String>,
) -> Result<PerpAsset, CliError> {
    let name = match dex.as_deref() {
        Some(dex_name) => market
            .name
            .strip_prefix(&format!("{dex_name}:"))
            .map_or_else(|| market.name.clone(), ToString::to_string),
        None => market.name.clone(),
    };
    Ok(PerpAsset {
        sz_decimals: sz_decimals(market.sz_decimals, &market.name)?,
        name,
        index: market.index,
        dex,
        collateral: market.collateral.name,
    })
}

fn spot_asset_from_listing(listing: SpotListing) -> Result<SpotAsset, CliError> {
    Ok(SpotAsset {
        base_sz_decimals: sz_decimals(listing.base_sz_decimals, &listing.symbol)?,
        symbol: listing.symbol,
        index: listing.index,
        base: listing.base,
        quote: listing.quote,
    })
}

/// A spot market as listed by the exchange, whichever loader produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotListing {
    pub symbol: String,
    pub index: usize,
    pub base: String,
    pub quote: String,
    pub base_sz_decimals: i64,
    pub quote_sz_decimals: i64,
}

impl From<SpotMarket> for SpotListing {
    fn from(market: SpotMarket) -> Self {
        let [base, quote] = &market.tokens;
        Self {
            symbol: market.symbol(),
            index: market.index,
            base: base.name.clone(),
            quote: quote.name.clone(),
            base_sz_decimals: base.sz_decimals,
            quote_sz_decimals: quote.sz_decimals,
        }
    }
}

/// Load all spot markets through `hypersdk`, falling back to raw `spotMeta`
/// (which skips entries with broken token references) when the SDK cannot
/// decode it. A failed fallback reports the SDK error.
pub async fn load_spot_markets(client: &HttpClient) -> Result<Vec<SpotListing>, CliError> {
    match client.spot().await {
        Ok(markets) => Ok(markets.into_iter().map(SpotListing::from).collect()),
        Err(spot_err) => {
            let raw = async {
                let api_url = raw_info_base_url(client)?;
                Ok::<_, CliError>(load_raw_spot_meta(api_url.as_str()).await?.into_listings())
            };
            raw.await.map_err(|_| map_api_error(spot_err))
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RawSpotMeta {
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

impl RawSpotMeta {
    fn token(&self, token_ref: u32) -> Option<&RawSpotToken> {
        self.tokens.iter().find(|token| token.index == token_ref)
    }

    /// Name of a perp collateral token, defaulting to USDC when the exchange
    /// names none or it is unknown.
    fn collateral_name(&self, collateral_token: Option<usize>) -> String {
        collateral_token
            .and_then(|position| {
                u32::try_from(position)
                    .ok()
                    .and_then(|index| self.token(index))
                    .or_else(|| self.tokens.get(position))
            })
            .map_or_else(|| "USDC".to_string(), |token| token.name.clone())
    }

    fn into_listings(self) -> Vec<SpotListing> {
        self.universe
            .iter()
            .filter_map(|item| {
                let base = self.token(*item.tokens.first()?)?;
                let quote = self.token(*item.tokens.get(1)?)?;
                Some(SpotListing {
                    symbol: format!("{}/{}", base.name, quote.name),
                    index: asset_id::spot_asset_index(item.index),
                    base: base.name.clone(),
                    quote: quote.name.clone(),
                    base_sz_decimals: base.sz_decimals,
                    quote_sz_decimals: quote.sz_decimals,
                })
            })
            .collect()
    }
}

async fn load_raw_spot_meta(api_base_url: &str) -> Result<RawSpotMeta, CliError> {
    post_info_json(
        api_base_url,
        &serde_json::json!({ "type": "spotMeta" }),
        "loading raw spot metadata",
    )
    .await
}

/// The `/info` base URL for a client's chain, honouring the API override.
pub(crate) fn raw_info_base_url(client: &HttpClient) -> Result<reqwest::Url, CliError> {
    let network = Network::from_chain(client.chain());

    if let Some(api_base_url) = Settings::from_process().api_base_url_override(network)? {
        Ok(api_base_url)
    } else if client.chain().is_mainnet() {
        Ok(hypercore::mainnet_url())
    } else {
        Ok(hypercore::testnet_url())
    }
}

/// Find a HIP-3 DEX by name, case-insensitively.
pub(crate) async fn find_perp_dex(client: &HttpClient, dex_name: &str) -> Result<Dex, CliError> {
    client
        .perp_dexes()
        .await
        .map_err(map_api_error)?
        .into_iter()
        .find(|dex| dex.name().eq_ignore_ascii_case(dex_name))
        .ok_or_else(|| CliError::Unsupported(format!("Unknown DEX: {dex_name}")))
}

/// Look up one HIP-3 market live, returning the DEX's canonical name with it.
///
/// `Ok(None)` means the DEX exists but lists no such market.
pub(crate) async fn load_hip3_market(
    client: &HttpClient,
    dex_name: &str,
    token: &str,
) -> Result<Option<(String, PerpMarket)>, CliError> {
    let dex = find_perp_dex(client, dex_name).await?;
    let canonical_dex_name = dex.name().to_string();
    let prefix = format!("{canonical_dex_name}:");
    let market = client
        .perps_from(dex)
        .await
        .map_err(map_api_error)?
        .into_iter()
        .find(|market| {
            let display_token = market.name.strip_prefix(&prefix).unwrap_or(&market.name);
            display_token.eq_ignore_ascii_case(token) || market.name.eq_ignore_ascii_case(token)
        });
    Ok(market.map(|market| (canonical_dex_name, market)))
}

/// A metadata snapshot with the time it was fetched.
#[derive(Debug, Clone)]
pub struct CachedMetadata {
    metadata: AssetMetadata,
    fetched_at: Instant,
}

impl CachedMetadata {
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
}

/// Thread-safe 60-second metadata cache keyed by Hyperliquid network.
#[derive(Debug, Default)]
pub struct MetadataCache {
    cached: Mutex<Vec<(Chain, CachedMetadata)>>,
}

impl MetadataCache {
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
        if let Some(metadata) = self.fresh_metadata_for_chain_at(chain, Instant::now()) {
            return Ok(metadata);
        }

        let metadata = AssetMetadata::load(client).await?;
        self.store_for_chain(chain, metadata.clone(), Instant::now());
        Ok(metadata)
    }

    fn store_for_chain(&self, chain: Chain, metadata: AssetMetadata, fetched_at: Instant) {
        let mut cached = self.cached.lock().expect("metadata cache lock poisoned");
        cached.retain(|(cached_chain, _)| *cached_chain != chain);
        cached.push((chain, CachedMetadata::new(metadata, fetched_at)));
    }

    fn fresh_metadata_for_chain_at(&self, chain: Chain, now: Instant) -> Option<AssetMetadata> {
        self.cached
            .lock()
            .expect("metadata cache lock poisoned")
            .iter()
            .find(|(cached_chain, cached)| *cached_chain == chain && cached.is_fresh_at(now))
            .map(|(_, cached)| cached.metadata.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::super::market::test_support::perp;
    use super::*;

    fn fixture_metadata(asset_name: &str, asset_index: usize) -> AssetMetadata {
        AssetMetadata::from_assets(vec![perp(asset_name, asset_index)], Vec::new())
    }

    fn raw_meta(value: serde_json::Value) -> RawSpotMeta {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn raw_spot_markets_skip_unresolvable_token_references() {
        let raw = raw_meta(serde_json::json!({
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
        }));

        let markets = raw.into_listings();

        assert_eq!(markets.len(), 1);
        assert_eq!(markets[0].symbol, "PURR/USDC");
        assert_eq!(markets[0].index, 10_000);
        assert_eq!(markets[0].base_sz_decimals, 0);
        assert_eq!(markets[0].quote_sz_decimals, 6);
    }

    #[test]
    fn collateral_name_prefers_token_index_then_position_then_usdc() {
        let raw = raw_meta(serde_json::json!({
            "tokens": [
                {"name": "USDC", "index": 0, "szDecimals": 6},
                {"name": "USDH", "index": 360, "szDecimals": 2}
            ],
            "universe": []
        }));

        assert_eq!(raw.collateral_name(Some(360)), "USDH");
        assert_eq!(raw.collateral_name(Some(1)), "USDH");
        assert_eq!(raw.collateral_name(Some(999)), "USDC");
        assert_eq!(raw.collateral_name(None), "USDC");
    }

    #[test]
    fn invalid_size_precision_is_an_error_not_zero() {
        assert_eq!(sz_decimals(5, "BTC").unwrap(), 5);
        let err = sz_decimals(-1, "BTC").unwrap_err();
        assert!(err.to_string().contains("BTC"));
        assert!(sz_decimals(i64::MAX, "BTC").is_err());
    }

    #[test]
    fn spot_listing_with_invalid_size_precision_fails_conversion() {
        let listing = SpotListing {
            symbol: "BAD/USDC".to_string(),
            index: 10_001,
            base: "BAD".to_string(),
            quote: "USDC".to_string(),
            base_sz_decimals: -3,
            quote_sz_decimals: 6,
        };

        assert!(spot_asset_from_listing(listing).is_err());
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
        let testnet_metadata = fixture_metadata("TESTBTC", 10);

        cache.store_for_chain(
            Chain::Mainnet,
            fixture_metadata("BTC", 0),
            stale_mainnet_fetched_at,
        );
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
