//! Streaming runtime shared by snapshot watch, `subscribe` and `watch risk`.
//!
//! Every stream stops on SIGINT, SIGTERM, and SIGHUP: the loop ends, the
//! terminal is restored, the current output line is complete, and the process
//! exits `128 + signal`. A closed stdout (for example `| head`) ends the stream
//! quietly with exit 0.

use std::error::Error as StdError;
use std::fmt;
use std::io::{self, Write};
use std::pin::Pin;
use std::time::Duration;

use clap::Args;
use futures::{Stream, StreamExt};
use hypersdk::Address;
use hypersdk::hypercore::{Chain, Incoming, Subscription, UserEvent, ws::Event as WsEvent};
use serde::Serialize;
use tokio::time::Sleep;

use crate::errors::CliError;
use crate::output::terminal;
use crate::output::{JsonRenderOptions, Session};
use crate::{output, ws};

/// Exit status for SIGHUP (`128 + 1`).
const EXIT_SIGHUP: i32 = 129;
/// Exit status for SIGINT and the Ctrl+C key (`128 + 2`).
pub(crate) const EXIT_SIGINT: i32 = 130;
/// Exit status for SIGTERM (`128 + 15`).
const EXIT_SIGTERM: i32 = 143;

/// A polling stream with no explicit idle bound ends after this long without output.
const POLL_DEFAULT_IDLE: Duration = Duration::from_secs(60 * 60);

pub(crate) const JSON_LINES_STDOUT_NOTICE: &str =
    "Streaming JSON lines because stdout isn't a terminal.";
pub(crate) const JSON_LINES_STDIN_NOTICE: &str =
    "Streaming JSON lines because stdin isn't a terminal.";
pub(crate) const CONNECTION_LOST_NOTICE: &str = "! Lost connection. Retrying automatically…";
const STREAM_GAP_MESSAGE: &str = "Connection was lost and restored. Events may have been missed.";

/// Output that no person is watching: stdout is not a terminal, or agent mode is on.
pub(crate) fn piped_or_agent() -> bool {
    !terminal::stdout_is_terminal() || terminal::agent_env()
}

// ── Closed pipe and line output ─────────────────────────────────────────

/// Stdout was closed by the reader (`| head`); the stream should end quietly.
#[derive(Debug)]
pub struct StdoutClosed;

impl fmt::Display for StdoutClosed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("stdout closed")
    }
}

impl StdError for StdoutClosed {}

/// Whether `err` is a closed-stdout condition rather than a real failure.
pub fn is_stdout_closed(err: &anyhow::Error) -> bool {
    err.downcast_ref::<StdoutClosed>().is_some()
}

fn map_write_error(err: io::Error) -> anyhow::Error {
    if err.kind() == io::ErrorKind::BrokenPipe {
        StdoutClosed.into()
    } else {
        err.into()
    }
}

/// Write `line` plus a newline to `output` and flush. A broken pipe becomes [`StdoutClosed`].
fn write_line<W: Write + ?Sized>(output: &mut W, line: &str) -> Result<(), anyhow::Error> {
    output
        .write_all(line.as_bytes())
        .and_then(|()| output.write_all(b"\n"))
        .and_then(|()| output.flush())
        .map_err(map_write_error)
}

/// Write one complete line to stdout. A broken pipe becomes [`StdoutClosed`].
pub(crate) fn write_stdout_line(line: &str) -> Result<(), anyhow::Error> {
    write_line(&mut io::stdout().lock(), line)
}

/// Write one compact JSON line, applying `--select`, `--results-only` and
/// `--max-results` with stream-envelope rules.
pub(crate) fn emit_json_line<T, W>(
    output: &mut W,
    item: &T,
    projection: &JsonRenderOptions,
) -> Result<(), anyhow::Error>
where
    T: Serialize + ?Sized,
    W: Write + ?Sized,
{
    let line = if projection.is_active() {
        let value = projection.apply_stream(serde_json::to_value(item)?);
        serde_json::to_string(&value)?
    } else {
        serde_json::to_string(item)?
    };
    write_line(output, &line)
}

pub(crate) fn emit_json_line_to_stdout<T: Serialize + ?Sized>(
    item: &T,
    projection: &JsonRenderOptions,
) -> Result<(), anyhow::Error> {
    emit_json_line(&mut io::stdout().lock(), item, projection)
}

