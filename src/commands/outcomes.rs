//! Outcome market discovery commands.
//!
//! Commands:
//! - `hyperliquid outcomes list` — list active outcome market sides
//! - `hyperliquid outcomes get <NOTATION>` — inspect an outcome side by `#N` or `+N`

use clap::Args;
use serde::{Deserialize, Serialize};

use crate::assets::asset_id::{
    outcome_asset_id, outcome_coin, outcome_encoding, outcome_token, parse_outcome_notation,
};
use crate::command_spec::{CliCommand, CommandSpec, GroupSpec, arg};
use crate::errors::CliError;
use crate::http_api::ApiClient;
use crate::invocation::Invocation;
use crate::output::TableData;

#[derive(Args, Debug, Clone)]
pub struct OutcomeListArgs {
    /// Maximum number of outcome side rows to display
    #[arg(long, default_value = "100", value_parser = parse_positive_usize)]
    pub limit: usize,
}

#[derive(Args, Debug, Clone)]
pub struct OutcomeGetArgs {
    /// Outcome notation, for example #10 or +10
    pub notation: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OutcomeSideRow {
    pub outcome: u64,
    pub side: u64,
    pub encoding: u64,
    pub coin: String,
    pub token: String,
    pub asset_id: u64,
    pub outcome_name: String,
    pub side_name: String,
    pub description: String,
    pub side_token: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeSidesOutput {
    rows: Vec<OutcomeSideRow>,
}

impl OutcomeSidesOutput {
    #[must_use]
    pub fn new(rows: Vec<OutcomeSideRow>) -> Self {
        Self { rows }
    }
}

impl TableData for OutcomeSidesOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Outcome",
            "Side",
            "Encoding",
            "Coin",
            "Token",
            "Asset ID",
            "Outcome Name",
            "Side Name",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.outcome.to_string(),
                    row.side.to_string(),
                    row.encoding.to_string(),
                    row.coin.clone(),
                    row.token.clone(),
                    row.asset_id.to_string(),
                    row.outcome_name.clone(),
                    row.side_name.clone(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeSideOutput {
    row: OutcomeSideRow,
}

impl OutcomeSideOutput {
    #[must_use]
    pub fn new(row: OutcomeSideRow) -> Self {
        Self { row }
    }
}

impl TableData for OutcomeSideOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Outcome",
            "Side",
            "Encoding",
            "Coin",
            "Token",
            "Asset ID",
            "Outcome Name",
            "Side Name",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        OutcomeSidesOutput::new(vec![self.row.clone()]).rows()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.row).unwrap_or_else(|_| serde_json::json!({}))
    }
}

pub async fn list(inv: &Invocation, args: &OutcomeListArgs) -> Result<(), anyhow::Error> {
    let mut rows = outcome_side_rows(inv.remote()?.api()).await?;
    rows.truncate(args.limit);
    inv.print(&OutcomeSidesOutput::new(rows));
    Ok(())
}

pub async fn get(inv: &Invocation, args: &OutcomeGetArgs) -> Result<(), anyhow::Error> {
    let parsed = parse_outcome_notation(&args.notation)?;
    let row = outcome_side_rows(inv.remote()?.api())
        .await?
        .into_iter()
        .find(|row| row.encoding == parsed.encoding)
        .ok_or_else(|| {
            CliError::Unsupported(format!(
                "outcome notation '{}' was not found in outcomeMeta",
                args.notation
            ))
        })?;
    inv.print(&OutcomeSideOutput::new(row));
    Ok(())
}

pub async fn outcome_side_rows(api: &ApiClient) -> Result<Vec<OutcomeSideRow>, CliError> {
    let response = api
        .info::<OutcomeMetaResponse>(
            &OutcomeMetaRequest {
                request_type: "outcomeMeta",
            },
            "loading outcome metadata",
        )
        .await?;
    Ok(rows_from_meta(response))
}

fn rows_from_meta(response: OutcomeMetaResponse) -> Vec<OutcomeSideRow> {
    let mut rows = response
        .outcomes
        .into_iter()
        .flat_map(|outcome| {
            let outcome_id = outcome.outcome;
            outcome
                .side_specs
                .into_iter()
                .enumerate()
                .filter_map(move |(side, side_spec)| {
                    let side = u64::try_from(side).ok()?;
                    outcome_encoding(outcome_id, side)
                        .ok()
                        .and_then(|encoding| {
                            let asset_id = outcome_asset_id(encoding).ok()?;
                            let coin = outcome_coin(encoding);
                            let token = outcome_token(encoding);
                            Some(OutcomeSideRow {
                                outcome: outcome_id,
                                side,
                                encoding,
                                coin,
                                token,
                                asset_id,
                                outcome_name: outcome.name.clone(),
                                side_name: side_spec.name,
                                description: outcome.description.clone(),
                                side_token: side_spec.token,
                            })
                        })
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| (row.outcome, row.side));
    rows
}

pub fn unsupported_outcome_trading_error(raw: &str) -> CliError {
    CliError::Unsupported(format!(
        "outcome asset notation '{raw}' requires an outcome-aware command path; use `hyperliquid outcomes get {raw}` to inspect the encoded HIP-4 asset id. Live limit orders support outcome notation."
    ))
}

pub(crate) fn parse_positive_usize(raw: &str) -> Result<usize, String> {
    let value = raw
        .parse::<usize>()
        .map_err(|err| format!("invalid positive integer: {err}"))?;
    if value == 0 {
        return Err("value must be greater than zero".to_string());
    }
    Ok(value)
}

#[derive(Debug, Serialize)]
struct OutcomeMetaRequest {
    #[serde(rename = "type")]
    request_type: &'static str,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct OutcomeMetaResponse {
    outcomes: Vec<OutcomeMetaOutcome>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct OutcomeMetaOutcome {
    outcome: u64,
    name: String,
    description: String,
    #[serde(default)]
    side_specs: Vec<OutcomeSideSpec>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct OutcomeSideSpec {
    name: String,
    token: Option<u64>,
}

pub const GROUP: GroupSpec =
    GroupSpec::new(&["outcomes"], "Browse outcome markets").aliases(&["outcome"]);

pub struct OutcomesList;

impl CliCommand for OutcomesList {
    type Args = OutcomeListArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["outcomes", "list"],
        "market",
        "List active outcome market sides",
        "List active outcome market sides from outcomeMeta. Use orders create with #<encoding> or +<encoding> notation for verified outcome limit orders.",
    )
    .cli_aliases(&["ls"])
    .args(&[
        arg("limit").describe("Maximum number of outcome side rows to return."),
    ]);

    async fn run(inv: &Invocation, args: &OutcomeListArgs) -> Result<(), anyhow::Error> {
        list(inv, args).await
    }
}

pub struct OutcomesGet;

impl CliCommand for OutcomesGet {
    type Args = OutcomeGetArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["outcomes", "get"],
        "market",
        "Get outcome side metadata by #N or +N notation",
        "Show outcome side metadata by #N coin notation or +N token notation.",
    )
    .args(&[arg("notation")]);

    fn validate(args: &OutcomeGetArgs) -> Result<(), CliError> {
        crate::assets::asset_id::parse_outcome_notation(&args.notation)?;
        Ok(())
    }

    async fn run(inv: &Invocation, args: &OutcomeGetArgs) -> Result<(), anyhow::Error> {
        get(inv, args).await
    }
}
