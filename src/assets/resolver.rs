//! Resolve asset names to markets over a metadata snapshot, with a live
//! HIP-3 lookup for DEX markets the snapshot does not list.

use hypersdk::hypercore::HttpClient;
use strsim::levenshtein;

use crate::errors::CliError;

use super::asset_id;
use super::market::{Market, OutcomeAsset, PerpAsset, SpotAsset};
use super::metadata::{AssetMetadata, load_hip3_market, perp_asset_from_market};
use super::query::{AssetQuery, eq_asset_name, normalize_asset_name, parse_asset_query};

/// Why an asset name did not resolve.
///
/// Deliberately not a `std::error::Error`: it must be converted to
/// [`CliError`] explicitly (`CliError::from`), because the user-facing text of
/// [`AssetError::UnsupportedOutcome`] belongs to the commands, not the resolver.
#[derive(Debug)]
pub enum AssetError {
    NotFound {
        asset: String,
        suggestion: Option<String>,
    },
    /// Outcome notation given where only perp or spot markets apply.
    UnsupportedOutcome { input: String },
    /// The live HIP-3 lookup failed.
    Load(CliError),
}

/// Asset resolver over a metadata snapshot.
#[derive(Debug, Clone)]
pub struct AssetResolver {
    metadata: AssetMetadata,
}

impl AssetResolver {
    #[must_use]
    pub fn new(metadata: AssetMetadata) -> Self {
        Self { metadata }
    }

    /// Resolver over metadata loaded from the raw `/info` endpoints.
    /// See [`AssetMetadata::load_raw`].
    pub async fn load_raw(api_base_url: &str) -> Result<Self, CliError> {
        AssetMetadata::load_raw(api_base_url).await.map(Self::new)
    }

    /// Resolve according to the user's asset format.
    ///
    /// Outcome notation resolves to [`Market::Outcome`] without being decoded;
    /// commands that cannot trade outcomes reject it themselves.
    pub fn resolve(&self, input: &str) -> Result<Market, AssetError> {
        match parse_asset_query(input) {
            AssetQuery::Perp(_) | AssetQuery::Hip3 { .. } => {
                self.resolve_perp(input).map(Market::Perp)
            }
            AssetQuery::Spot(_) => self.resolve_spot(input).map(Market::Spot),
            AssetQuery::Outcome(notation) => Ok(Market::Outcome(OutcomeAsset::new(notation))),
        }
    }

    /// Resolve a perpetual market symbol or HIP-3 `dex:TOKEN` input.
    pub fn resolve_perp(&self, input: &str) -> Result<PerpAsset, AssetError> {
        match parse_asset_query(input) {
            AssetQuery::Hip3 { dex, token } => self.resolve_hip3(input, &dex, &token),
            AssetQuery::Perp(symbol) => self.resolve_default_perp(input, &symbol),
            AssetQuery::Outcome(_) => Err(unsupported_outcome(input)),
            AssetQuery::Spot(_) => Err(self.not_found(input, self.perp_candidate_names())),
        }
    }

    /// Resolve a spot pair input.
    pub fn resolve_spot(&self, input: &str) -> Result<SpotAsset, AssetError> {
        let pair = match parse_asset_query(input) {
            AssetQuery::Spot(pair) => pair,
            AssetQuery::Outcome(_) => return Err(unsupported_outcome(input)),
            _ => return Err(self.not_found(input, self.spot_candidate_names())),
        };

        self.metadata
            .spots
            .iter()
            .find(|spot| eq_asset_name(&spot.symbol, &pair))
            .cloned()
            .ok_or_else(|| self.not_found(input, self.spot_candidate_names()))
    }

    /// [`Self::resolve_perp`], looking a missing HIP-3 market up live.
    pub async fn resolve_perp_or_fetch(
        &self,
        client: &HttpClient,
        input: &str,
    ) -> Result<PerpAsset, AssetError> {
        match self.resolve_perp(input) {
            Ok(perp) => Ok(perp),
            Err(err) => fetch_hip3_perp(client, input, err).await,
        }
    }

    /// [`Self::resolve`], looking a missing HIP-3 market up live.
    pub async fn resolve_or_fetch(
        &self,
        client: &HttpClient,
        input: &str,
    ) -> Result<Market, AssetError> {
        match parse_asset_query(input) {
            AssetQuery::Hip3 { .. } => self
                .resolve_perp_or_fetch(client, input)
                .await
                .map(Market::Perp),
            _ => self.resolve(input),
        }
    }

    /// Resolve a perp like [`Self::resolve_perp_or_fetch`] and return a resolver
    /// that also lists it, for planners that resolve again without a client.
    pub async fn with_resolved_perp(
        self,
        client: &HttpClient,
        input: &str,
    ) -> Result<(Self, PerpAsset), AssetError> {
        match self.resolve_perp(input) {
            Ok(perp) => Ok((self, perp)),
            Err(err) => {
                let perp = fetch_hip3_perp(client, input, err).await?;
                Ok((self.with_perp(&perp), perp))
            }
        }
    }

