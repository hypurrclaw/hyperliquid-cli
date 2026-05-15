//! Protocol asset ID lookup commands.
//!
//! Commands:
//! - `hyperliquid asset decode <ASSET_ID>` — explain the protocol asset IDs surfaced in exchange errors.
//! - `hyperliquid asset search <QUERY>` — find assets by symbol, title, slug, notation, or ID.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use clap::Args;
use serde::Serialize;
use serde_json::{Map, Number, Value};
use strsim::levenshtein;

use crate::command_context::CommandContext;
use crate::commands::{AssetResolver, ResolvedAsset};
use crate::output::TableData;

const SPOT_ASSET_ID_OFFSET: u64 = 10_000;
const HIP3_ASSET_ID_OFFSET: u64 = 100_000;
const OUTCOME_ASSET_ID_OFFSET: u64 = 100_000_000;
const HIP3_DEX_SLOT_SIZE: u64 = 10_000;

#[derive(Args, Debug, Clone)]
pub struct AssetDecodeArgs {
    /// Raw protocol asset ID from exchange errors, schemas, or payloads
    #[arg(value_parser = parse_asset_id)]
    pub asset_id: u64,
}

#[derive(Args, Debug, Clone)]
pub struct AssetSearchArgs {
    /// Asset symbol, market title, app slug, outcome notation, or protocol asset ID
    pub query: String,

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
pub struct AssetDecodeResult {
    pub output: AssetDecodeOutput,
    pub elapsed: Duration,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetSearchResult {
    pub output: AssetSearchOutput,
    pub elapsed: Duration,
}

pub async fn decode_with_context(
    context: &CommandContext<'_>,
    args: &AssetDecodeArgs,
) -> Result<(), anyhow::Error> {
    let result = decode_query(context.api_base_url(), context.network(), args).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

pub async fn decode_query(
    api_base_url: &str,
    network: &str,
    args: &AssetDecodeArgs,
) -> Result<AssetDecodeResult, anyhow::Error> {
    let start = Instant::now();
    let mut decoded = decode_asset_id(args.asset_id, network);

    match decoded.kind {
        AssetIdKind::Outcome => enrich_outcome(api_base_url, &mut decoded).await,
        AssetIdKind::Perp | AssetIdKind::Spot | AssetIdKind::Hip3Perp => {
            enrich_from_resolver(api_base_url, &mut decoded).await
        }
    }

    Ok(AssetDecodeResult {
        output: AssetDecodeOutput::new(decoded),
        elapsed: start.elapsed(),
    })
}

pub async fn search_with_context(
    context: &CommandContext<'_>,
    args: &AssetSearchArgs,
) -> Result<(), anyhow::Error> {
    let result = search_query(context.api_base_url(), context.network(), args).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

pub async fn search_query(
    api_base_url: &str,
    network: &str,
    args: &AssetSearchArgs,
) -> Result<AssetSearchResult, anyhow::Error> {
    let start = Instant::now();
    let rows = search_assets(api_base_url, network, &args.query, args.limit).await;

    Ok(AssetSearchResult {
        output: AssetSearchOutput::new(rows),
        elapsed: start.elapsed(),
    })
}

#[must_use]
pub fn decode_asset_id(asset_id: u64, network: &str) -> DecodedAssetId {
    if asset_id >= OUTCOME_ASSET_ID_OFFSET {
        decode_outcome_asset_id(asset_id, network)
    } else if asset_id >= HIP3_ASSET_ID_OFFSET {
        decode_hip3_asset_id(asset_id, network)
    } else if asset_id >= SPOT_ASSET_ID_OFFSET {
        decode_spot_asset_id(asset_id, network)
    } else {
        decode_default_perp_asset_id(asset_id, network)
    }
}

fn decode_default_perp_asset_id(asset_id: u64, network: &str) -> DecodedAssetId {
    let mut decoded = DecodedAssetId::new(asset_id, AssetIdKind::Perp, network);
    decoded.perp_index = Some(asset_id);
    decoded.market_index = Some(asset_id);
    decoded
}

fn decode_spot_asset_id(asset_id: u64, network: &str) -> DecodedAssetId {
    let mut decoded = DecodedAssetId::new(asset_id, AssetIdKind::Spot, network);
    let spot_index = asset_id - SPOT_ASSET_ID_OFFSET;
    decoded.spot_index = Some(spot_index);
    decoded.market_index = Some(spot_index);
    decoded
}

fn decode_hip3_asset_id(asset_id: u64, network: &str) -> DecodedAssetId {
    let mut decoded = DecodedAssetId::new(asset_id, AssetIdKind::Hip3Perp, network);
    let encoded = asset_id - HIP3_ASSET_ID_OFFSET;
    decoded.dex_slot = Some(encoded / HIP3_DEX_SLOT_SIZE);
    decoded.perp_index = Some(encoded % HIP3_DEX_SLOT_SIZE);
    decoded.market_index = decoded.perp_index;
    decoded
}

fn decode_outcome_asset_id(asset_id: u64, network: &str) -> DecodedAssetId {
    let mut decoded = DecodedAssetId::new(asset_id, AssetIdKind::Outcome, network);
    let encoding = asset_id - OUTCOME_ASSET_ID_OFFSET;
    let outcome = encoding / 10;
    let side = encoding % 10;
    decoded.encoding = Some(encoding);
    decoded.outcome = Some(outcome);
    decoded.side = Some(side);
    decoded.coin = Some(format!("#{encoding}"));
    decoded.token = Some(format!("+{encoding}"));

    if side > 1 {
        decoded.lookup_status = LookupStatus::InvalidId;
        decoded.cli_input = None;
        decoded.reason = Some("only binary outcome sides 0 and 1 are supported".to_string());
    } else {
        decoded.cli_input = decoded.coin.clone();
        decoded.side_name = Some(
            match side {
                0 => "Yes",
                1 => "No",
                _ => unreachable!("side was already checked"),
            }
            .to_string(),
        );
    }

    decoded
}

async fn search_assets(
    api_base_url: &str,
    network: &str,
    query: &str,
    limit: usize,
) -> Vec<DecodedAssetId> {
    let mut candidates = Vec::new();

    let resolver = crate::commands::orders::load_perp_resolver_from_api_base(api_base_url)
        .await
        .ok();
    if let Some(resolver) = resolver.as_ref() {
        candidates.extend(search_resolver_assets(resolver, network));
    }

    let outcome_rows = crate::commands::outcomes::outcome_side_rows(api_base_url)
        .await
        .ok();
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

async fn enrich_from_resolver(api_base_url: &str, decoded: &mut DecodedAssetId) {
    let Ok(resolver) =
        crate::commands::orders::load_perp_resolver_from_api_base(api_base_url).await
    else {
        return;
    };
    enrich_from_loaded_resolver(&resolver, decoded);
}

pub fn enrich_from_loaded_resolver(resolver: &AssetResolver, decoded: &mut DecodedAssetId) {
    let Ok(asset_id) = usize::try_from(decoded.asset_id) else {
        return;
    };

    let resolved = match decoded.kind {
        AssetIdKind::Perp | AssetIdKind::Hip3Perp => resolver.perp_by_protocol_asset_id(asset_id),
        AssetIdKind::Spot => resolver.spot_by_protocol_asset_id(asset_id),
        AssetIdKind::Outcome => None,
    };

    match resolved {
        Some(ResolvedAsset::Perp {
            name,
            index: _,
            dex,
            sz_decimals: _,
            collateral: _,
        }) => {
            decoded.lookup_status = LookupStatus::MetadataFound;
            decoded.symbol = Some(name.clone());
            decoded.dex = dex.clone();
            decoded.cli_input = Some(match dex {
                Some(dex) => format!("{dex}:{name}"),
                None => name,
            });
        }
        Some(ResolvedAsset::Spot {
            symbol,
            index: _,
            base,
            quote,
            base_sz_decimals: _,
        }) => {
            decoded.lookup_status = LookupStatus::MetadataFound;
            decoded.cli_input = Some(symbol.clone());
            decoded.symbol = Some(symbol);
            decoded.base = Some(base);
            decoded.quote = Some(quote);
        }
        None => {}
    }
}

async fn enrich_outcome(api_base_url: &str, decoded: &mut DecodedAssetId) {
    if decoded.lookup_status == LookupStatus::InvalidId {
        return;
    }

    let Some(encoding) = decoded.encoding else {
        return;
    };
    let Ok(rows) = crate::commands::outcomes::outcome_side_rows(api_base_url).await else {
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
    if is_price_binary(parts)
        && let (Some(underlying), Some(target_price), Some(expiry)) = (
            parts.underlying.as_deref(),
            parts.target_price.as_deref(),
            parts.expiry.as_deref(),
        )
    {
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
    if is_price_binary(parts)
        && let (Some(underlying), Some(target_price), Some(expiry)) = (
            parts.underlying.as_deref(),
            parts.target_price.as_deref(),
            parts.expiry.as_deref(),
        )
    {
        let expiry = format_expiry_display(expiry).unwrap_or_else(|| expiry.to_string());
        return Some(format!(
            "{underlying} above {target_price} {side_name} {expiry}"
        ));
    }

    (!fallback_name.trim().is_empty()).then(|| fallback_name.to_string())
}

fn outcome_slug(parts: &OutcomeDescriptionParts, side_name: &str) -> Option<String> {
    if is_price_binary(parts)
        && let (Some(underlying), Some(target_price), Some(expiry)) = (
            parts.underlying.as_deref(),
            parts.target_price.as_deref(),
            parts.expiry.as_deref(),
        )
        && let Some(expiry_slug) = format_expiry_slug(expiry)
    {
        return Some(format!(
            "{}-above-{}-{}-{expiry_slug}",
            slug_token(underlying),
            slug_token(target_price),
            slug_token(side_name)
        ));
    }

    None
}

fn is_price_binary(parts: &OutcomeDescriptionParts) -> bool {
    parts
        .condition_class
        .as_deref()
        .is_some_and(|condition_class| condition_class.eq_ignore_ascii_case("priceBinary"))
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

fn parse_positive_usize(raw: &str) -> Result<usize, String> {
    let value = raw
        .parse::<usize>()
        .map_err(|err| format!("invalid positive integer: {err}"))?;
    if value == 0 {
        return Err("value must be greater than zero".to_string());
    }
    Ok(value)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{AssetMetadata, PerpAsset, SpotAsset};

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
            vec![
                PerpAsset::default_dex("BTC", 0),
                PerpAsset::hip3("dex", "TOKEN", 110_000),
            ],
            vec![SpotAsset::new("PURR/USDC", 10_000, "PURR", "USDC")],
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
