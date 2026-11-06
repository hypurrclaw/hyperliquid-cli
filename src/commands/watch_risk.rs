//! Read-only position risk watcher: the alert rules and one polling tick.
//!
//! Looping, bounds, signals and line output belong to [`crate::stream`].

use crate::command_spec::{CliCommand, CommandSpec, GroupSpec, InputKind, arg};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::{Args, ValueEnum};
use hypersdk::hypercore::HttpClient;
use hypersdk::hypercore::types::{AssetPosition, BasicOrder, Fill, OrderType, Side};
use hypersdk::{Address, Decimal};
use serde::Serialize;
use serde_json::json;

use crate::errors::CliError;
use crate::errors::from_sdk;
use crate::invocation::Invocation;
use crate::output::{self, OutputFormat, Session};
use crate::stream::{
    self, StreamBoundArgs, StreamBounds, StreamEnd, Ticker, emit_json_line_to_stdout,
    write_stdout_line,
};

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

/// The limits a rule fires at, in the units that rule uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Threshold {
    /// The rule reports every transition and has no limit.
    None,
    /// Margin used, in percent of equity, at or above which the rule fires.
    MarginUsedPercent(Decimal),
    /// Distance to liquidation, as a fraction of the mark price, at or below which the rule fires.
    LiqDistance { critical: Decimal, warning: Decimal },
    /// Time a position may stay open without a TP/SL order.
    OpenWithoutTpsl(Duration),
    /// Time a position must be watched before paid funding is reported.
    FundingWindow(Duration),
    /// A resting order older than `age` and at least `mid_distance` (fraction) away from the mid.
    StaleOrder {
        age: Duration,
        mid_distance: Decimal,
    },
}

impl RiskRule {
    pub const ALL: [RiskRule; 6] = [
        RiskRule::LiqProximity,
        RiskRule::MarginUsage,
        RiskRule::NoTpsl,
        RiskRule::StaleOrder,
        RiskRule::FundingBleed,
        RiskRule::PositionLifecycle,
    ];

    /// The rule name in alert JSON and on the command line.
    pub fn as_str(self) -> &'static str {
        match self {
            RiskRule::LiqProximity => "liq_proximity",
            RiskRule::MarginUsage => "margin_usage",
            RiskRule::NoTpsl => "no_tpsl",
            RiskRule::StaleOrder => "stale_order",
            RiskRule::FundingBleed => "funding_bleed",
            RiskRule::PositionLifecycle => "position_lifecycle",
        }
    }

    pub fn default_threshold(self) -> Threshold {
        match self {
            RiskRule::LiqProximity => Threshold::LiqDistance {
                critical: Decimal::new(5, 2),
                warning: Decimal::new(1, 1),
            },
            RiskRule::MarginUsage => Threshold::MarginUsedPercent(Decimal::from(80)),
            RiskRule::NoTpsl => Threshold::OpenWithoutTpsl(Duration::from_secs(15 * 60)),
            RiskRule::StaleOrder => Threshold::StaleOrder {
                age: Duration::from_secs(12 * 60 * 60),
                mid_distance: Decimal::new(3, 2),
            },
            RiskRule::FundingBleed => Threshold::FundingWindow(Duration::from_secs(60 * 60)),
            RiskRule::PositionLifecycle => Threshold::None,
        }
    }
}

#[derive(Args, Debug, Clone)]
#[command(
    mut_arg("max_events", |arg| arg.help(
        "Stop after this many alerts. Required in agent mode unless --idle-timeout-ms is set"
    )),
    mut_arg("idle_timeout_ms", |arg| arg
        .value_name("IDLE_TIMEOUT_MS")
        .help("Stop after this many idle milliseconds"))
)]
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
    #[command(flatten)]
    pub stream: StreamBoundArgs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Serialize)]
#[clap(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

