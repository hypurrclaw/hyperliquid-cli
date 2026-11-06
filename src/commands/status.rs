//! API health and rate-limit status command.

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::command_spec::{CliCommand, CommandSpec, NoArgs};
use crate::errors::from_sdk;
use crate::invocation::Invocation;
use crate::output::TableData;
use crate::output::format::{duration, plural};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StatusOutput {
    pub network: String,
    pub api_url: String,
    pub health: String,
    pub latency_ms: u128,
    pub perps_count: usize,
    pub mids_count: usize,
    pub rate_limit_status: String,
    pub rate_limit_note: String,
}

impl TableData for StatusOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Network", "Health", "Latency", "Markets", "Rate Limits"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.network.clone(),
            self.health.clone(),
            duration(Duration::from_millis(
                u64::try_from(self.latency_ms).unwrap_or(u64::MAX),
            )),
            format!(
                "{}, {}",
                plural(self.perps_count, "perp", "perps"),
                plural(self.mids_count, "mid", "mids")
            ),
            self.rate_limit_status.clone(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

/// Query lightweight public endpoints and render API health/rate-limit status.
pub async fn show(inv: &Invocation) -> Result<(), anyhow::Error> {
    let remote = inv.remote()?;
    let client = remote.api().sdk();
    let probe_start = Instant::now();
    let perps = client.perps().await.map_err(from_sdk)?;
    let mids = client.all_mids(None).await.map_err(from_sdk)?;
    let latency = probe_start.elapsed();

    inv.print(&StatusOutput {
        network: remote.network().to_string(),
        api_url: remote.api().base_url().to_string(),
        health: "healthy".to_string(),
        latency_ms: latency.as_millis(),
        perps_count: perps.len(),
        mids_count: mids.len(),
        rate_limit_status: "ok".to_string(),
        rate_limit_note:
            "no rate-limit response observed; 429 responses map to exit code 11 and should be retried with backoff"
                .to_string(),
    });
    Ok(())
}

pub struct Status;

impl CliCommand for Status {
    type Args = NoArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["status"],
        "market",
        "Show API health and rate limits",
        "Show Hyperliquid API health and rate-limit context.",
    );

    async fn run(inv: &Invocation, _args: &NoArgs) -> Result<(), anyhow::Error> {
        show(inv).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_output_includes_health_and_rate_limit_fields() {
        let output = StatusOutput {
            network: "mainnet".to_string(),
            api_url: "https://api.hyperliquid.xyz".to_string(),
            health: "healthy".to_string(),
            latency_ms: 42,
            perps_count: 100,
            mids_count: 100,
            rate_limit_status: "ok".to_string(),
            rate_limit_note: "no rate-limit response observed".to_string(),
        };
        let json = output.to_json_value();

        assert_eq!(json["health"], "healthy");
        assert_eq!(json["rate_limit_status"], "ok");
        assert_eq!(output.rows()[0][3], "100 perps, 100 mids");
        assert_eq!(output.rows()[0][4], "ok");
        assert_eq!(output.rows()[0][2], "42ms");
        assert_eq!(json["rate_limit_note"], "no rate-limit response observed");
    }
}