    /// Spot protocol asset id for an info-endpoint `@N` key, if listed.
    #[must_use]
    pub fn spot_asset_index_for_internal_id(&self, internal_id: usize) -> Option<usize> {
        self.spot_for_internal_id(internal_id)
            .map(|spot| spot.index)
    }

    #[must_use]
    pub fn display_coin(&self, coin: &str) -> String {
        asset_id::parse_spot_coin_key(coin)
            .and_then(|internal_id| self.spot_for_internal_id(internal_id))
            .map_or_else(|| coin.to_string(), |spot| spot.symbol.clone())
    }

    #[must_use]
    pub fn perp_by_protocol_asset_id(&self, asset_id: usize) -> Option<PerpAsset> {
        self.metadata
            .perps
            .iter()
            .find(|asset| asset.index == asset_id)
            .cloned()
    }

    #[must_use]
    pub fn spot_by_protocol_asset_id(&self, asset_id: usize) -> Option<SpotAsset> {
        self.metadata
            .spots
            .iter()
            .find(|spot| spot.index == asset_id)
            .cloned()
    }

    #[must_use]
    pub fn perps(&self) -> &[PerpAsset] {
        self.metadata.perps()
    }

    #[must_use]
    pub fn spots(&self) -> &[SpotAsset] {
        self.metadata.spots()
    }

    /// Suggestions across all known asset names.
    #[must_use]
    pub fn suggestions(&self, input: &str) -> Vec<String> {
        suggestions(input, self.metadata.all_candidate_names())
    }

    #[must_use]
    pub fn with_perp(&self, asset: &PerpAsset) -> Self {
        let mut metadata = self.metadata.clone();
        if !metadata.has_perp(asset) {
            metadata.perps.push(asset.clone());
        }
        Self { metadata }
    }

    fn spot_for_internal_id(&self, internal_id: usize) -> Option<&SpotAsset> {
        self.metadata
            .spots
            .iter()
            .find(|spot| asset_id::spot_internal_id(spot.index) == internal_id)
    }

    fn resolve_default_perp(&self, input: &str, symbol: &str) -> Result<PerpAsset, AssetError> {
        self.metadata
            .perps
            .iter()
            .find(|asset| asset.dex.is_none() && eq_asset_name(&asset.name, symbol))
            .cloned()
            .ok_or_else(|| self.not_found(input, self.perp_candidate_names()))
    }

    fn resolve_hip3(&self, input: &str, dex: &str, token: &str) -> Result<PerpAsset, AssetError> {
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
            .cloned()
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

    fn not_found(&self, input: &str, candidates: Vec<String>) -> AssetError {
        AssetError::NotFound {
            asset: input.to_string(),
            suggestion: suggestions(input, candidates).into_iter().next(),
        }
    }
}

fn unsupported_outcome(input: &str) -> AssetError {
    AssetError::UnsupportedOutcome {
        input: input.to_string(),
    }
}

/// After `missed` (a failed snapshot lookup of `input`), look a HIP-3 market up
/// live. A DEX without that market reports the original miss.
async fn fetch_hip3_perp(
    client: &HttpClient,
    input: &str,
    missed: AssetError,
) -> Result<PerpAsset, AssetError> {
    let AssetQuery::Hip3 { dex, token } = parse_asset_query(input) else {
        return Err(missed);
    };
    match load_hip3_market(client, &dex, &token).await {
        Ok(Some((canonical_dex, market))) => {
            perp_asset_from_market(market, Some(canonical_dex)).map_err(AssetError::Load)
        }
        Ok(None) => Err(missed),
        Err(err) => Err(AssetError::Load(err)),
    }
}

/// Return at most one close fuzzy match by Levenshtein distance.
///
/// The match must be tight (see [`suggestion_threshold`]); a loose match is
/// worse than none. Ties break by candidate order, then alphabetically.
#[must_use]
pub fn suggestions<I, S>(input: &str, candidates: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let normalized_input = normalize_asset_name(input);
    let max_distance = suggestion_threshold(&normalized_input);

    candidates
        .into_iter()
        .enumerate()
        .filter_map(|(position, candidate)| {
            let candidate = candidate.as_ref().trim().to_string();
            if candidate.is_empty() {
                return None;
            }

            let normalized_candidate = normalize_asset_name(&candidate);
            let distance = levenshtein(&normalized_input, &normalized_candidate);
            is_rankable_suggestion(
                &normalized_input,
                &normalized_candidate,
                distance,
                max_distance,
            )
            .then_some((distance, position, candidate))
        })
        .min_by(
            |(left_distance, left_position, left), (right_distance, right_position, right)| {
                left_distance
                    .cmp(right_distance)
                    .then_with(|| left_position.cmp(right_position))
                    .then_with(|| left.cmp(right))
            },
        )
        .map(|(_, _, candidate)| candidate)
        .into_iter()
        .collect()
}

/// Largest edit distance still worth suggesting: one edit per three characters
/// (at least one, at most three).
fn suggestion_threshold(input: &str) -> usize {
    (input.chars().count() / 3).clamp(1, 3)
}

fn is_rankable_suggestion(
    input: &str,
    candidate: &str,
    distance: usize,
    max_distance: usize,
) -> bool {
    distance <= max_distance
        && distance < input.chars().count().max(1)
        && candidate.chars().count() >= 2
}

#[cfg(test)]
mod tests {
    use super::super::market::test_support::{hip3_perp, perp, spot};
    use super::*;