impl AlertSeverity {
    fn as_str(self) -> &'static str {
        match self {
            AlertSeverity::Info => "info",
            AlertSeverity::Warning => "warning",
            AlertSeverity::Critical => "critical",
        }
    }
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

impl RiskAlert {
    fn for_rule(
        user: Address,
        rule: RiskRule,
        coin: Option<&str>,
        severity: AlertSeverity,
        message: String,
        payload: serde_json::Value,
    ) -> Self {
        Self::build(
            "alert",
            user,
            rule.as_str(),
            coin,
            severity,
            message,
            payload,
        )
    }

    /// Announces that an account has no open positions.
    fn heartbeat(user: Address) -> Self {
        Self::build(
            "heartbeat",
            user,
            "heartbeat",
            None,
            AlertSeverity::Info,
            "no open positions".to_string(),
            json!({ "positions": 0 }),
        )
    }

    fn build(
        kind: &'static str,
        user: Address,
        rule: &str,
        coin: Option<&str>,
        severity: AlertSeverity,
        message: String,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            kind,
            time: now_ms(),
            address: user.to_string(),
            rule: rule.to_string(),
            coin: coin.map(str::to_string),
            severity,
            message,
            payload,
        }
    }
}

pub fn parse_watch_user(raw: &str) -> Result<Address, CliError> {
    raw.parse::<Address>()
        .map_err(|_| CliError::Unsupported(format!("Invalid address: {raw}")))
}

pub async fn watch_risk(
    inv: &Invocation,
    user: Address,
    args: &WatchRiskArgs,
) -> Result<(), anyhow::Error> {
    let client = inv.remote()?.api().sdk();
    let session = inv.session();
    let bounds = StreamBounds::watch_risk(&args.stream);
    if stream::piped_or_agent() && !bounds.is_bounded() {
        return Err(CliError::Configuration(
            "watch risk requires --max-events or --idle-timeout-ms in agent mode.\n  \
             hyperliquid --format json watch risk --user 0x0000000000000000000000000000000000000001 --max-events 20"
                .to_string(),
        )
        .into());
    }

    stream::run_stream(async {
        let mut watcher = RiskWatcher::new(client, user, args).await;
        stream::poll_stream(&mut watcher, args.interval, bounds, |alert| {
            emit_alert(alert, session)
        })
        .await?;
        Ok(StreamEnd::Finished)
    })
    .await
}

fn emit_alert(alert: RiskAlert, session: &Session) -> Result<(), anyhow::Error> {
    match session.format() {
        OutputFormat::Json => emit_json_line_to_stdout(&alert, session.json_options()),
        _ => write_stdout_line(&session.render(&AlertTable(alert))),
    }
}

struct RiskWatcher<'a> {
    client: &'a HttpClient,
    user: Address,
    enabled: Vec<RiskRule>,
    min_severity: AlertSeverity,
    state: RiskEvalState,
    flat_announced: bool,
}

impl<'a> RiskWatcher<'a> {
    async fn new(client: &'a HttpClient, user: Address, args: &WatchRiskArgs) -> Self {
        // Seed position ages from fill history so positions opened before this
        // watcher started do not wait a full threshold window for age-based rules.
        let fills = client.user_fills(user).await.unwrap_or_default();
        Self {
            client,
            user,
            enabled: enabled_rules(args),
            min_severity: args.min_severity,
            state: RiskEvalState::new(position_open_times(&fills)),
            flat_announced: false,
        }
    }
}