// ── Signals and the run loop ────────────────────────────────────────────

/// Stop signals a stream reacts to.
pub struct ShutdownSignals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(unix)]
    hangup: tokio::signal::unix::Signal,
}

impl ShutdownSignals {
    /// Register SIGINT, SIGTERM, and SIGHUP handlers.
    #[cfg(unix)]
    pub fn install() -> Result<Self, anyhow::Error> {
        use tokio::signal::unix::{SignalKind, signal};

        let register = |kind: SignalKind, name: &str| {
            signal(kind).map_err(|err| {
                CliError::Internal(anyhow::anyhow!("failed to install {name} handler: {err}"))
            })
        };
        Ok(Self {
            interrupt: register(SignalKind::interrupt(), "SIGINT")?,
            terminate: register(SignalKind::terminate(), "SIGTERM")?,
            hangup: register(SignalKind::hangup(), "SIGHUP")?,
        })
    }

    #[cfg(not(unix))]
    pub fn install() -> Result<Self, anyhow::Error> {
        Ok(Self {})
    }

    /// Wait for the first stop signal and return the matching exit status.
    #[cfg(unix)]
    pub async fn recv(&mut self) -> i32 {
        tokio::select! {
            Some(()) = self.interrupt.recv() => EXIT_SIGINT,
            Some(()) = self.terminate.recv() => EXIT_SIGTERM,
            Some(()) = self.hangup.recv() => EXIT_SIGHUP,
            else => std::future::pending().await,
        }
    }

    #[cfg(not(unix))]
    pub async fn recv(&mut self) -> i32 {
        match tokio::signal::ctrl_c().await {
            Ok(()) => EXIT_SIGINT,
            Err(_) => std::future::pending().await,
        }
    }
}

/// How a stream's work ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamEnd {
    Finished,
    Signal(i32),
}

/// Flush the standard streams and exit with a signal status. Call after cleanup.
fn exit_for_signal(code: i32) -> ! {
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    std::process::exit(code)
}

/// Run `work` until it ends, a stop signal arrives, or stdout closes.
///
/// Anything `work` must restore (the terminal) lives inside it, so it is
/// dropped before the process exits for a signal. Signal handlers are
/// installed before `work` first runs.
pub(crate) async fn run_stream<F>(work: F) -> Result<(), anyhow::Error>
where
    F: Future<Output = Result<StreamEnd, anyhow::Error>>,
{
    let mut signals = ShutdownSignals::install()?;
    let end = tokio::select! {
        end = work => end,
        code = signals.recv() => Ok(StreamEnd::Signal(code)),
    };

    match end {
        Ok(StreamEnd::Finished) => Ok(()),
        Ok(StreamEnd::Signal(code)) => exit_for_signal(code),
        Err(err) if is_stdout_closed(&err) => Ok(()),
        Err(err) => Err(err),
    }
}

// ── Bounds ──────────────────────────────────────────────────────────────

/// A count of at least 1.
pub fn parse_positive_usize(raw: &str) -> Result<usize, String> {
    let value = raw
        .parse::<usize>()
        .map_err(|err| format!("invalid positive integer: {err}"))?;
    if value == 0 {
        return Err("value must be at least 1".to_string());
    }
    Ok(value)
}

/// `--watch` / `--max-ticks`, shared by every snapshot command.
#[derive(Args, Debug, Clone, Default)]
pub struct SnapshotWatchArgs {
    /// Watch live updates in-place
    #[arg(short = 'w', long)]
    pub watch: bool,
    /// Stop after rendering this many watch snapshots (useful for automation)
    #[arg(long, value_name = "TICKS", requires = "watch", value_parser = parse_positive_usize)]
    pub max_ticks: Option<usize>,
}

/// `--max-events` / `--idle-timeout-ms`, shared by `subscribe` and `watch risk`.
#[derive(Args, Debug, Clone, Default)]
pub struct StreamBoundArgs {
    /// Stop after emitting this many matching events (useful for automation)
    #[arg(long)]
    pub max_events: Option<usize>,
    /// Stop if no matching events are emitted within this many milliseconds
    #[arg(long, value_name = "MILLISECONDS", value_parser = clap::value_parser!(u64).range(1..))]
    pub idle_timeout_ms: Option<u64>,
}

impl StreamBoundArgs {
    fn idle(&self) -> Option<Duration> {
        self.idle_timeout_ms.map(Duration::from_millis)
    }
}

