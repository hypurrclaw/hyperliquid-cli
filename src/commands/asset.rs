//! Protocol asset ID lookup commands.
//!
//! Commands:
//! - `hyperliquid asset decode <ASSET_ID>` — explain the protocol asset IDs surfaced in exchange errors.
//! - `hyperliquid asset search <QUERY>` — find assets by symbol, title, slug, notation, or ID.

use std::collections::BTreeSet;

use clap::{Args, ValueEnum};
use serde::Serialize;
use serde_json::{Map, Number, Value};
use strsim::levenshtein;

use crate::assets::asset_id::{
    self, AssetIdLayout, MAX_OUTCOME_SIDE, outcome_coin, outcome_token, split_outcome_encoding,
};
use crate::assets::{AssetResolver, Market};
use crate::command_spec::{CliCommand, CommandSpec, GroupSpec, InputKind, arg};
use crate::http_api::ApiClient;
use crate::invocation::Invocation;
use crate::output::TableData;
use crate::stream::parse_positive_usize;

#[derive(Args, Debug, Clone)]
pub struct AssetDecodeArgs {
    /// Raw protocol asset ID from exchange errors, schemas, or payloads
    #[arg(value_parser = parse_asset_id)]
    pub asset_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab_case")]
pub enum SearchVenueFilter {
    All,
    Perp,
    Hip3,
    Spot,
    Outcome,
}

#[derive(Args, Debug, Clone)]
pub struct AssetSearchArgs {
    /// Asset symbol, market title, app slug, outcome notation, or protocol asset ID
    pub query: String,

    /// Restrict results to one venue
    #[arg(
        id = "type",
        long = "type",
        value_name = "VENUE",
        value_enum,
        default_value = "all"
    )]
    pub venue: SearchVenueFilter,

    /// Maximum number of matching assets to return
    #[arg(long, default_value = "20", value_parser = parse_positive_usize)]
    pub limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetIdKind {
    Perp,
    Spot,
    Hip3Perp,
    Outcome,
}