impl Ticker for RiskWatcher<'_> {
    type Item = RiskAlert;

    async fn tick(&mut self) -> Result<Vec<RiskAlert>, anyhow::Error> {
        let state = self
            .client
            .clearinghouse_state(self.user, None)
            .await
            .map_err(from_sdk)?;
        let open_orders = self
            .client
            .open_orders(self.user, None)
            .await
            .unwrap_or_default();
        let mids = self.client.all_mids(None).await.unwrap_or_default();

        let snapshot = RiskSnapshot {
            positions: &state.asset_positions,
            open_orders: &open_orders,
            mids: &mids,
            equity: state.margin_summary.account_value,
            margin_used: state.margin_summary.total_margin_used,
        };
        let mut alerts = evaluate_state(self.user, &self.enabled, snapshot, &mut self.state);

        let no_positions = state
            .asset_positions
            .iter()
            .all(|position| position.position.szi.is_zero());
        if no_positions {
            // Heartbeat once per flat transition; a flat account keeps
            // monitoring so positions opened later are still observed.
            if !self.flat_announced {
                alerts.push(RiskAlert::heartbeat(self.user));
                self.flat_announced = true;
            }
        } else {
            self.flat_announced = false;
        }

        alerts.retain(|alert| alert.severity >= self.min_severity);
        Ok(alerts)
    }
}

struct RiskSnapshot<'a> {
    positions: &'a [AssetPosition],
    open_orders: &'a [BasicOrder],
    mids: &'a HashMap<String, Decimal>,
    equity: Decimal,
    margin_used: Decimal,
}

struct RiskEvalState {
    last_sizes: HashMap<String, Decimal>,
    first_seen: HashMap<String, Instant>,
    funding_seen: HashMap<String, Instant>,
    funding_baseline: HashMap<String, Decimal>,
    funding_alerted: HashSet<String>,
    /// Coin → position open time (ms since epoch) seeded from fill history.
    open_times: HashMap<String, u64>,
}

impl RiskEvalState {
    fn new(open_times: HashMap<String, u64>) -> Self {
        Self {
            last_sizes: HashMap::new(),
            first_seen: HashMap::new(),
            funding_seen: HashMap::new(),
            funding_baseline: HashMap::new(),
            funding_alerted: HashSet::new(),
            open_times,
        }
    }

