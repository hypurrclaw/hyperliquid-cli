//! API health and rate-limit status command.

use std::time::{Duration, Instant};

use hypersdk::hypercore::HttpClient;
use serde::Serialize;

use crate::command_context::CommandContext;
use crate::commands::map_api_error;
use crate::output::{OutputFormat, TableData};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusResult {
    pub output: StatusOutput,
    pub elapsed: Duration,
}

impl TableData for StatusOutput {
    fn headers(&self) -> Vec<&str> {
        vec!["Network", "Health", "Latency", "Markets", "Rate Limits"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.network.clone(),
            self.health.clone(),
            format!("{}ms", self.latency_ms),
            format!("{} perps, {} mids", self.perps_count, self.mids_count),
            format!("{} — {}", self.rate_limit_status, self.rate_limit_note),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

/// Query lightweight public endpoints and return typed API health/rate-limit status.
pub async fn query(
    client: &HttpClient,
    network: impl Into<String>,
    api_url: impl Into<String>,
) -> Result<StatusResult, anyhow::Error> {
    let start = Instant::now();
    let output = query_output(client, network, api_url).await?;

    Ok(StatusResult {
        output,
        elapsed: start.elapsed(),
    })
}

async fn query_output(
    client: &HttpClient,
    network: impl Into<String>,
    api_url: impl Into<String>,
) -> Result<StatusOutput, anyhow::Error> {
    let probe_start = Instant::now();
    let perps = client.perps().await.map_err(map_api_error)?;
    let mids = client.all_mids(None).await.map_err(map_api_error)?;
    let latency = probe_start.elapsed();

    Ok(StatusOutput {
        network: network.into(),
        api_url: api_url.into(),
        health: "healthy".to_string(),
        latency_ms: duration_millis(latency),
        perps_count: perps.len(),
        mids_count: mids.len(),
        rate_limit_status: "ok".to_string(),
        rate_limit_note:
            "no rate-limit response observed; 429 responses map to exit code 11 and should be retried with backoff"
                .to_string(),
    })
}

/// Query lightweight public endpoints and render API health/rate-limit status
/// through a per-call output context.
pub async fn show_with_context(context: &CommandContext<'_>) -> Result<(), anyhow::Error> {
    let client = context
        .hypercore_client()
        .ok_or_else(|| anyhow::anyhow!("status command requires a Hyperliquid HTTP client"))?;
    let result = query(client, context.network(), context.api_base_url()).await?;
    context.print(&result.output, result.elapsed);
    Ok(())
}

/// Query lightweight public endpoints and render API health/rate-limit status.
pub async fn show(
    client: &HttpClient,
    network: impl Into<String>,
    api_url: impl Into<String>,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    let result = query(client, network, api_url).await?;
    crate::output::print_data(&result.output, format, result.elapsed);
    Ok(())
}

fn duration_millis(duration: Duration) -> u128 {
    duration.as_millis()
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
        assert!(output.rows()[0][4].contains("rate-limit"));
    }
}