    fn resolver() -> AssetResolver {
        AssetResolver::new(AssetMetadata::from_assets(
            vec![perp("BTC", 0), hip3_perp("dex", "TOKEN", 110_000)],
            vec![spot("PURR/USDC", 10_000, "PURR", "USDC")],
        ))
    }

    #[test]
    fn suggests_at_most_one_tight_match() {
        let candidates = ["BTC", "LTC", "BCH", "FARTCOIN", "TON", "NFTI"];

        assert_eq!(suggestions("BTCC", candidates), vec!["BTC"]);
        assert_eq!(suggestions("ETHH", ["ETH", "ETC"]), vec!["ETH"]);
        assert!(suggestions("NOTACOIN", candidates).is_empty());
        assert!(suggestions("ZZZZZZZZ", candidates).is_empty());
    }

    #[test]
    fn suggestion_ties_prefer_earlier_candidate() {
        assert_eq!(suggestions("BTX", ["BTC", "BTT"]), vec!["BTC"]);
    }

    #[test]
    fn suggestion_thresholds_stay_tight() {
        assert_eq!(suggestion_threshold("BT"), 1);
        assert_eq!(suggestion_threshold("BTCC"), 1);
        assert_eq!(suggestion_threshold("NOTACOIN"), 2);
        assert_eq!(suggestion_threshold("A".repeat(40).as_str()), 3);
    }

    #[test]
    fn not_found_carries_one_suggestion() {
        let err = resolver().resolve_perp("BTCC").unwrap_err();

        assert!(matches!(
            err,
            AssetError::NotFound { asset, suggestion }
                if asset == "BTCC" && suggestion.as_deref() == Some("BTC")
        ));
    }

    #[test]
    fn resolve_returns_each_market_kind_for_its_query_shape() {
        let resolver = resolver();

        assert!(matches!(resolver.resolve("btc"), Ok(Market::Perp(perp)) if perp.index == 0));
        assert!(
            matches!(resolver.resolve("dex:token"), Ok(Market::Perp(perp)) if perp.index == 110_000)
        );
        assert!(
            matches!(resolver.resolve("purr/usdc"), Ok(Market::Spot(spot)) if spot.index == 10_000)
        );
        assert!(matches!(
            resolver.resolve("#10"),
            Ok(Market::Outcome(outcome)) if outcome.notation() == "#10"
        ));
    }

    #[test]
    fn perp_and_spot_lookups_reject_outcome_notation_with_the_raw_input() {
        let resolver = resolver();

        for err in [
            resolver.resolve_perp(" #10").unwrap_err(),
            resolver.resolve_spot(" #10").unwrap_err(),
        ] {
            assert!(matches!(err, AssetError::UnsupportedOutcome { input } if input == " #10"));
        }
    }

    #[test]
    fn spot_query_is_not_a_perp_and_a_perp_is_not_a_spot() {
        let resolver = resolver();

        assert!(matches!(
            resolver.resolve_perp("PURR/USDC"),
            Err(AssetError::NotFound { .. })
        ));
        assert!(matches!(
            resolver.resolve_spot("BTC"),
            Err(AssetError::NotFound { .. })
        ));
    }

    #[test]
    fn spot_coin_keys_display_as_symbols() {
        let resolver = resolver();

        assert_eq!(resolver.display_coin("@0"), "PURR/USDC");
        assert_eq!(resolver.display_coin("@9"), "@9");
        assert_eq!(resolver.display_coin("BTC"), "BTC");
        assert_eq!(resolver.spot_asset_index_for_internal_id(0), Some(10_000));
    }

    #[test]
    fn with_perp_adds_only_unlisted_markets() {
        let resolver = resolver();
        let listed = hip3_perp("DEX", "token", 110_001);
        let new = hip3_perp("other", "NEW", 120_000);

        assert_eq!(resolver.with_perp(&listed).perps().len(), 2);
        assert_eq!(resolver.with_perp(&new).perps().len(), 3);
    }
}