/// A bound that may also come from an environment variable.
struct EnvBound {
    name: &'static str,
    /// `0` in the environment means "no bound" rather than "stop immediately".
    positive_only: bool,
}

const WATCH_MAX_TICKS: EnvBound = EnvBound {
    name: "HYPERLIQUID_WATCH_MAX_TICKS",
    positive_only: true,
};

const SUBSCRIBE_MAX_EVENTS: EnvBound = EnvBound {
    name: "HYPERLIQUID_SUBSCRIBE_MAX_EVENTS",
    positive_only: false,
};

impl EnvBound {
    /// The bound, ignoring an unparsable value.
    fn read(&self) -> Option<usize> {
        std::env::var(self.name)
            .ok()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            .filter(|value| !self.positive_only || *value > 0)
    }

    /// Reject a set but unparsable value.
    fn validate(&self) -> Result<(), CliError> {
        let Ok(raw) = std::env::var(self.name) else {
            return Ok(());
        };
        raw.trim().parse::<usize>().map(|_| ()).map_err(|err| {
            CliError::Configuration(format!(
                "{} must be a non-negative integer: {err}",
                self.name
            ))
        })
    }
}

/// Resolved stop conditions of one stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamBounds {
    /// Ticks, events or alerts, depending on the stream.
    pub max_items: Option<usize>,
    /// Stop after this long without output.
    pub idle: Option<Duration>,
}

impl StreamBounds {
    /// `--max-ticks`, falling back to `HYPERLIQUID_WATCH_MAX_TICKS`.
    pub fn snapshot_watch(max_ticks: Option<usize>) -> Self {
        Self {
            max_items: max_ticks.or_else(|| WATCH_MAX_TICKS.read()),
            idle: None,
        }
    }

    /// `--max-events`, falling back to `HYPERLIQUID_SUBSCRIBE_MAX_EVENTS`, plus `--idle-timeout-ms`.
    pub fn subscribe(args: &StreamBoundArgs) -> Self {
        Self {
            max_items: args.max_events.or_else(|| SUBSCRIBE_MAX_EVENTS.read()),
            idle: args.idle(),
        }
    }

    /// `watch risk` takes its bounds from flags only.
    pub fn watch_risk(args: &StreamBoundArgs) -> Self {
        Self {
            max_items: args.max_events,
            idle: args.idle(),
        }
    }

    pub fn is_bounded(&self) -> bool {
        self.max_items.is_some() || self.idle.is_some()
    }

    fn reached(&self, emitted: usize) -> bool {
        self.max_items.is_some_and(|max| emitted >= max)
    }
}

/// Machine contexts must not start an endless snapshot watch.
pub fn require_tick_bound(max_ticks: Option<usize>) -> Result<(), CliError> {
    if StreamBounds::snapshot_watch(max_ticks).is_bounded() {
        return Ok(());
    }
    WATCH_MAX_TICKS.validate()?;
    Err(CliError::Unsupported(
        "JSON watch output must be bounded with --max-ticks <N> or HYPERLIQUID_WATCH_MAX_TICKS"
            .to_string(),
    ))
}

/// Machine contexts must not start an endless subscribe stream.
pub fn require_event_bound(args: &StreamBoundArgs) -> Result<(), CliError> {
    SUBSCRIBE_MAX_EVENTS.validate()?;
    if StreamBounds::subscribe(args).is_bounded() {
        return Ok(());
    }
    Err(CliError::Unsupported(
        "JSON subscribe output must be bounded with --max-events <N>, --idle-timeout-ms <N>, or HYPERLIQUID_SUBSCRIBE_MAX_EVENTS"
            .to_string(),
    ))
}

/// Restarts on every emitted item; never fires when no idle bound is set.
struct IdleTimer(Option<(Duration, Pin<Box<Sleep>>)>);

impl IdleTimer {
    fn new(idle: Option<Duration>) -> Self {
        Self(idle.map(|idle| (idle, Box::pin(tokio::time::sleep(idle)))))
    }

    /// Resolves with the configured idle duration once it elapses.
    async fn expired(&mut self) -> Duration {
        match &mut self.0 {
            Some((idle, sleep)) => {
                sleep.as_mut().await;
                *idle
            }
            None => std::future::pending().await,
        }
    }

    fn restart(&mut self) {
        if let Some((idle, sleep)) = &mut self.0 {
            sleep.as_mut().reset(tokio::time::Instant::now() + *idle);
        }
    }
}

