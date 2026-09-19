//! Read-only position risk watcher.

use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::{Args, ValueEnum};
use hypersdk::hypercore::HttpClient;
use hypersdk::{Address, Decimal};
use serde::Serialize;
use serde_json::json;

use crate::commands::map_api_error;
use crate::errors::CliError;
use crate::output::{self, OutputFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "snake_case")]
pub enum RiskRule {
    LiqProximity,
    MarginUsage,
    NoTpsl,
    StaleOrder,
    FundingBleed,
    PositionLifecycle,
}

#[derive(Args, Debug, Clone)]
pub struct WatchRiskArgs {
    /// Protocol user address to watch. Defaults to the selected signer.
    #[arg(long)]
    pub user: Option<String>,
    #[arg(long, value_enum, default_value = "info")]
    pub min_severity: AlertSeverity,
    #[arg(long = "disable", value_enum)]
    pub disable: Vec<RiskRule>,
    #[arg(long = "only", value_enum)]
    pub only: Vec<RiskRule>,
    #[arg(long, default_value = "15s", value_parser = parse_secs)]
    pub interval: Duration,
    #[arg(long)]
    pub max_events: Option<usize>,
    #[arg(long)]
    pub idle_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Serialize)]
#[clap(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize)]
pub struct RiskAlert {
    pub kind: &'static str,
    pub time: u64,
    pub address: String,
    pub rule: String,
    pub coin: Option<String>,
    pub severity: AlertSeverity,
    pub message: String,
    pub payload: serde_json::Value,
}

pub fn parse_watch_user(raw: &str) -> Result<Address, CliError> {
    raw.parse::<Address>()
        .map_err(|_| CliError::Unsupported(format!("Invalid address: {raw}")))
}

pub async fn watch_risk(
    client: &HttpClient,
    user: Address,
    args: &WatchRiskArgs,
    format: OutputFormat,
) -> Result<(), anyhow::Error> {
    if agent_mode() && args.max_events.is_none() && args.idle_timeout_ms.is_none() {
        return Err(CliError::Configuration(
            "watch risk requires --max-events or --idle-timeout-ms in agent mode.\n  \
             hyperliquid --format json watch risk --user 0x0000000000000000000000000000000000000001 --max-events 20"
                .to_string(),
        )
        .into());
    }

    let enabled = enabled_rules(args);
    let mut emitted = 0usize;
    let mut last_sizes: HashMap<String, Decimal> = HashMap::new();
    let mut first_seen: HashMap<String, Instant> = HashMap::new();
    let mut funding_seen: HashMap<String, Instant> = HashMap::new();
    let mut funding_baseline: HashMap<String, Decimal> = HashMap::new();
    let mut funding_alerted: HashSet<String> = HashSet::new();
    let mut flat_announced = false;
    // Idle means "no output": the clock restarts on every emitted event so an
    // actively alerting stream is not cut off mid-incident.
    let idle = args
        .idle_timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(60 * 60));
    let mut last_output = Instant::now();

    // Seed position ages from fill history so positions opened before this
    // watcher started do not wait a full threshold window for age-based rules.
    let fills = client.user_fills(user).await.unwrap_or_default();
    let open_times = position_open_times(&fills);

    loop {
        if last_output.elapsed() > idle {
            break;
        }
        if let Some(max) = args.max_events
            && emitted >= max
        {
            break;
        }

        let state = client
            .clearinghouse_state(user, None)
            .await
            .map_err(map_api_error)?;
        let open_orders = client.open_orders(user, None).await.unwrap_or_default();
        let mids = client.all_mids(None).await.unwrap_or_default();
        let equity = state.margin_summary.account_value;
        let margin_used = state.margin_summary.total_margin_used;

        let mut eval = RiskEvalState {
            last_sizes: &mut last_sizes,
            first_seen: &mut first_seen,
            funding_seen: &mut funding_seen,
            funding_baseline: &mut funding_baseline,
            funding_alerted: &mut funding_alerted,
            open_times: &open_times,
        };
        let snapshot = RiskSnapshot {
            positions: &state.asset_positions,
            open_orders: &open_orders,
            mids: &mids,
            equity,
            margin_used,
        };
        let mut alerts = evaluate_state(user, &enabled, snapshot, &mut eval);

        let no_positions = state
            .asset_positions
            .iter()
            .all(|position| position.position.szi.is_zero());
        if no_positions {
            // Heartbeat once per flat transition; a flat account keeps
            // monitoring so positions opened later are still observed.
            if !flat_announced {
                alerts.push(alert(
                    user,
                    "heartbeat",
                    None,
                    AlertSeverity::Info,
                    "no open positions".to_string(),
                    json!({ "positions": 0 }),
                ));
                flat_announced = true;
            }
        } else {
            flat_announced = false;
        }

        for alert in alerts {
            if alert.severity < args.min_severity {
                continue;
            }
            match format {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string(&alert)?);
                }
                _ => {
                    output::print_data(&AlertTable(alert), format, Duration::from_millis(0));
                }
            }
            emitted += 1;
            last_output = Instant::now();
            if let Some(max) = args.max_events
                && emitted >= max
            {
                return Ok(());
            }
        }

        tokio::time::sleep(args.interval).await;
    }
    Ok(())
}