impl AssetIdKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Perp => "perp",
            Self::Spot => "spot",
            Self::Hip3Perp => "hip3_perp",
            Self::Outcome => "outcome",
        }
    }

    fn venue(self) -> &'static str {
        match self {
            Self::Perp => "perp",
            Self::Spot => "spot",
            Self::Hip3Perp => "hip3",
            Self::Outcome => "outcome",
        }
    }

    fn matches_filter(self, filter: SearchVenueFilter) -> bool {
        match filter {
            SearchVenueFilter::All => true,
            SearchVenueFilter::Perp => matches!(self, Self::Perp),
            SearchVenueFilter::Hip3 => matches!(self, Self::Hip3Perp),
            SearchVenueFilter::Spot => matches!(self, Self::Spot),
            SearchVenueFilter::Outcome => matches!(self, Self::Outcome),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupStatus {
    MetadataFound,
    FormulaOnly,
    InvalidId,
}

impl LookupStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::MetadataFound => "metadata_found",
            Self::FormulaOnly => "formula_only",
            Self::InvalidId => "invalid_id",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedAssetId {
    pub asset_id: u64,
    pub kind: AssetIdKind,
    pub lookup_status: LookupStatus,
    pub network: String,
    pub cli_input: Option<String>,
    pub symbol: Option<String>,
    pub dex: Option<String>,
    pub perp_index: Option<u64>,
    pub spot_index: Option<u64>,
    pub dex_slot: Option<u64>,
    pub market_index: Option<u64>,
    pub base: Option<String>,
    pub quote: Option<String>,
    pub encoding: Option<u64>,
    pub outcome: Option<u64>,
    pub side: Option<u64>,
    pub side_name: Option<String>,
    pub coin: Option<String>,
    pub token: Option<String>,
    pub market_title: Option<String>,
    pub slug: Option<String>,
    pub condition: Option<String>,
    pub condition_class: Option<String>,
    pub underlying: Option<String>,
    pub expiry: Option<String>,
    pub target_price: Option<String>,
    pub period: Option<String>,
    pub outcome_name: Option<String>,
    pub description: Option<String>,
    pub reason: Option<String>,
}

impl DecodedAssetId {
    fn new(asset_id: u64, kind: AssetIdKind, network: impl Into<String>) -> Self {
        Self {
            asset_id,
            kind,
            lookup_status: LookupStatus::FormulaOnly,
            network: network.into(),
            cli_input: None,
            symbol: None,
            dex: None,
            perp_index: None,
            spot_index: None,
            dex_slot: None,
            market_index: None,
            base: None,
            quote: None,
            encoding: None,
            outcome: None,
            side: None,
            side_name: None,
            coin: None,
            token: None,
            market_title: None,
            slug: None,
            condition: None,
            condition_class: None,
            underlying: None,
            expiry: None,
            target_price: None,
            period: None,
            outcome_name: None,
            description: None,
            reason: None,
        }
    }

    fn json_value(&self) -> Value {
        let mut object = Map::new();
        insert_u64(&mut object, "asset_id", self.asset_id);
        insert_str(&mut object, "kind", self.kind.as_str());
        insert_str(&mut object, "venue", self.kind.venue());
        insert_str(&mut object, "lookup_status", self.lookup_status.as_str());
        insert_str(&mut object, "network", &self.network);
        insert_option_str(&mut object, "cli_input", self.cli_input.as_deref());
        insert_option_str(&mut object, "symbol", self.symbol.as_deref());
        insert_option_str(&mut object, "dex", self.dex.as_deref());
        insert_option_u64(&mut object, "dex_slot", self.dex_slot);
        insert_option_u64(&mut object, "perp_index", self.perp_index);
        insert_option_u64(&mut object, "spot_index", self.spot_index);
        insert_option_u64(&mut object, "market_index", self.market_index);
        insert_option_str(&mut object, "base", self.base.as_deref());
        insert_option_str(&mut object, "quote", self.quote.as_deref());
        insert_option_u64(&mut object, "encoding", self.encoding);
        insert_option_u64(&mut object, "outcome", self.outcome);
        insert_option_u64(&mut object, "side", self.side);
        insert_option_str(&mut object, "side_name", self.side_name.as_deref());
        insert_option_str(&mut object, "coin", self.coin.as_deref());
        insert_option_str(&mut object, "token", self.token.as_deref());
        insert_option_str(&mut object, "market_title", self.market_title.as_deref());
        insert_option_str(&mut object, "slug", self.slug.as_deref());
        insert_option_str(&mut object, "condition", self.condition.as_deref());
        insert_option_str(
            &mut object,
            "condition_class",
            self.condition_class.as_deref(),
        );
        insert_option_str(&mut object, "underlying", self.underlying.as_deref());
        insert_option_str(&mut object, "expiry", self.expiry.as_deref());
        insert_option_str(&mut object, "target_price", self.target_price.as_deref());
        insert_option_str(&mut object, "period", self.period.as_deref());
        insert_option_str(&mut object, "outcome_name", self.outcome_name.as_deref());
        insert_option_str(&mut object, "description", self.description.as_deref());
        insert_option_str(&mut object, "reason", self.reason.as_deref());
        Value::Object(object)
    }

    fn field_rows(&self) -> Vec<Vec<String>> {
        let Value::Object(object) = self.json_value() else {
            return Vec::new();
        };
        object
            .into_iter()
            .map(|(key, value)| vec![humanize_field(&key), json_cell(&value)])
            .collect()
    }

    fn search_fields(&self) -> Vec<String> {
        let mut fields = vec![
            self.asset_id.to_string(),
            self.kind.as_str().to_string(),
            self.lookup_status.as_str().to_string(),
        ];
        fields.extend(
            [
                self.cli_input.as_deref(),
                self.symbol.as_deref(),
                self.dex.as_deref(),
                self.base.as_deref(),
                self.quote.as_deref(),
                self.coin.as_deref(),
                self.token.as_deref(),
                self.market_title.as_deref(),
                self.slug.as_deref(),
                self.condition.as_deref(),
                self.condition_class.as_deref(),
                self.underlying.as_deref(),
                self.target_price.as_deref(),
                self.expiry.as_deref(),
                self.period.as_deref(),
                self.outcome_name.as_deref(),
                self.description.as_deref(),
            ]
            .into_iter()
            .flatten()
            .map(str::to_string),
        );
        if let Some(perp_index) = self.perp_index {
            fields.push(perp_index.to_string());
        }
        if let Some(spot_index) = self.spot_index {
            fields.push(spot_index.to_string());
        }
        if let Some(encoding) = self.encoding {
            fields.push(encoding.to_string());
        }
        fields
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetDecodeOutput {
    decoded: DecodedAssetId,
}

impl AssetDecodeOutput {
    #[must_use]
    pub fn new(decoded: DecodedAssetId) -> Self {
        Self { decoded }
    }
}

impl TableData for AssetDecodeOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Field", "Value"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.decoded.field_rows()
    }

    fn to_json_value(&self) -> Value {
        self.decoded.json_value()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetSearchOutput {
    rows: Vec<DecodedAssetId>,
}

impl AssetSearchOutput {
    #[must_use]
    pub fn new(rows: Vec<DecodedAssetId>) -> Self {
        Self { rows }
    }
}

impl TableData for AssetSearchOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Asset ID",
            "Kind",
            "Status",
            "Use As",
            "Symbol",
            "Market Title",
            "Condition",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.asset_id.to_string(),
                    row.kind.as_str().to_string(),
                    row.lookup_status.as_str().to_string(),
                    row.cli_input.clone().unwrap_or_else(|| "n/a".to_string()),
                    row.symbol.clone().unwrap_or_default(),
                    row.market_title.clone().unwrap_or_default(),
                    row.condition.clone().unwrap_or_default(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> Value {
        Value::Array(self.rows.iter().map(DecodedAssetId::json_value).collect())
    }
}

pub async fn decode(inv: &Invocation, args: &AssetDecodeArgs) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let api = remote.api();
    let mut decoded = decode_asset_id(args.asset_id, &remote.network().to_string());

    match decoded.kind {
        AssetIdKind::Outcome => enrich_outcome(api, &mut decoded).await,
        AssetIdKind::Perp | AssetIdKind::Spot | AssetIdKind::Hip3Perp => {
            enrich_from_resolver(api, &mut decoded).await
        }
    }

    inv.print(&AssetDecodeOutput::new(decoded));
    Ok(())
}

pub async fn search(inv: &Invocation, args: &AssetSearchArgs) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let rows = search_assets(
        remote.api(),
        &remote.network().to_string(),
        &args.query,
        args.venue,
        args.limit,
    )
    .await;

    inv.print(&AssetSearchOutput::new(rows));
    Ok(())
}

#[must_use]
pub fn decode_asset_id(asset_id: u64, network: &str) -> DecodedAssetId {
    match asset_id::classify(asset_id) {
        AssetIdLayout::Outcome { encoding } => decode_outcome_asset_id(asset_id, encoding, network),
        AssetIdLayout::Hip3Perp { dex_slot, index } => {
            let mut decoded = DecodedAssetId::new(asset_id, AssetIdKind::Hip3Perp, network);
            decoded.dex_slot = Some(dex_slot);
            decoded.perp_index = Some(index);
            decoded.market_index = Some(index);
            decoded
        }
        AssetIdLayout::Spot { spot_index } => {
            let mut decoded = DecodedAssetId::new(asset_id, AssetIdKind::Spot, network);
            decoded.spot_index = Some(spot_index);
            decoded.market_index = Some(spot_index);
            decoded
        }
        AssetIdLayout::Perp { index } => {
            let mut decoded = DecodedAssetId::new(asset_id, AssetIdKind::Perp, network);
            decoded.perp_index = Some(index);
            decoded.market_index = Some(index);
            decoded
        }
    }
}

fn decode_outcome_asset_id(asset_id: u64, encoding: u64, network: &str) -> DecodedAssetId {
    let mut decoded = DecodedAssetId::new(asset_id, AssetIdKind::Outcome, network);
    let (outcome, side) = split_outcome_encoding(encoding);
    decoded.encoding = Some(encoding);
    decoded.outcome = Some(outcome);
    decoded.side = Some(side);
    decoded.coin = Some(outcome_coin(encoding));
    decoded.token = Some(outcome_token(encoding));

    if side > MAX_OUTCOME_SIDE {
        decoded.lookup_status = LookupStatus::InvalidId;
        decoded.cli_input = None;
        decoded.reason = Some("only binary outcome sides 0 and 1 are supported".to_string());
    } else {
        decoded.cli_input = decoded.coin.clone();
        decoded.side_name = Some(if side == 0 { "Yes" } else { "No" }.to_string());
    }

    decoded
}

async fn search_assets(
    api: &ApiClient,
    network: &str,
    query: &str,
    venue: SearchVenueFilter,
    limit: usize,
) -> Vec<DecodedAssetId> {
    let mut candidates = Vec::new();

    let resolver = AssetResolver::load_raw(api).await.ok();
    if let Some(resolver) = resolver.as_ref() {
        candidates.extend(search_resolver_assets(resolver, network));
    }

    let outcome_rows = crate::commands::outcomes::outcome_side_rows(api).await.ok();
    if let Some(rows) = outcome_rows.as_ref() {
        candidates.extend(rows.iter().cloned().filter_map(|row| {
            let asset_id = row.asset_id;
            let mut decoded = decode_asset_id(asset_id, network);
            apply_outcome_row(&mut decoded, row);
            (decoded.lookup_status == LookupStatus::MetadataFound).then_some(decoded)
        }));
    }

    if let Ok(asset_id) = parse_asset_id(query) {
        let mut decoded = decode_asset_id(asset_id, network);
        match decoded.kind {
            AssetIdKind::Outcome => {
                if let Some(row) = outcome_rows
                    .as_ref()
                    .and_then(|rows| {
                        rows.iter()
                            .find(|row| Some(row.encoding) == decoded.encoding)
                    })
                    .cloned()
                {
                    apply_outcome_row(&mut decoded, row);
                }
            }
            AssetIdKind::Perp | AssetIdKind::Spot | AssetIdKind::Hip3Perp => {
                if let Some(resolver) = resolver.as_ref() {
                    enrich_from_loaded_resolver(resolver, &mut decoded);
                }
            }
        }
        if !candidates
            .iter()
            .any(|candidate| candidate.asset_id == decoded.asset_id)
        {
            candidates.push(decoded);
        }
    }

    let candidates = candidates
        .into_iter()
        .filter(|candidate| candidate.kind.matches_filter(venue))
        .collect();
    ranked_search_results(query, candidates, limit)
}

fn search_resolver_assets(resolver: &AssetResolver, network: &str) -> Vec<DecodedAssetId> {
    let mut rows = Vec::new();

    for perp in resolver.perps() {
        if let Ok(asset_id) = u64::try_from(perp.index) {
            let mut decoded = decode_asset_id(asset_id, network);
            enrich_from_loaded_resolver(resolver, &mut decoded);
            rows.push(decoded);
        }
    }

    for spot in resolver.spots() {
        if let Ok(asset_id) = u64::try_from(spot.index) {
            let mut decoded = decode_asset_id(asset_id, network);
            enrich_from_loaded_resolver(resolver, &mut decoded);
            rows.push(decoded);
        }
    }

    rows
}

fn ranked_search_results(
    query: &str,
    candidates: Vec<DecodedAssetId>,
    limit: usize,
) -> Vec<DecodedAssetId> {
    let mut seen = BTreeSet::new();
    let mut scored = candidates
        .into_iter()
        .filter(|candidate| seen.insert(candidate.asset_id))
        .filter_map(|candidate| search_score(query, &candidate).map(|score| (score, candidate)))
        .collect::<Vec<_>>();

    scored.sort_by(|(left_score, left), (right_score, right)| {
        left_score
            .cmp(right_score)
            .then_with(|| left.asset_id.cmp(&right.asset_id))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, candidate)| candidate)
        .collect()
}

fn search_score(query: &str, candidate: &DecodedAssetId) -> Option<usize> {
    let query = normalize_search_text(query);
    if query.is_empty() {
        return None;
    }

    candidate
        .search_fields()
        .into_iter()
        .filter_map(|field| {
            let field = normalize_search_text(&field);
            if field.is_empty() {
                None
            } else if field == query {
                Some(0)
            } else if field.starts_with(&query) {
                Some(10)
            } else if field.contains(&query) {
                Some(20)
            } else if fuzzy_search_allowed(&query, &field) {
                let distance = levenshtein(&query, &field);
                (distance <= fuzzy_search_threshold(&query)).then_some(50 + distance)
            } else {
                None
            }
        })
        .min()
}

fn fuzzy_search_allowed(query: &str, field: &str) -> bool {
    query.len() <= 32 && field.len() <= 64
}

fn fuzzy_search_threshold(query: &str) -> usize {
    if query.len() <= 3 { 1 } else { 2 }
}

fn normalize_search_text(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

async fn enrich_from_resolver(api: &ApiClient, decoded: &mut DecodedAssetId) {
    let Ok(resolver) = AssetResolver::load_raw(api).await else {
        return;
    };
    enrich_from_loaded_resolver(&resolver, decoded);
}

pub fn enrich_from_loaded_resolver(resolver: &AssetResolver, decoded: &mut DecodedAssetId) {
    let Ok(asset_id) = usize::try_from(decoded.asset_id) else {
        return;
    };

    let resolved = match decoded.kind {
        AssetIdKind::Perp | AssetIdKind::Hip3Perp => resolver
            .perp_by_protocol_asset_id(asset_id)
            .map(Market::Perp),
        AssetIdKind::Spot => resolver
            .spot_by_protocol_asset_id(asset_id)
            .map(Market::Spot),
        AssetIdKind::Outcome => None,
    };

    match resolved {
        Some(Market::Perp(perp)) => {
            decoded.lookup_status = LookupStatus::MetadataFound;
            decoded.cli_input = Some(perp.display_name());
            decoded.symbol = Some(perp.name);
            decoded.dex = perp.dex;
        }
        Some(Market::Spot(spot)) => {
            decoded.lookup_status = LookupStatus::MetadataFound;
            decoded.cli_input = Some(spot.symbol.clone());
            decoded.symbol = Some(spot.symbol);
            decoded.base = Some(spot.base);
            decoded.quote = Some(spot.quote);
        }
        Some(Market::Outcome(_)) | None => {}
    }
}

async fn enrich_outcome(api: &ApiClient, decoded: &mut DecodedAssetId) {
    if decoded.lookup_status == LookupStatus::InvalidId {
        return;
    }

    let Some(encoding) = decoded.encoding else {
        return;
    };
    let Ok(rows) = crate::commands::outcomes::outcome_side_rows(api).await else {
        return;
    };
    let Some(row) = rows.into_iter().find(|row| row.encoding == encoding) else {
        return;
    };

    apply_outcome_row(decoded, row);
}

fn apply_outcome_row(decoded: &mut DecodedAssetId, row: crate::commands::outcomes::OutcomeSideRow) {
    decoded.lookup_status = LookupStatus::MetadataFound;
    decoded.cli_input = Some(row.coin.clone());
    decoded.coin = Some(row.coin);
    decoded.token = Some(row.token);
    decoded.outcome = Some(row.outcome);
    decoded.side = Some(row.side);
    let side_name = row.side_name;
    decoded.side_name = Some(side_name.clone());
    decoded.outcome_name = Some(row.outcome_name.clone());
    let description = row.description;
    let parts = parse_outcome_description(&description);
    decoded.market_title = outcome_market_title(&parts, &side_name, &row.outcome_name);
    decoded.slug = outcome_slug(&parts, &side_name);
    decoded.condition = parts.condition;
    decoded.condition_class = parts.condition_class;
    decoded.underlying = parts.underlying;
    decoded.expiry = parts.expiry;
    decoded.target_price = parts.target_price;
    decoded.period = parts.period;
    decoded.description = Some(description);
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct OutcomeDescriptionParts {
    condition: Option<String>,
    condition_class: Option<String>,
    underlying: Option<String>,
    expiry: Option<String>,
    target_price: Option<String>,
    period: Option<String>,
}

impl OutcomeDescriptionParts {
    /// `(underlying, target_price, expiry)` of a fully described price-binary market.
    fn price_binary(&self) -> Option<(&str, &str, &str)> {
        self.condition_class
            .as_deref()
            .filter(|class| class.eq_ignore_ascii_case("priceBinary"))?;
        Some((
            self.underlying.as_deref()?,
            self.target_price.as_deref()?,
            self.expiry.as_deref()?,
        ))
    }
}

fn parse_outcome_description(description: &str) -> OutcomeDescriptionParts {
    let mut parts = OutcomeDescriptionParts::default();

    for segment in description.split('|') {
        let Some((raw_key, raw_value)) = segment.split_once(':') else {
            continue;
        };
        let key = raw_key.trim();
        let value = raw_value.trim();
        if value.is_empty() {
            continue;
        }
        match key {
            "class" => parts.condition_class = Some(value.to_string()),
            "underlying" => parts.underlying = Some(value.to_string()),
            "expiry" => parts.expiry = Some(value.to_string()),
            "targetPrice" | "target_price" => parts.target_price = Some(value.to_string()),
            "period" => parts.period = Some(value.to_string()),
            _ => {}
        }
    }

    parts.condition = outcome_condition_summary(&parts);
    parts
}

fn outcome_condition_summary(parts: &OutcomeDescriptionParts) -> Option<String> {
    if let Some((underlying, target_price, expiry)) = parts.price_binary() {
        let expiry = format_expiry_display(expiry).unwrap_or_else(|| expiry.to_string());
        return Some(format!("{underlying} above {target_price} at {expiry}"));
    }

    let mut fields = Vec::new();
    if let Some(condition_class) = parts.condition_class.as_deref() {
        fields.push(condition_class.to_string());
    }
    if let Some(underlying) = parts.underlying.as_deref() {
        fields.push(format!("underlying={underlying}"));
    }
    if let Some(target_price) = parts.target_price.as_deref() {
        fields.push(format!("target_price={target_price}"));
    }
    if let Some(expiry) = parts.expiry.as_deref() {
        fields.push(format!("expiry={expiry}"));
    }
    if let Some(period) = parts.period.as_deref() {
        fields.push(format!("period={period}"));
    }

    (!fields.is_empty()).then(|| fields.join("; "))
}

fn outcome_market_title(
    parts: &OutcomeDescriptionParts,
    side_name: &str,
    fallback_name: &str,
) -> Option<String> {
    if let Some((underlying, target_price, expiry)) = parts.price_binary() {
        let expiry = format_expiry_display(expiry).unwrap_or_else(|| expiry.to_string());
        return Some(format!(
            "{underlying} above {target_price} {side_name} {expiry}"
        ));
    }

    (!fallback_name.trim().is_empty()).then(|| fallback_name.to_string())
}

fn outcome_slug(parts: &OutcomeDescriptionParts, side_name: &str) -> Option<String> {
    let (underlying, target_price, expiry) = parts.price_binary()?;
    let expiry_slug = format_expiry_slug(expiry)?;
    Some(format!(
        "{}-above-{}-{}-{expiry_slug}",
        slug_token(underlying),
        slug_token(target_price),
        slug_token(side_name)
    ))
}

fn format_expiry_display(raw: &str) -> Option<String> {
    let (_, month, day, hour, minute) = parse_expiry_parts(raw)?;
    let month = month_name(month)?;
    Some(format!("{month} {day} {hour}:{minute}"))
}

fn format_expiry_slug(raw: &str) -> Option<String> {
    let (_, month, day, hour, minute) = parse_expiry_parts(raw)?;
    let month = month_name(month)?.to_ascii_lowercase();
    Some(format!("{month}-{day}-{hour}{minute}"))
}

fn parse_expiry_parts(raw: &str) -> Option<(&str, &str, &str, &str, &str)> {
    let raw = raw.trim();
    if raw.len() != 13 || raw.as_bytes().get(8) != Some(&b'-') {
        return None;
    }
    let year = raw.get(0..4)?;
    let month = raw.get(4..6)?;
    let day = raw.get(6..8)?;
    let hour = raw.get(9..11)?;
    let minute = raw.get(11..13)?;
    [year, month, day, hour, minute]
        .into_iter()
        .all(|part| part.chars().all(|ch| ch.is_ascii_digit()))
        .then_some((year, month, day, hour, minute))
}

fn month_name(month: &str) -> Option<&'static str> {
    match month {
        "01" => Some("Jan"),
        "02" => Some("Feb"),
        "03" => Some("Mar"),
        "04" => Some("Apr"),
        "05" => Some("May"),
        "06" => Some("Jun"),
        "07" => Some("Jul"),
        "08" => Some("Aug"),
        "09" => Some("Sep"),
        "10" => Some("Oct"),
        "11" => Some("Nov"),
        "12" => Some("Dec"),
        _ => None,
    }
}

fn slug_token(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if ch.is_whitespace() || matches!(ch, '_' | ':') {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

pub fn parse_asset_id(raw: &str) -> Result<u64, String> {
    if raw.is_empty() || !raw.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("asset id must be a non-negative decimal integer".to_string());
    }
    raw.parse::<u64>()
        .map_err(|err| format!("invalid asset id: {err}"))
}

fn insert_str(object: &mut Map<String, Value>, key: &str, value: &str) {
    object.insert(key.to_string(), Value::String(value.to_string()));
}

fn insert_u64(object: &mut Map<String, Value>, key: &str, value: u64) {
    object.insert(key.to_string(), Value::Number(Number::from(value)));
}

fn insert_option_str(object: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if key == "cli_input" {
        object.insert(
            key.to_string(),
            value
                .map(|value| Value::String(value.to_string()))
                .unwrap_or(Value::Null),
        );
    } else if let Some(value) = value {
        insert_str(object, key, value);
    }
}

fn insert_option_u64(object: &mut Map<String, Value>, key: &str, value: Option<u64>) {
    if let Some(value) = value {
        insert_u64(object, key, value);
    }
}

fn json_cell(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => value.to_string(),
    }
}

fn humanize_field(field: &str) -> String {
    field
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub const GROUP: GroupSpec =
    GroupSpec::new(&["asset"], "Decode and search protocol asset IDs").aliases(&["assets"]);

pub struct AssetDecode;

impl CliCommand for AssetDecode {
    type Args = AssetDecodeArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["asset", "decode"],
        "market",
        "Decode a raw protocol asset ID",
        "Decode a raw Hyperliquid protocol asset ID from exchange errors, schemas, or payloads into reusable market notation when metadata is available.",
    )
    .args(&[
        arg("asset_id").kind(InputKind::Identifier).describe("Raw non-negative decimal protocol asset ID, for example 100000400."),
    ]);

    async fn run(inv: &Invocation, args: &AssetDecodeArgs) -> Result<(), anyhow::Error> {
        decode(inv, args).await
    }
}

pub struct AssetSearch;

impl CliCommand for AssetSearch {
    type Args = AssetSearchArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["asset", "search"],
        "market",
        "Search assets by symbol, market title, slug, notation, or protocol ID",
        "Search Hyperliquid assets by symbol, market title, app slug, outcome notation, or protocol asset ID and return reusable CLI selectors plus protocol IDs.",
    )
    .args(&[
        arg("query").kind(InputKind::Identifier).describe("Asset search query, for example BTC, xyz:TSLA, PURR/USDC, #400, or btc-above-79779-yes-may-15-0600.").asset_input("asset search query"),
        arg("limit"),
        arg("type").describe("Restrict results to one venue."),
    ]);

    async fn run(inv: &Invocation, args: &AssetSearchArgs) -> Result<(), anyhow::Error> {
        search(inv, args).await
    }
}

pub struct Search;

impl CliCommand for Search {
    type Args = AssetSearchArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["search"],
        "market",
        "Search markets across perps, HIP-3, spot, and outcomes",
        "Search perps, HIP-3, spot, and outcome markets in one list. Each row includes a stable venue field. Retry: safe. Pipe --select coin into buy --dry-run.",
    )
    .long_about("Search markets across perps, HIP-3, spot, and outcomes.\n\
\n\
Retry: safe.\n\
\n\
Examples:\n  \
hyperliquid --format json search GOLD --select venue,coin\n  \
hyperliquid --format json search ETH --type perp --select coin")
    .args(&[
        arg("query").kind(InputKind::Identifier).describe("Search query, for example GOLD, BTC, PURR/USDC, or #10.").asset_input("asset search query"),
        arg("limit"),
        arg("type").describe("Restrict results to one venue."),
    ]);

    async fn run(inv: &Invocation, args: &AssetSearchArgs) -> Result<(), anyhow::Error> {
        search(inv, args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::AssetMetadata;
    use crate::assets::test_support::{hip3_perp, perp, spot};

    #[test]
    fn decodes_protocol_asset_id_ranges() {
        let cases = [
            (0, AssetIdKind::Perp, Some(0), None, None, None),
            (9_999, AssetIdKind::Perp, Some(9_999), None, None, None),
            (10_000, AssetIdKind::Spot, None, Some(0), None, None),
            (10_001, AssetIdKind::Spot, None, Some(1), None, None),
            (99_999, AssetIdKind::Spot, None, Some(89_999), None, None),
            (100_000, AssetIdKind::Hip3Perp, Some(0), None, Some(0), None),
            (
                110_035,
                AssetIdKind::Hip3Perp,
                Some(35),
                None,
                Some(1),
                None,
            ),
            (100_000_000, AssetIdKind::Outcome, None, None, None, Some(0)),
            (
                100_000_011,
                AssetIdKind::Outcome,
                None,
                None,
                None,
                Some(11),
            ),
        ];

        for (asset_id, kind, perp_index, spot_index, dex_slot, encoding) in cases {
            let decoded = decode_asset_id(asset_id, "mainnet");
            assert_eq!(decoded.kind, kind, "asset_id={asset_id}");
            assert_eq!(decoded.perp_index, perp_index, "asset_id={asset_id}");
            assert_eq!(decoded.spot_index, spot_index, "asset_id={asset_id}");
            assert_eq!(decoded.dex_slot, dex_slot, "asset_id={asset_id}");
            assert_eq!(decoded.encoding, encoding, "asset_id={asset_id}");
        }
    }

    #[test]
    fn outcome_side_one_uses_no_notation_metadata() {
        let decoded = decode_asset_id(100_000_401, "mainnet");

        assert_eq!(decoded.kind, AssetIdKind::Outcome);
        assert_eq!(decoded.lookup_status, LookupStatus::FormulaOnly);
        assert_eq!(decoded.encoding, Some(401));
        assert_eq!(decoded.outcome, Some(40));
        assert_eq!(decoded.side, Some(1));
        assert_eq!(decoded.side_name.as_deref(), Some("No"));
        assert_eq!(decoded.coin.as_deref(), Some("#401"));
        assert_eq!(decoded.token.as_deref(), Some("+401"));
        assert_eq!(decoded.cli_input.as_deref(), Some("#401"));
    }

    #[test]
    fn invalid_outcome_side_returns_machine_readable_invalid_result() {
        let decoded = decode_asset_id(100_000_012, "mainnet");

        assert_eq!(decoded.kind, AssetIdKind::Outcome);
        assert_eq!(decoded.lookup_status, LookupStatus::InvalidId);
        assert_eq!(decoded.side, Some(2));
        assert_eq!(decoded.cli_input, None);
        assert_eq!(
            decoded.reason.as_deref(),
            Some("only binary outcome sides 0 and 1 are supported")
        );
    }

    #[test]
    fn enriches_assets_from_loaded_metadata() {
        let resolver = AssetResolver::new(AssetMetadata::from_assets(
            vec![perp("BTC", 0), hip3_perp("dex", "TOKEN", 110_000)],
            vec![spot("PURR/USDC", 10_000, "PURR", "USDC")],
        ));

        let mut perp = decode_asset_id(0, "mainnet");
        enrich_from_loaded_resolver(&resolver, &mut perp);
        assert_eq!(perp.lookup_status, LookupStatus::MetadataFound);
        assert_eq!(perp.cli_input.as_deref(), Some("BTC"));

        let mut spot = decode_asset_id(10_000, "mainnet");
        enrich_from_loaded_resolver(&resolver, &mut spot);
        assert_eq!(spot.lookup_status, LookupStatus::MetadataFound);
        assert_eq!(spot.cli_input.as_deref(), Some("PURR/USDC"));

        let mut hip3 = decode_asset_id(110_000, "mainnet");
        enrich_from_loaded_resolver(&resolver, &mut hip3);
        assert_eq!(hip3.lookup_status, LookupStatus::MetadataFound);
        assert_eq!(hip3.cli_input.as_deref(), Some("dex:TOKEN"));
    }

    #[test]
    fn parses_outcome_description_into_human_readable_market_fields() {
        let parts = parse_outcome_description(
            "class:priceBinary|underlying:BTC|expiry:20260510-0000|targetPrice:100000|period:1d",
        );

        assert_eq!(parts.condition_class.as_deref(), Some("priceBinary"));
        assert_eq!(parts.underlying.as_deref(), Some("BTC"));
        assert_eq!(parts.expiry.as_deref(), Some("20260510-0000"));
        assert_eq!(parts.target_price.as_deref(), Some("100000"));
        assert_eq!(parts.period.as_deref(), Some("1d"));
        assert_eq!(
            parts.condition.as_deref(),
            Some("BTC above 100000 at May 10 00:00")
        );
        assert_eq!(
            outcome_market_title(&parts, "Yes", "Recurring").as_deref(),
            Some("BTC above 100000 Yes May 10 00:00")
        );
        assert_eq!(
            outcome_slug(&parts, "Yes").as_deref(),
            Some("btc-above-100000-yes-may-10-0000")
        );
    }

    #[test]
    fn strict_decimal_asset_id_parser_rejects_ambiguous_inputs() {
        for raw in ["", "abc", "-1", "1.5", "0x10"] {
            assert!(parse_asset_id(raw).is_err(), "{raw} should fail");
        }
        assert_eq!(parse_asset_id("100000400").unwrap(), 100_000_400);
    }
}