// ── Polling streams ─────────────────────────────────────────────────────

/// One poll of a source that yields zero or more items per tick.
pub(crate) trait Ticker {
    type Item;

    async fn tick(&mut self) -> Result<Vec<Self::Item>, anyhow::Error>;
}

/// Poll `ticker` every `interval`, passing each item to `emit`, until the
/// item bound is reached or nothing was emitted for the idle bound (default one hour).
pub(crate) async fn poll_stream<T: Ticker>(
    ticker: &mut T,
    interval: Duration,
    bounds: StreamBounds,
    mut emit: impl FnMut(T::Item) -> Result<(), anyhow::Error>,
) -> Result<(), anyhow::Error> {
    let idle = bounds.idle.unwrap_or(POLL_DEFAULT_IDLE);
    let mut emitted = 0usize;
    let mut last_output = std::time::Instant::now();

    loop {
        if last_output.elapsed() > idle || bounds.reached(emitted) {
            return Ok(());
        }

        for item in ticker.tick().await? {
            emit(item)?;
            emitted += 1;
            last_output = std::time::Instant::now();
            if bounds.reached(emitted) {
                return Ok(());
            }
        }

        tokio::time::sleep(interval).await;
    }
}

// ── Subscribe streams ───────────────────────────────────────────────────

/// A WebSocket subscription paired with the messages that belong to it.
pub struct SubscribeTarget {
    subscription: Subscription,
    matches: fn(&Incoming) -> bool,
}

impl SubscribeTarget {
    pub fn trades(coin: String) -> Self {
        Self {
            subscription: Subscription::Trades { coin },
            matches: |message| matches!(message, Incoming::Trades(_)),
        }
    }

    pub fn orderbook(coin: String) -> Self {
        Self {
            subscription: Subscription::L2Book {
                coin,
                n_sig_figs: None,
                mantissa: None,
                fast: false,
            },
            matches: |message| matches!(message, Incoming::L2Book(_)),
        }
    }

    pub fn candles(coin: String, interval: String) -> Self {
        Self {
            subscription: Subscription::Candle { coin, interval },
            matches: |message| matches!(message, Incoming::Candle(_)),
        }
    }

    pub fn all_mids() -> Self {
        Self {
            subscription: Subscription::AllMids { dex: None },
            matches: |message| matches!(message, Incoming::AllMids { .. }),
        }
    }

    pub fn order_updates(user: Address) -> Self {
        Self {
            subscription: Subscription::OrderUpdates { user },
            matches: |message| matches!(message, Incoming::OrderUpdates(_)),
        }
    }

    pub fn fills(user: Address) -> Self {
        Self {
            subscription: Subscription::UserFills { user },
            matches: |message| {
                matches!(
                    message,
                    Incoming::UserFills { .. } | Incoming::UserEvents(UserEvent::Fills { .. })
                )
            },
        }
    }

    pub fn matches(&self, message: &Incoming) -> bool {
        (self.matches)(message)
    }
}

/// Stream a WebSocket subscription to stdout as NDJSON.
///
/// A human output format on a non-terminal stdout only happens when it was
/// asked for explicitly, so it earns a notice that the stream is JSON lines.
pub async fn stream_subscription(
    session: &Session,
    chain: Chain,
    target: SubscribeTarget,
    bounds: StreamBounds,
) -> Result<(), anyhow::Error> {
    let human_format_requested = session.format() != output::OutputFormat::Json;
    let projection = session.json_options();
    if !terminal::stdout_is_terminal() && human_format_requested {
        eprintln!("{JSON_LINES_STDOUT_NOTICE}");
    }

    run_stream(async move {
        let mut ws = ws::websocket_for_chain(chain);
        let label = target.subscription.to_string();
        ws.subscribe(target.subscription.clone());
        let mut stdout = io::stdout();
        stream_subscription_events(
            &label,
            bounds,
            |message| target.matches(message),
            &mut ws,
            &mut stdout,
            projection,
        )
        .await?;
        Ok(StreamEnd::Finished)
    })
    .await
}