fn agent_mode() -> bool {
    !std::io::stdout().is_terminal()
        || std::env::var("HYPERLIQUID_AGENT").is_ok_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
}
struct RiskSnapshot<'a> {
    positions: &'a [hypersdk::hypercore::types::AssetPosition],
    open_orders: &'a [hypersdk::hypercore::types::BasicOrder],
    mids: &'a std::collections::HashMap<String, Decimal>,
    equity: Decimal,
    margin_used: Decimal,
}

struct RiskEvalState<'a> {
    last_sizes: &'a mut HashMap<String, Decimal>,
    first_seen: &'a mut HashMap<String, Instant>,
    funding_seen: &'a mut HashMap<String, Instant>,
    funding_baseline: &'a mut HashMap<String, Decimal>,
    funding_alerted: &'a mut HashSet<String>,
    /// Coin → position open time (ms since epoch) seeded from fill history.
    open_times: &'a HashMap<String, u64>,
}

fn position_open_times(fills: &[hypersdk::hypercore::types::Fill]) -> HashMap<String, u64> {
    let mut open_times: HashMap<String, u64> = HashMap::new();
    let mut oldest: HashMap<String, u64> = HashMap::new();
    for fill in fills {
        oldest
            .entry(fill.coin.clone())
            .and_modify(|time| *time = (*time).min(fill.time))
            .or_insert(fill.time);
        let signed_sz = if fill.side == hypersdk::hypercore::types::Side::Bid {
            fill.sz
        } else {
            -fill.sz
        };
        let after = fill.start_position + signed_sz;
        let opened_here = fill.start_position.is_zero()
            || fill.start_position.is_sign_positive() != after.is_sign_positive();
        if opened_here {
            open_times
                .entry(fill.coin.clone())
                .and_modify(|time| *time = (*time).max(fill.time))
                .or_insert(fill.time);
        }
    }
    for (coin, time) in oldest {
        open_times.entry(coin).or_insert(time);
    }
    open_times
}

/// Instant approximating a position open time in ms since epoch.
fn open_instant(open_times: &HashMap<String, u64>, coin: &str) -> Option<Instant> {
    let ms = *open_times.get(coin)?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if ms >= now_ms {
        return None;
    }
    Instant::now().checked_sub(Duration::from_millis(now_ms - ms))
}

