//! Outcome market discovery commands.
//!
//! Commands:
//! - `hyperliquid outcomes list` — list active outcome market sides
//! - `hyperliquid outcomes get <NOTATION>` — inspect an outcome side by `#N` or `+N`

use std::time::Instant;

use clap::Args;
use serde::{Deserialize, Serialize};

use crate::errors::CliError;
use crate::http_api::post_info_json;
use crate::output::{OutputFormat, TableData};

const OUTCOME_ASSET_ID_OFFSET: u64 = 100_000_000;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeSidesResult {
    pub output: OutcomeSidesOutput,
    pub elapsed: std::time::Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeSideResult {
    pub output: OutcomeSideOutput,
    pub elapsed: std::time::Duration,
}

pub async fn list(
    api_base_url: &str,
    args: &OutcomeListArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = list_query(api_base_url, args).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

pub async fn get(
    api_base_url: &str,
    args: &OutcomeGetArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = get_query(api_base_url, args).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

pub async fn list_query(
    api_base_url: &str,
    args: &OutcomeListArgs,
) -> Result<OutcomeSidesResult, anyhow::Error> {
    let start = Instant::now();
    let mut rows = outcome_side_rows(api_base_url).await?;
    rows.truncate(args.limit);
    Ok(OutcomeSidesResult {
        output: OutcomeSidesOutput::new(rows),
        elapsed: start.elapsed(),
    })
}

pub async fn get_query(
    api_base_url: &str,
    args: &OutcomeGetArgs,
) -> Result<OutcomeSideResult, anyhow::Error> {
    let start = Instant::now();
    let parsed = parse_outcome_notation(&args.notation)?;
    let row = outcome_side_rows(api_base_url)
        .await?
        .into_iter()
        .find(|row| row.encoding == parsed.encoding)
        .ok_or_else(|| {
            CliError::Unsupported(format!(
                "outcome notation '{}' was not found in outcomeMeta",
                args.notation
            ))
        })?;
    Ok(OutcomeSideResult {
        output: OutcomeSideOutput::new(row),
        elapsed: start.elapsed(),
    })
}

pub async fn outcome_side_rows(api_base_url: &str) -> Result<Vec<OutcomeSideRow>, CliError> {
    let response = post_info_json::<OutcomeMetaResponse>(
        api_base_url,
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
                            let coin = format!("#{encoding}");
                            let token = format!("+{encoding}");
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

pub fn parse_outcome_notation(raw: &str) -> Result<OutcomeNotation, CliError> {
    let trimmed = raw.trim();
    let Some(rest) = trimmed
        .strip_prefix('#')
        .or_else(|| trimmed.strip_prefix('+'))
    else {
        return Err(outcome_notation_error(raw));
    };
    if rest.is_empty() || !rest.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(outcome_notation_error(raw));
    }
    let encoding = rest
        .parse::<u64>()
        .map_err(|_| outcome_notation_error(raw))?;
    let outcome = encoding / 10;
    let side = encoding % 10;
    if side > 1 {
        return Err(CliError::Unsupported(format!(
            "invalid outcome notation '{raw}': only binary outcome sides 0 and 1 are supported"
        )));
    }
    Ok(OutcomeNotation {
        outcome,
        side,
        encoding,
    })
}

pub fn outcome_encoding(outcome: u64, side: u64) -> Result<u64, CliError> {
    if side > 1 {
        return Err(CliError::Unsupported(
            "only binary outcome sides 0 and 1 are supported".to_string(),
        ));
    }
    outcome
        .checked_mul(10)
        .and_then(|value| value.checked_add(side))
        .ok_or_else(|| CliError::Unsupported("outcome encoding overflowed u64".to_string()))
}

pub fn outcome_asset_id(encoding: u64) -> Result<u64, CliError> {
    OUTCOME_ASSET_ID_OFFSET
        .checked_add(encoding)
        .ok_or_else(|| CliError::Unsupported("outcome asset id overflowed".to_string()))
}

pub fn outcome_notation_error(raw: &str) -> CliError {
    CliError::Unsupported(format!(
        "invalid outcome notation '{raw}': expected #<encoding> or +<encoding>, for example #10"
    ))
}

pub fn unsupported_outcome_trading_error(raw: &str) -> CliError {
    CliError::Unsupported(format!(
        "outcome asset notation '{raw}' requires an outcome-aware command path; use `hyperliquid outcomes get {raw}` to inspect the encoded HIP-4 asset id. Live limit orders support outcome notation."
    ))
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeNotation {
    pub outcome: u64,
    pub side: u64,
    pub encoding: u64,
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
    #[serde(default)]
    questions: Vec<serde_json::Value>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_encoding_uses_official_formula() {
        assert_eq!(outcome_encoding(1, 0).unwrap(), 10);
        assert_eq!(outcome_encoding(1, 1).unwrap(), 11);
        assert!(outcome_encoding(1, 2).is_err());
    }

    #[test]
    fn outcome_asset_id_checks_overflow() {
        assert_eq!(outcome_asset_id(10).unwrap(), OUTCOME_ASSET_ID_OFFSET + 10);
        assert!(outcome_asset_id(u64::MAX).is_err());
    }

    #[test]
    fn parse_outcome_notation_accepts_coin_and_token_forms() {
        assert_eq!(
            parse_outcome_notation("#10").unwrap(),
            OutcomeNotation {
                outcome: 1,
                side: 0,
                encoding: 10
            }
        );
        assert_eq!(
            parse_outcome_notation("+11").unwrap(),
            OutcomeNotation {
                outcome: 1,
                side: 1,
                encoding: 11
            }
        );
    }

    #[test]
    fn parse_outcome_notation_rejects_malformed_values() {
        assert!(parse_outcome_notation("#abc").is_err());
        assert!(parse_outcome_notation("10").is_err());
        assert!(parse_outcome_notation("#12").is_err());
    }
}