/// Stream pre-built WebSocket events to a writer as subscription NDJSON.
///
/// This is the deterministic core behind [`stream_subscription`]. Production
/// passes the real Hyperliquid WebSocket stream, while tests can pass a fake
/// stream of [`WsEvent`] values and a byte buffer.
///
/// A disconnect prints a stderr notice when stdout is a terminal. When the
/// connection comes back, one `{"type":"warning","reason":"stream_gap"}` line
/// marks the point where events may have been missed.
pub async fn stream_subscription_events<S, F, W>(
    subscription_label: &str,
    bounds: StreamBounds,
    event_matches: F,
    events: &mut S,
    output: &mut W,
    projection: &JsonRenderOptions,
) -> Result<(), anyhow::Error>
where
    S: Stream<Item = WsEvent> + Unpin + ?Sized,
    F: Fn(&Incoming) -> bool,
    W: Write + ?Sized,
{
    emit_json_line(
        output,
        &serde_json::json!({
            "type": "subscribed",
            "subscription": subscription_label,
        }),
        projection,
    )?;

    if bounds.max_items == Some(0) {
        return Ok(());
    }

    let mut emitted_events = 0usize;
    let mut gap_pending = false;
    let mut idle = IdleTimer::new(bounds.idle);

    loop {
        tokio::select! {
            idle_for = idle.expired() => {
                return Err(subscription_idle_timeout_error(idle_for).into());
            }
            event = events.next() => {
                let Some(event) = event else {
                    return Err(subscription_closed_error().into());
                };

                if handle_subscription_event(
                    event,
                    subscription_label,
                    &event_matches,
                    &mut gap_pending,
                    output,
                    projection,
                )? {
                    emitted_events += 1;
                    if bounds.reached(emitted_events) {
                        return Ok(());
                    }
                    idle.restart();
                }
            }
        }
    }
}

/// Apply one WebSocket event to the stream. Returns whether an event line was written.
fn handle_subscription_event<F, W>(
    event: WsEvent,
    subscription_label: &str,
    event_matches: &F,
    gap_pending: &mut bool,
    output: &mut W,
    projection: &JsonRenderOptions,
) -> Result<bool, anyhow::Error>
where
    F: Fn(&Incoming) -> bool,
    W: Write + ?Sized,
{
    match event {
        WsEvent::Message(message) if event_matches(&message) => {
            emit_json_line(
                output,
                &serde_json::json!({
                    "type": "event",
                    "subscription": subscription_label,
                    "data": message,
                }),
                projection,
            )?;
            Ok(true)
        }
        WsEvent::Message(_) => Ok(false),
        WsEvent::Disconnected => {
            if !*gap_pending {
                *gap_pending = true;
                if terminal::stdout_is_terminal() {
                    eprintln!("{CONNECTION_LOST_NOTICE}");
                }
            }
            Ok(false)
        }
        WsEvent::Connected => {
            if *gap_pending {
                *gap_pending = false;
                // Control line: written unprojected so --select cannot drop the gap marker.
                write_line(
                    output,
                    &serde_json::to_string(&serde_json::json!({
                        "type": "warning",
                        "reason": "stream_gap",
                        "subscription": subscription_label,
                        "message": STREAM_GAP_MESSAGE,
                    }))?,
                )?;
            }
            Ok(false)
        }
    }
}

fn subscription_closed_error() -> CliError {
    CliError::Unavailable("WebSocket closed before receiving subscription events.".to_string())
}