fn evaluate_state(
    user: Address,
    enabled: &[RiskRule],
    snapshot: RiskSnapshot<'_>,
    state: &mut RiskEvalState<'_>,
) -> Vec<RiskAlert> {
    let mut alerts = Vec::new();
    if enabled.contains(&RiskRule::MarginUsage)
        && snapshot.equity > Decimal::ZERO
        && snapshot.margin_used * Decimal::from(100) / snapshot.equity >= Decimal::from(80)
    {
        let pct = snapshot.margin_used * Decimal::from(100) / snapshot.equity;
        alerts.push(alert(
            user,
            "margin_usage",
            None,
            AlertSeverity::Warning,
            format!("margin used {pct}% of equity"),
            json!({ "margin_used_pct": pct.to_string() }),
        ));
    }

    // Detect positions that closed by disappearing from the snapshot entirely:
    // a coin remembered with nonzero size that is absent now has closed.
    if enabled.contains(&RiskRule::PositionLifecycle) {
        let present: HashSet<String> = snapshot
            .positions
            .iter()
            .map(|position| position.position.coin.clone())
            .collect();
        let vanished: Vec<String> = state
            .last_sizes
            .iter()
            .filter(|(coin, size)| !size.is_zero() && !present.contains(*coin))
            .map(|(coin, _)| coin.clone())
            .collect();
        for coin in vanished {
            alerts.push(alert(
                user,
                "position_lifecycle",
                Some(&coin),
                AlertSeverity::Info,
                format!("{coin} closed"),
                json!({ "size": "0" }),
            ));
            state.last_sizes.insert(coin.clone(), Decimal::ZERO);
            state.first_seen.remove(&coin);
            state.funding_seen.remove(&coin);
            state.funding_baseline.remove(&coin);
            state.funding_alerted.remove(&coin);
        }
    }

    for position in snapshot.positions {
        let coin = position.position.coin.clone();
        let size = position.position.szi;
        if enabled.contains(&RiskRule::PositionLifecycle) {
            match state.last_sizes.get(&coin) {
                None if !size.is_zero() => alerts.push(alert(
                    user,
                    "position_lifecycle",
                    Some(&coin),
                    AlertSeverity::Info,
                    format!("{coin} opened"),
                    json!({ "size": size.to_string() }),
                )),
                Some(prev) if prev.is_zero() && !size.is_zero() => alerts.push(alert(
                    user,
                    "position_lifecycle",
                    Some(&coin),
                    AlertSeverity::Info,
                    format!("{coin} opened"),
                    json!({ "size": size.to_string() }),
                )),
                Some(prev) if !prev.is_zero() && size.is_zero() => alerts.push(alert(
                    user,
                    "position_lifecycle",
                    Some(&coin),
                    AlertSeverity::Info,
                    format!("{coin} closed"),
                    json!({ "size": size.to_string() }),
                )),
                Some(prev) if *prev != size => alerts.push(alert(
                    user,
                    "position_lifecycle",
                    Some(&coin),
                    AlertSeverity::Info,
                    format!("{coin} resized"),
                    json!({ "from": prev.to_string(), "to": size.to_string() }),
                )),
                _ => {}
            }
        }
        state.last_sizes.insert(coin.clone(), size);
        if size.is_zero() {
            state.first_seen.remove(&coin);
            state.funding_seen.remove(&coin);
            state.funding_baseline.remove(&coin);
            state.funding_alerted.remove(&coin);
            continue;
        }
        // Seed from fill history when available so pre-existing positions do
        // not restart their age clocks at watcher startup.
        let seeded = open_instant(state.open_times, &coin);
        state
            .first_seen
            .entry(coin.clone())
            .or_insert_with(|| seeded.unwrap_or_else(Instant::now));

        if enabled.contains(&RiskRule::LiqProximity)
            && let Some(liq) = position.position.liquidation_px
        {
            let mark = if position.position.abs_size().is_zero() {
                Decimal::ZERO
            } else {
                position.position.position_value / position.position.abs_size()
            };
            if mark > Decimal::ZERO {
                let distance = if size > Decimal::ZERO {
                    (mark - liq) / mark
                } else {
                    (liq - mark) / mark
                };
                if distance <= Decimal::new(5, 2) {
                    alerts.push(alert(
                        user,
                        "liq_proximity",
                        Some(&coin),
                        AlertSeverity::Critical,
                        format!("{coin} is {distance} from liquidation"),
                        json!({ "distance": distance.to_string() }),
                    ));
                } else if distance <= Decimal::new(1, 1) {
                    alerts.push(alert(
                        user,
                        "liq_proximity",
                        Some(&coin),
                        AlertSeverity::Warning,
                        format!("{coin} is {distance} from liquidation"),
                        json!({ "distance": distance.to_string() }),
                    ));
                }
            }
        }

        if enabled.contains(&RiskRule::NoTpsl) {
            let seen = state
                .first_seen
                .get(&coin)
                .copied()
                .unwrap_or_else(Instant::now);
            let has_trigger = snapshot.open_orders.iter().any(|order| {
                order.coin.eq_ignore_ascii_case(&coin)
                    && format!("{:?}", order.order_type)
                        .to_ascii_lowercase()
                        .contains("trigger")
            });
            if !has_trigger && seen.elapsed() >= Duration::from_secs(15 * 60) {
                alerts.push(alert(
                    user,
                    "no_tpsl",
                    Some(&coin),
                    AlertSeverity::Warning,
                    format!("{coin} has been open 15m without TP/SL"),
                    json!({ "open_for_secs": seen.elapsed().as_secs() }),
                ));
            }
        }

        if enabled.contains(&RiskRule::FundingBleed) {
            // Bleed = funding actually paid while watching: the delta of
            // cum_funding.since_open since the position was first observed.
            let paid = position.position.cum_funding.since_open;
            let baseline = *state.funding_baseline.entry(coin.clone()).or_insert(paid);
            state
                .funding_seen
                .entry(coin.clone())
                .or_insert_with(|| seeded.unwrap_or_else(Instant::now));
            if let Some(seen) = state.funding_seen.get(&coin)
                && seen.elapsed() >= Duration::from_secs(60 * 60)
                && paid - baseline > Decimal::ZERO
                && state.funding_alerted.insert(coin.clone())
            {
                alerts.push(alert(
                    user,
                    "funding_bleed",
                    Some(&coin),
                    AlertSeverity::Warning,
                    format!(
                        "{coin} paid {} funding since watch started",
                        paid - baseline
                    ),
                    json!({
                        "open_for_secs": seen.elapsed().as_secs(),
                        "funding_paid": (paid - baseline).to_string(),
                    }),
                ));
            }
        }
    }

    if enabled.contains(&RiskRule::StaleOrder) {
        let stale_after = Duration::from_secs(12 * 60 * 60);
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        for order in snapshot.open_orders {
            let age_ms = now_ms.saturating_sub(order.timestamp);
            if Duration::from_millis(age_ms) < stale_after {
                continue;
            }
            if let Some(mid) = snapshot.mids.get(&order.coin)
                && *mid > Decimal::ZERO
            {
                let distance = (order.limit_px - *mid).abs() / *mid;
                if distance >= Decimal::new(3, 2) {
                    alerts.push(alert(
                        user,
                        "stale_order",
                        Some(&order.coin),
                        AlertSeverity::Warning,
                        format!("{} limit is stale and {distance} from mid", order.coin),
                        json!({
                            "oid": order.oid,
                            "distance": distance.to_string(),
                        }),
                    ));
                }
            }
        }
    }

    alerts
}