    /// Drop the age and funding clocks of a position that is no longer open.
    fn forget_clocks(&mut self, coin: &str) {
        self.first_seen.remove(coin);
        self.funding_seen.remove(coin);
        self.funding_baseline.remove(coin);
        self.funding_alerted.remove(coin);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn position_open_times(fills: &[Fill]) -> HashMap<String, u64> {
    let mut open_times: HashMap<String, u64> = HashMap::new();
    let mut oldest: HashMap<String, u64> = HashMap::new();
    for fill in fills {
        oldest
            .entry(fill.coin.clone())
            .and_modify(|time| *time = (*time).min(fill.time))
            .or_insert(fill.time);
        let signed_sz = if fill.side == Side::Bid {
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
    let now_ms = now_ms();
    if ms >= now_ms {
        return None;
    }
    Instant::now().checked_sub(Duration::from_millis(now_ms - ms))
}

fn evaluate_state(
    user: Address,
    enabled: &[RiskRule],
    snapshot: RiskSnapshot<'_>,
    state: &mut RiskEvalState,
) -> Vec<RiskAlert> {
    let mut alerts = Vec::new();
    if enabled.contains(&RiskRule::MarginUsage)
        && let Threshold::MarginUsedPercent(limit) = RiskRule::MarginUsage.default_threshold()
        && snapshot.equity > Decimal::ZERO
        && snapshot.margin_used * Decimal::from(100) / snapshot.equity >= limit
    {
        let pct = snapshot.margin_used * Decimal::from(100) / snapshot.equity;
        alerts.push(RiskAlert::for_rule(
            user,
            RiskRule::MarginUsage,
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
            alerts.push(RiskAlert::for_rule(
                user,
                RiskRule::PositionLifecycle,
                Some(&coin),
                AlertSeverity::Info,
                format!("{coin} closed"),
                json!({ "size": "0" }),
            ));
            state.last_sizes.insert(coin.clone(), Decimal::ZERO);
            state.forget_clocks(&coin);
        }
    }

    for position in snapshot.positions {
        let coin = position.position.coin.clone();
        let size = position.position.szi;
        if enabled.contains(&RiskRule::PositionLifecycle) {
            let lifecycle = |message: String, payload: serde_json::Value| {
                RiskAlert::for_rule(
                    user,
                    RiskRule::PositionLifecycle,
                    Some(&coin),
                    AlertSeverity::Info,
                    message,
                    payload,
                )
            };
            match state.last_sizes.get(&coin) {
                None if !size.is_zero() => alerts.push(lifecycle(
                    format!("{coin} opened"),
                    json!({ "size": size.to_string() }),
                )),
                Some(prev) if prev.is_zero() && !size.is_zero() => alerts.push(lifecycle(
                    format!("{coin} opened"),
                    json!({ "size": size.to_string() }),
                )),
                Some(prev) if !prev.is_zero() && size.is_zero() => alerts.push(lifecycle(
                    format!("{coin} closed"),
                    json!({ "size": size.to_string() }),
                )),
                Some(prev) if *prev != size => alerts.push(lifecycle(
                    format!("{coin} resized"),
                    json!({ "from": prev.to_string(), "to": size.to_string() }),
                )),
                _ => {}
            }
        }
        state.last_sizes.insert(coin.clone(), size);
        if size.is_zero() {
            state.forget_clocks(&coin);
            continue;
        }
        // Seed from fill history when available so pre-existing positions do
        // not restart their age clocks at watcher startup.
        let seeded = open_instant(&state.open_times, &coin);
        state
            .first_seen
            .entry(coin.clone())
            .or_insert_with(|| seeded.unwrap_or_else(Instant::now));

        if enabled.contains(&RiskRule::LiqProximity)
            && let Threshold::LiqDistance { critical, warning } =
                RiskRule::LiqProximity.default_threshold()
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
                let severity = if distance <= critical {
                    Some(AlertSeverity::Critical)
                } else if distance <= warning {
                    Some(AlertSeverity::Warning)
                } else {
                    None
                };
                if let Some(severity) = severity {
                    alerts.push(RiskAlert::for_rule(
                        user,
                        RiskRule::LiqProximity,
                        Some(&coin),
                        severity,
                        format!("{coin} is {distance} from liquidation"),
                        json!({ "distance": distance.to_string() }),
                    ));
                }
            }
        }

        if enabled.contains(&RiskRule::NoTpsl)
            && let Threshold::OpenWithoutTpsl(open_for) = RiskRule::NoTpsl.default_threshold()
        {
            let seen = state
                .first_seen
                .get(&coin)
                .copied()
                .unwrap_or_else(Instant::now);
            let has_trigger = snapshot.open_orders.iter().any(|order| {
                order.coin.eq_ignore_ascii_case(&coin)
                    && matches!(order.order_type, OrderType::Trigger)
            });
            if !has_trigger && seen.elapsed() >= open_for {
                alerts.push(RiskAlert::for_rule(
                    user,
                    RiskRule::NoTpsl,
                    Some(&coin),
                    AlertSeverity::Warning,
                    format!("{coin} has been open 15m without TP/SL"),
                    json!({ "open_for_secs": seen.elapsed().as_secs() }),
                ));
            }
        }

        if enabled.contains(&RiskRule::FundingBleed)
            && let Threshold::FundingWindow(window) = RiskRule::FundingBleed.default_threshold()
        {
            // Bleed = funding actually paid while watching: the delta of
            // cum_funding.since_open since the position was first observed.
            let paid = position.position.cum_funding.since_open;
            let baseline = *state.funding_baseline.entry(coin.clone()).or_insert(paid);
            state
                .funding_seen
                .entry(coin.clone())
                .or_insert_with(|| seeded.unwrap_or_else(Instant::now));
            if let Some(seen) = state.funding_seen.get(&coin)
                && seen.elapsed() >= window
                && paid - baseline > Decimal::ZERO
                && state.funding_alerted.insert(coin.clone())
            {
                alerts.push(RiskAlert::for_rule(
                    user,
                    RiskRule::FundingBleed,
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

    if enabled.contains(&RiskRule::StaleOrder)
        && let Threshold::StaleOrder { age, mid_distance } =
            RiskRule::StaleOrder.default_threshold()
    {
        let now_ms = now_ms();
        for order in snapshot.open_orders {
            let age_ms = now_ms.saturating_sub(order.timestamp);
            if Duration::from_millis(age_ms) < age {
                continue;
            }
            if let Some(mid) = snapshot.mids.get(&order.coin)
                && *mid > Decimal::ZERO
            {
                let distance = (order.limit_px - *mid).abs() / *mid;
                if distance >= mid_distance {
                    alerts.push(RiskAlert::for_rule(
                        user,
                        RiskRule::StaleOrder,
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
    RiskRule::ALL
        .into_iter()
        .filter(|rule| {
            if !args.only.is_empty() {
                args.only.contains(rule)
            } else {
                !args.disable.contains(rule)
            }
        })
        .collect()
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

impl output::TableData for AlertTable {
    fn headers(&self) -> Vec<&str> {
        vec!["Time", "Rule", "Coin", "Severity", "Message"]
    }

    fn rows(&self) -> Vec<Vec<String>> {
        vec![vec![
            self.0.time.to_string(),
            self.0.rule.clone(),
            self.0.coin.clone().unwrap_or_default(),
            self.0.severity.as_str().to_string(),
            self.0.message.clone(),
        ]]
    }

    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(&self.0).unwrap_or(json!({}))
    }
}

pub const GROUP: GroupSpec = GroupSpec::new(&["watch"], "Watch accounts for risk alerts");

pub struct WatchRisk;

impl CliCommand for WatchRisk {
    type Args = WatchRiskArgs;

    const SPEC: CommandSpec = CommandSpec::read(
        &["watch", "risk"],
        "watch",
        "Watch an address for liquidation and risk alerts",
        "Read-only liquidation and risk watcher. Emits JSON-line alerts with kind alert or heartbeat and never places orders. Retry: safe. Agent mode requires --max-events or --idle-timeout-ms.",
    )
    .long_about("Read-only liquidation and risk watcher. Emits JSON-line alerts and never places orders.\n\
\n\
Retry: safe. Agent/non-TTY mode requires --max-events or --idle-timeout-ms.\n\
\n\
Examples:\n  \
hyperliquid --format json watch risk --user 0x0000000000000000000000000000000000000001 --max-events 20\n  \
hyperliquid --format json watch risk --user 0x0000000000000000000000000000000000000001 --idle-timeout-ms 8000")
    .args(&[
        arg("user").kind(InputKind::Identifier).resource_id("user"),
        arg("min_severity").describe("Minimum alert severity to emit."),
        arg("disable").describe("Disable a risk rule.").schema_single(),
        arg("only").describe("Only evaluate these risk rules.").schema_single(),
        arg("interval").kind(InputKind::Duration).describe("Polling interval between risk checks, for example 15s or 500ms. Default 15s."),
        arg("max_events"),
        arg("idle_timeout_ms"),
    ]);

    async fn run(inv: &Invocation, args: &WatchRiskArgs) -> Result<(), anyhow::Error> {
        let user = if let Some(raw) = args.user.as_deref() {
            parse_watch_user(raw)?
        } else {
            inv.resolve_signer()?.query_address()
        };
        watch_risk(inv, user, args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> WatchRiskArgs {
        WatchRiskArgs {
            user: None,
            min_severity: AlertSeverity::Info,
            disable: vec![],
            only: vec![],
            interval: Duration::from_secs(15),
            stream: StreamBoundArgs {
                max_events: Some(1),
                idle_timeout_ms: None,
            },
        }
    }

    fn user() -> Address {
        parse_watch_user("0x0000000000000000000000000000000000000001").unwrap()
    }

    #[test]
    fn parse_watch_user_rejects_invalid_address() {
        assert!(parse_watch_user("not-an-address").is_err());
    }

    #[test]
    fn disable_and_only_filter_rules() {
        let mut args = args();
        args.disable = vec![RiskRule::LiqProximity];
        assert!(!enabled_rules(&args).contains(&RiskRule::LiqProximity));
        args.only = vec![RiskRule::MarginUsage];
        assert_eq!(enabled_rules(&args), vec![RiskRule::MarginUsage]);
    }

    #[test]
    fn rule_names_match_command_line_values() {
        for rule in RiskRule::ALL {
            let value = rule.to_possible_value().unwrap();
            assert_eq!(value.get_name(), rule.as_str());
        }
    }

    #[test]
    fn every_rule_has_a_threshold_of_its_own_shape() {
        for rule in RiskRule::ALL {
            let matches_shape = matches!(
                (rule, rule.default_threshold()),
                (RiskRule::LiqProximity, Threshold::LiqDistance { .. })
                    | (RiskRule::MarginUsage, Threshold::MarginUsedPercent(_))
                    | (RiskRule::NoTpsl, Threshold::OpenWithoutTpsl(_))
                    | (RiskRule::StaleOrder, Threshold::StaleOrder { .. })
                    | (RiskRule::FundingBleed, Threshold::FundingWindow(_))
                    | (RiskRule::PositionLifecycle, Threshold::None)
            );
            assert!(matches_shape, "{rule:?}");
        }
    }

    #[test]
    fn heartbeat_and_rule_alerts_set_kind() {
        let heartbeat = RiskAlert::heartbeat(user());
        let liq = RiskAlert::for_rule(
            user(),
            RiskRule::LiqProximity,
            Some("ETH"),
            AlertSeverity::Critical,
            "near liq".to_string(),
            json!({}),
        );
        assert_eq!(heartbeat.kind, "heartbeat");
        assert_eq!(heartbeat.rule, "heartbeat");
        assert_eq!(liq.kind, "alert");
        assert_eq!(liq.rule, "liq_proximity");
        assert_eq!(
            serde_json::to_value(&heartbeat).unwrap()["kind"],
            "heartbeat"
        );
        assert_eq!(serde_json::to_value(&liq).unwrap()["kind"], "alert");
    }

    #[test]
    fn severity_names_match_serialized_names() {
        for severity in [
            AlertSeverity::Info,
            AlertSeverity::Warning,
            AlertSeverity::Critical,
        ] {
            assert_eq!(
                serde_json::to_value(severity).unwrap(),
                json!(severity.as_str())
            );
        }
    }

    #[test]
    fn margin_usage_fires_at_the_default_threshold() {
        let mids = HashMap::new();
        let evaluate = |margin_used: i64| {
            let mut state = RiskEvalState::new(HashMap::new());
            evaluate_state(
                user(),
                &[RiskRule::MarginUsage],
                RiskSnapshot {
                    positions: &[],
                    open_orders: &[],
                    mids: &mids,
                    equity: Decimal::from(100),
                    margin_used: Decimal::from(margin_used),
                },
                &mut state,
            )
        };

        assert!(evaluate(79).is_empty());
        let alerts = evaluate(80);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].rule, "margin_usage");
        assert_eq!(alerts[0].severity, AlertSeverity::Warning);
    }

    #[test]
    fn disabled_rules_do_not_fire() {
        let mids = HashMap::new();
        let mut state = RiskEvalState::new(HashMap::new());
        let alerts = evaluate_state(
            user(),
            &[RiskRule::NoTpsl],
            RiskSnapshot {
                positions: &[],
                open_orders: &[],
                mids: &mids,
                equity: Decimal::from(100),
                margin_used: Decimal::from(100),
            },
            &mut state,
        );
        assert!(alerts.is_empty());
    }
}