fn subscription_idle_timeout_error(idle_timeout: Duration) -> CliError {
    CliError::Timeout(format!(
        "Timed out waiting for subscription events after {}.",
        output::format::duration(idle_timeout)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn broken_pipe_write_becomes_stdout_closed() {
        struct ClosedPipe;
        impl Write for ClosedPipe {
            fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let err = write_line(&mut ClosedPipe, "{}").unwrap_err();
        assert!(is_stdout_closed(&err));
    }

    #[test]
    fn other_write_errors_stay_errors() {
        struct Failing;
        impl Write for Failing {
            fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("disk full"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let err = write_line(&mut Failing, "{}").unwrap_err();
        assert!(!is_stdout_closed(&err));
        assert!(err.to_string().contains("disk full"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shutdown_signals_map_to_128_plus_signal() {
        let mut signals = ShutdownSignals::install().unwrap();
        let pid = std::process::id().to_string();

        for (name, expected) in [("-HUP", 129), ("-TERM", 143), ("-INT", 130)] {
            let status = std::process::Command::new("kill")
                .args([name, &pid])
                .status()
                .unwrap();
            assert!(status.success());
            let code = tokio::time::timeout(Duration::from_secs(5), signals.recv())
                .await
                .expect("signal should arrive");
            assert_eq!(code, expected, "{name}");
        }
    }

    #[test]
    fn bounds_without_any_limit_are_unbounded() {
        assert!(!StreamBounds::default().is_bounded());
        assert!(
            StreamBounds {
                max_items: Some(0),
                idle: None
            }
            .is_bounded()
        );
        assert!(
            StreamBounds::watch_risk(&StreamBoundArgs::default())
                .max_items
                .is_none()
        );
        assert_eq!(
            StreamBounds::watch_risk(&StreamBoundArgs {
                max_events: Some(3),
                idle_timeout_ms: Some(250),
            }),
            StreamBounds {
                max_items: Some(3),
                idle: Some(Duration::from_millis(250)),
            }
        );
    }

    #[test]
    fn explicit_tick_bound_wins_over_environment() {
        assert_eq!(StreamBounds::snapshot_watch(Some(4)).max_items, Some(4));
        assert!(require_tick_bound(Some(4)).is_ok());
    }

    #[test]
    fn explicit_event_or_idle_bound_satisfies_subscribe_requirement() {
        let events = StreamBoundArgs {
            max_events: Some(0),
            idle_timeout_ms: None,
        };
        let idle = StreamBoundArgs {
            max_events: None,
            idle_timeout_ms: Some(10),
        };
        assert!(StreamBounds::subscribe(&events).is_bounded());
        assert!(StreamBounds::subscribe(&idle).is_bounded());
    }

    struct CountingTicker {
        ticks: Arc<AtomicUsize>,
        per_tick: usize,
    }

    impl Ticker for CountingTicker {
        type Item = usize;

        async fn tick(&mut self) -> Result<Vec<usize>, anyhow::Error> {
            let tick = self.ticks.fetch_add(1, Ordering::SeqCst) + 1;
            Ok((0..self.per_tick).map(|index| tick * 10 + index).collect())
        }
    }

    #[tokio::test]
    async fn poll_stream_stops_at_item_bound_mid_tick() {
        let ticks = Arc::new(AtomicUsize::new(0));
        let mut ticker = CountingTicker {
            ticks: Arc::clone(&ticks),
            per_tick: 2,
        };
        let mut seen = Vec::new();

        poll_stream(
            &mut ticker,
            Duration::ZERO,
            StreamBounds {
                max_items: Some(3),
                idle: None,
            },
            |item| {
                seen.push(item);
                Ok(())
            },
        )
        .await
        .unwrap();

        assert_eq!(seen, vec![10, 11, 20]);
        assert_eq!(ticks.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn poll_stream_with_zero_item_bound_never_ticks() {
        let ticks = Arc::new(AtomicUsize::new(0));
        let mut ticker = CountingTicker {
            ticks: Arc::clone(&ticks),
            per_tick: 1,
        };

        poll_stream(
            &mut ticker,
            Duration::ZERO,
            StreamBounds {
                max_items: Some(0),
                idle: None,
            },
            |_| Ok(()),
        )
        .await
        .unwrap();

        assert_eq!(ticks.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn poll_stream_ends_cleanly_after_idle_without_output() {
        let ticks = Arc::new(AtomicUsize::new(0));
        let mut ticker = CountingTicker {
            ticks: Arc::clone(&ticks),
            per_tick: 0,
        };

        poll_stream(
            &mut ticker,
            Duration::from_millis(2),
            StreamBounds {
                max_items: None,
                idle: Some(Duration::from_millis(10)),
            },
            |_| Ok(()),
        )
        .await
        .unwrap();

        assert!(ticks.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn poll_stream_propagates_emit_errors() {
        let mut ticker = CountingTicker {
            ticks: Arc::new(AtomicUsize::new(0)),
            per_tick: 1,
        };

        let err = poll_stream(&mut ticker, Duration::ZERO, StreamBounds::default(), |_| {
            Err(StdoutClosed.into())
        })
        .await
        .unwrap_err();

        assert!(is_stdout_closed(&err));
    }

    #[test]
    fn subscribe_targets_match_only_their_own_messages() {
        let message = Incoming::AllMids {
            dex: None,
            mids: Default::default(),
        };
        assert!(SubscribeTarget::all_mids().matches(&message));
        assert!(!SubscribeTarget::trades("BTC".to_string()).matches(&message));
    }
}
