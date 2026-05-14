//! Gossip priority auction commands.

use std::time::{Duration, Instant};

use clap::Args;
use hypersdk::Decimal;
use hypersdk::hypercore::types::{Action, GossipPriorityBid};
use hypersdk::hypercore::{Chain, HttpClient};
use rust_decimal::prelude::FromPrimitive;
use serde::Serialize;

use crate::commands::actions::{nonce_now, send_l1_action_raw};
use crate::commands::map_api_error;
use crate::errors::CliError;
use crate::output::{OutputFormat, TableData};
use crate::signing::SelectedSigner;

/// Arguments for `prio bid`.
#[derive(Args, Debug, Clone)]
pub struct BidArgs {
    /// Maximum HYPE to bid. You pay the live current-gas price up to this cap.
    #[arg(long)]
    pub max: Decimal,

    /// IP address to receive prioritized gossip data.
    #[arg(long)]
    pub ip: String,

    /// Slot index (0-4), where 0 is highest priority.
    #[arg(long, default_value = "0", value_parser = clap::value_parser!(u8).range(0..=4))]
    pub slot: u8,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PrioritySlotRow {
    slot: usize,
    start_time_seconds: u64,
    duration_seconds: u64,
    start_gas: String,
    current_gas: String,
    end_gas: String,
    previous_winner: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorityStatusOutput {
    rows: Vec<PrioritySlotRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorityStatusResult {
    pub output: PriorityStatusOutput,
    pub elapsed: Duration,
}

impl TableData for PriorityStatusOutput {
    fn headers(&self) -> Vec<&str> {
        vec![
            "Slot",
            "Start Time",
            "Duration",
            "Start Gas",
            "Current Gas",
            "End Gas",
            "Previous Winner",
        ]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.slot.to_string(),
                    row.start_time_seconds.to_string(),
                    row.duration_seconds.to_string(),
                    row.start_gas.clone(),
                    row.current_gas.clone(),
                    row.end_gas.clone(),
                    row.previous_winner.clone(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PriorityBidRow {
    slot: u8,
    ip: String,
    max_hype: String,
    bid_hype: String,
    status: String,
}

struct PriorityBidOutput {
    rows: Vec<PriorityBidRow>,
}

impl TableData for PriorityBidOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Slot", "IP", "Max HYPE", "Bid HYPE", "Status"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows
            .iter()
            .map(|row| {
                vec![
                    row.slot.to_string(),
                    row.ip.clone(),
                    row.max_hype.clone(),
                    row.bid_hype.clone(),
                    row.status.clone(),
                ]
            })
            .collect()
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.rows).unwrap_or_else(|_| serde_json::json!([]))
    }
}

pub async fn status(client: &HttpClient, format: OutputFormat) -> Result<(), anyhow::Error> {
    let result = status_query(client).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

pub async fn status_query(client: &HttpClient) -> Result<PriorityStatusResult, anyhow::Error> {
    let start = Instant::now();
    let status = client
        .gossip_priority_auction_status()
        .await
        .map_err(map_api_error)?;

    let rows = status
        .slots
        .iter()
        .enumerate()
        .map(|(slot, item)| PrioritySlotRow {
            slot,
            start_time_seconds: item.start_time_seconds,
            duration_seconds: item.duration_seconds,
            start_gas: item.start_gas.clone(),
            current_gas: item
                .current_gas
                .clone()
                .unwrap_or_else(|| "(no bid)".to_string()),
            end_gas: item.end_gas.clone().unwrap_or_else(|| "-".to_string()),
            previous_winner: status
                .prev_winners
                .get(slot)
                .and_then(Clone::clone)
                .unwrap_or_else(|| "-".to_string()),
        })
        .collect();

    Ok(PriorityStatusResult {
        output: PriorityStatusOutput { rows },
        elapsed: start.elapsed(),
    })
}

pub async fn bid(
    api_base_url: &str,
    client: &HttpClient,
    signer: &SelectedSigner,
    chain: Chain,
    args: &BidArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let start = Instant::now();
    if args.max <= Decimal::ZERO {
        return Err(
            CliError::Configuration("prio bid requires --max to be positive".to_string()).into(),
        );
    }
    if args.ip.trim().is_empty() {
        return Err(
            CliError::Configuration("prio bid requires a non-empty --ip".to_string()).into(),
        );
    }
    let max = args.max;
    let ip = args.ip.as_str();
    signer.ensure_can_attempt_live_signing()?;

    let decimals = client
        .spot_tokens()
        .await
        .map_err(map_api_error)?
        .into_iter()
        .find(|token| token.name == "HYPE")
        .map(|token| token.wei_decimals as u32)
        .unwrap_or(18);
    let max_gas = hype_to_wei_u64(max, decimals, "--max")?;
    if max_gas == 0 {
        return Err(CliError::Configuration(
            "--max is too small for HYPE wei precision".to_string(),
        )
        .into());
    }

    let status = client
        .gossip_priority_auction_status()
        .await
        .map_err(map_api_error)?;
    let slot = status
        .slots
        .get(args.slot as usize)
        .ok_or_else(|| CliError::Configuration(format!("invalid priority slot {}", args.slot)))?;
    let current = match slot.current_gas.as_deref() {
        Some(gas) => {
            let parsed = gas.parse::<Decimal>().map_err(|err| {
                CliError::Internal(anyhow::anyhow!(
                    "invalid current gas value {gas:?} from priority auction status: {err}"
                ))
            })?;
            hype_to_wei_u64(parsed, decimals, "current gas")?
        }
        None => 0,
    };

    if current >= max_gas && current > 0 {
        return Err(CliError::Unsupported(format!(
            "priority bid not submitted: current gas {} is at or above --max {max}",
            fmt_wei(current, decimals)
        ))
        .into());
    }

    let bid = if current > 0 { current + 1 } else { max_gas };
    let response = send_l1_action_raw(
        api_base_url,
        chain,
        signer,
        Action::GossipPriorityBid(GossipPriorityBid {
            slot_id: args.slot,
            ip: ip.to_string(),
            max_gas: bid,
        }),
        nonce_now(),
        None,
        "priority bid",
    )
    .await?;
    let status = match response {
        serde_json::Value::Object(map)
            if map.get("type") == Some(&serde_json::Value::String("default".to_string())) =>
        {
            "submitted".to_string()
        }
        other => format!("submitted with response: {other:?}"),
    };

    let row = PriorityBidRow {
        slot: args.slot,
        ip: ip.to_string(),
        max_hype: max.to_string(),
        bid_hype: fmt_wei(bid, decimals).to_string(),
        status,
    };
    crate::output::print_data(
        &PriorityBidOutput { rows: vec![row] },
        format,
        start.elapsed(),
    );
    Ok(())
}

fn fmt_wei(wei: u64, decimals: u32) -> Decimal {
    Decimal::from_u64(wei).unwrap_or_default()
        / Decimal::from_u64(10u64.saturating_pow(decimals)).unwrap_or(Decimal::ONE)
}

fn hype_to_wei_u64(amount: Decimal, decimals: u32, label: &str) -> Result<u64, CliError> {
    hypersdk::hyperevm::to_wei(amount, decimals)
        .try_into()
        .map_err(|_| CliError::Configuration(format!("{label} is too large")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_output_has_auction_columns() {
        let output = PriorityStatusOutput { rows: vec![] };

        assert!(output.headers().contains(&"Current Gas"));
        assert!(output.headers().contains(&"Previous Winner"));
    }

    #[test]
    fn decimal_current_gas_uses_hype_to_wei_conversion() {
        assert_eq!(
            hype_to_wei_u64("0.01".parse::<Decimal>().unwrap(), 18, "current gas").unwrap(),
            10_000_000_000_000_000
        );
    }
}