fn enabled_rules(args: &WatchRiskArgs) -> Vec<RiskRule> {
    let all = [
        RiskRule::LiqProximity,
        RiskRule::MarginUsage,
        RiskRule::NoTpsl,
        RiskRule::StaleOrder,
        RiskRule::FundingBleed,
        RiskRule::PositionLifecycle,
    ];
    all.into_iter()
        .filter(|rule| {
            if !args.only.is_empty() {
                args.only.contains(rule)
            } else {
                !args.disable.contains(rule)
            }
        })
        .collect()
}

fn alert(
    user: Address,
    rule: &str,
    coin: Option<&str>,
    severity: AlertSeverity,
    message: String,
    payload: serde_json::Value,
) -> RiskAlert {
    RiskAlert {
        kind: if rule == "heartbeat" {
            "heartbeat"
        } else {
            "alert"
        },
        time: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        address: user.to_string(),
        rule: rule.to_string(),
        coin: coin.map(str::to_string),
        severity,
        message,
        payload,
    }
}

fn parse_secs(raw: &str) -> Result<Duration, String> {
    let trimmed = raw.trim();
    if let Some(ms) = trimmed.strip_suffix("ms") {
        let value: u64 = ms.parse().map_err(|_| format!("invalid duration {raw}"))?;
        return Ok(Duration::from_millis(value));
    }
    let seconds = trimmed
        .strip_suffix('s')
        .unwrap_or(trimmed)
        .parse::<u64>()
        .map_err(|_| format!("invalid duration {raw}"))?;
    Ok(Duration::from_secs(seconds))
}

struct AlertTable(RiskAlert);

impl crate::output::TableData for AlertTable {
    fn headers(&self) -> Vec<&str> {
        vec!["Time", "Rule", "Coin", "Severity", "Message"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.0.time.to_string(),
            self.0.rule.clone(),
            self.0.coin.clone().unwrap_or_default(),
            format!("{:?}", self.0.severity).to_lowercase(),
            self.0.message.clone(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.0).unwrap_or(json!({}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_watch_user_rejects_invalid_address() {
        assert!(parse_watch_user("not-an-address").is_err());
    }

    #[test]
    fn disable_and_only_filter_rules() {
        let mut args = WatchRiskArgs {
            user: None,
            min_severity: AlertSeverity::Info,
            disable: vec![RiskRule::LiqProximity],
            only: vec![],
            interval: Duration::from_secs(15),
            max_events: Some(1),
            idle_timeout_ms: None,
        };
        assert!(!enabled_rules(&args).contains(&RiskRule::LiqProximity));
        args.only = vec![RiskRule::MarginUsage];
        assert_eq!(enabled_rules(&args), vec![RiskRule::MarginUsage]);
    }

    #[test]
    fn heartbeat_and_rule_alerts_set_kind() {
        let user = parse_watch_user("0x0000000000000000000000000000000000000001").unwrap();
        let heartbeat = alert(
            user,
            "heartbeat",
            None,
            AlertSeverity::Info,
            "no open positions".to_string(),
            json!({ "positions": 0 }),
        );
        let liq = alert(
            user,
            "liq_proximity",
            Some("ETH"),
            AlertSeverity::Critical,
            "near liq".to_string(),
            json!({}),
        );
        assert_eq!(heartbeat.kind, "heartbeat");
        assert_eq!(liq.kind, "alert");
        assert_eq!(
            serde_json::to_value(&heartbeat).unwrap()["kind"],
            "heartbeat"
        );
        assert_eq!(serde_json::to_value(&liq).unwrap()["kind"], "alert");
    }
}
