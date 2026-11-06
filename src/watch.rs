//! Watch mode and WebSocket subscription helpers.
//!
//! Watch mode renders snapshots in-place using crossterm's alternate screen for
//! human formats and emits newline-delimited JSON for `--format json`.
//!
//! Every stream here stops on SIGINT, SIGTERM, and SIGHUP: the loop ends, the
//! terminal is restored, the current output line is complete, and the process
//! exits `128 + signal`. A closed stdout (for example `| head`) ends the stream
//! quietly with exit 0.

use std::error::Error as StdError;
use std::fmt;
use std::future::Future;
use std::io::{self, IsTerminal, Write};
use std::time::Duration;

use crossterm::cursor;
use crossterm::event::{self, Event as TerminalEvent, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
    enable_raw_mode, size as terminal_size,
};
use futures::{Stream, StreamExt};
use hypersdk::hypercore::{
    self, Chain, Incoming, Subscription, UserEvent, WebSocket, ws::Event as WsEvent,
};

use crate::errors::CliError;
use crate::output::{self, OutputFormat, TableData};

const WATCH_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const KEY_POLL_INTERVAL: Duration = Duration::from_millis(150);

/// Exit status for SIGHUP (`128 + 1`).
const EXIT_SIGHUP: i32 = 129;
/// Exit status for SIGINT and the Ctrl+C key (`128 + 2`).
const EXIT_SIGINT: i32 = 130;
/// Exit status for SIGTERM (`128 + 15`).
const EXIT_SIGTERM: i32 = 143;

const JSON_LINES_STDOUT_NOTICE: &str = "Streaming JSON lines because stdout isn't a terminal.";
const JSON_LINES_STDIN_NOTICE: &str = "Streaming JSON lines because stdin isn't a terminal.";
const CONNECTION_LOST_NOTICE: &str = "! Lost connection. Retrying automatically…";
const STREAM_GAP_MESSAGE: &str = "Connection was lost and restored. Events may have been missed.";

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
pub fn write_stdout_line(line: &str) -> Result<(), anyhow::Error> {
    write_line(&mut io::stdout().lock(), line)
}

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

/// Flush the standard streams and exit with a signal status. Call after cleanup.
pub fn exit_for_signal(code: i32) -> ! {
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    std::process::exit(code)
}

/// Subscription stream event families supported by the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscribeEventKind {
    Trades,
    Orderbook,
    Candles,
    AllMids,
    OrderUpdates,
    Fills,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchEvent {
    Timer,
    WebSocketMessage,
    WebSocketClosed,
    ConnectionLost,
    ConnectionRestored,
    KeyRefresh,
    KeyQuit,
    KeyInterrupt,
    Continue,
}

/// How a watch loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchEnd {
    Finished,
    Signal(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotWatchRenderMode {
    Ndjson,
    Tui(OutputFormat),
}

trait WatchInput {
    async fn next_event(&mut self) -> Result<WatchEvent, anyhow::Error>;
}

trait WatchOutput {
    fn emit_json(&mut self, line: &str) -> Result<(), anyhow::Error>;

    fn render_tui(
        &mut self,
        title: &str,
        data: &dyn TableData,
        format: OutputFormat,
        connection_lost: bool,
    ) -> Result<(), anyhow::Error>;
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self, anyhow::Error> {
        enable_raw_mode()?;
        let guard = Self;
        let mut stdout = io::stdout();
        match execute!(
            stdout,
            EnterAlternateScreen,
            cursor::Hide,
            Clear(ClearType::All)
        ) {
            Ok(()) => Ok(guard),
            Err(err) => {
                drop(guard);
                Err(err.into())
            }
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, cursor::Show, LeaveAlternateScreen);
    }
}

struct StdoutWatchOutput;

impl WatchOutput for StdoutWatchOutput {
    fn emit_json(&mut self, line: &str) -> Result<(), anyhow::Error> {
        write_stdout_line(line)
    }

    fn render_tui(
        &mut self,
        title: &str,
        data: &dyn TableData,
        format: OutputFormat,
        connection_lost: bool,
    ) -> Result<(), anyhow::Error> {
        render_tui(title, data, format, connection_lost).map_err(Into::into)
    }
}

struct RealWatchInput {
    ws: WebSocket,
    read_keys: bool,
}

impl RealWatchInput {
    fn new(ws: WebSocket, read_keys: bool) -> Self {
        Self { ws, read_keys }
    }
}

impl WatchInput for RealWatchInput {
    async fn next_event(&mut self) -> Result<WatchEvent, anyhow::Error> {
        let refresh_sleep = tokio::time::sleep(WATCH_REFRESH_INTERVAL);
        tokio::pin!(refresh_sleep);

        loop {
            tokio::select! {
                _ = &mut refresh_sleep => return Ok(WatchEvent::Timer),
                event = self.ws.next() => {
                    return match event {
                        Some(WsEvent::Message(_)) => Ok(WatchEvent::WebSocketMessage),
                        Some(WsEvent::Disconnected) => Ok(WatchEvent::ConnectionLost),
                        Some(WsEvent::Connected) => Ok(WatchEvent::ConnectionRestored),
                        None => {
                            tokio::time::sleep(WATCH_REFRESH_INTERVAL).await;
                            Ok(WatchEvent::WebSocketClosed)
                        }
                    };
                }
                _ = tokio::time::sleep(KEY_POLL_INTERVAL), if self.read_keys => {
                    match try_read_key_event().await? {
                        Some(key) => return Ok(watch_key_event_action(key)),
                        None => continue,
                    }
                }
            }
        }
    }
}

/// Run a refresh-in-place watch loop for data that can be fetched as snapshots.
pub async fn run_snapshot_watch<T, F, Fut>(
    title: &str,
    chain: Chain,
    subscriptions: Vec<Subscription>,
    format: OutputFormat,
    max_ticks: Option<usize>,
    refresh: F,
) -> Result<(), anyhow::Error>
where
    T: TableData,
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, anyhow::Error>>,
{
    let max_ticks = max_ticks
        .or_else(|| max_count_from_env("HYPERLIQUID_WATCH_MAX_TICKS").filter(|value| *value > 0));
    let stdin_is_tty = io::stdin().is_terminal();
    let stdout_is_tty = io::stdout().is_terminal();
    let render_mode = snapshot_watch_render_mode(format, max_ticks, stdin_is_tty, stdout_is_tty)?;

    // A human format that reaches the NDJSON path was asked for explicitly:
    // non-terminal stdout defaults to JSON before this point.
    if render_mode == SnapshotWatchRenderMode::Ndjson && format != OutputFormat::Json {
        eprintln!(
            "{}",
            if stdout_is_tty {
                JSON_LINES_STDIN_NOTICE
            } else {
                JSON_LINES_STDOUT_NOTICE
            }
        );
    }

    let mut signals = ShutdownSignals::install()?;
    let ws = websocket_for_chain(chain);
    for subscription in subscriptions {
        ws.subscribe(subscription);
    }

    let mut input = RealWatchInput::new(ws, matches!(render_mode, SnapshotWatchRenderMode::Tui(_)));
    let mut output = StdoutWatchOutput;

    // The terminal guard drops at the end of its arm, before any exit below.
    let end = match render_mode {
        SnapshotWatchRenderMode::Ndjson => {
            tokio::select! {
                end = run_snapshot_watch_core(
                    title,
                    OutputFormat::Json,
                    max_ticks,
                    refresh,
                    &mut input,
                    &mut output,
                ) => end,
                code = signals.recv() => Ok(WatchEnd::Signal(code)),
            }
        }
        SnapshotWatchRenderMode::Tui(format) => {
            let _terminal = enter_watch_terminal()?;
            tokio::select! {
                end = run_snapshot_watch_core(
                    title,
                    format,
                    max_ticks,
                    refresh,
                    &mut input,
                    &mut output,
                ) => end,
                code = signals.recv() => Ok(WatchEnd::Signal(code)),
            }
        }
    };

    match end {
        Ok(WatchEnd::Finished) => Ok(()),
        Ok(WatchEnd::Signal(code)) => exit_for_signal(code),
        Err(err) if is_stdout_closed(&err) => Ok(()),
        Err(err) => Err(err),
    }
}

/// Stream a WebSocket subscription to stdout as NDJSON.
pub async fn stream_subscription<F>(
    chain: Chain,
    subscription: Subscription,
    max_events: Option<usize>,
    idle_timeout: Option<Duration>,
    event_matches: F,
) -> Result<(), anyhow::Error>
where
    F: Fn(&Incoming) -> bool,
{
    if !io::stdout().is_terminal() && explicit_human_format_requested() {
        eprintln!("{JSON_LINES_STDOUT_NOTICE}");
    }

    let mut signals = ShutdownSignals::install()?;
    let mut ws = websocket_for_chain(chain);
    let subscription_label = subscription.to_string();
    ws.subscribe(subscription);

    let max_events = max_events.or_else(|| max_count_from_env("HYPERLIQUID_SUBSCRIBE_MAX_EVENTS"));
    let mut stdout = io::stdout();
    let result = tokio::select! {
        result = stream_subscription_events(
            &subscription_label,
            max_events,
            idle_timeout,
            event_matches,
            &mut ws,
            &mut stdout,
        ) => result,
        code = signals.recv() => exit_for_signal(code),
    };

    match result {
        Err(err) if is_stdout_closed(&err) => Ok(()),
        other => other,
    }
}

/// Whether `--format`/`-f` or `HYPERLIQUID_FORMAT` explicitly asked for a human format.
fn explicit_human_format_requested() -> bool {
    requested_human_format(
        std::env::args().skip(1),
        std::env::var("HYPERLIQUID_FORMAT").ok().as_deref(),
    )
}

fn requested_human_format(args: impl IntoIterator<Item = String>, env: Option<&str>) -> bool {
    fn is_format_name(value: &str) -> bool {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "pretty" | "table" | "json"
        )
    }

    let mut flag_value: Option<String> = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        if arg == "--format" || arg == "-f" {
            flag_value = args.next();
        } else if let Some(value) = arg.strip_prefix("--format=") {
            flag_value = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("-f=") {
            flag_value = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("-f")
            && is_format_name(value)
        {
            flag_value = Some(value.to_string());
        }
    }

    let raw = flag_value.or_else(|| env.map(str::to_string));
    raw.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "pretty" | "table"
        )
    })
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
    max_events: Option<usize>,
    idle_timeout: Option<Duration>,
    event_matches: F,
    events: &mut S,
    output: &mut W,
) -> Result<(), anyhow::Error>
where
    S: Stream<Item = WsEvent> + Unpin + ?Sized,
    F: Fn(&Incoming) -> bool,
    W: Write + ?Sized,
{
    write_subscription_line(
        output,
        &serde_json::json!({
            "type": "subscribed",
            "subscription": subscription_label,
        }),
    )?;

    if max_events == Some(0) {
        return Ok(());
    }

    let mut emitted_events = 0usize;
    let mut gap_pending = false;
    if let Some(idle_timeout) = idle_timeout {
        let idle_sleep = tokio::time::sleep(idle_timeout);
        tokio::pin!(idle_sleep);

        loop {
            tokio::select! {
                _ = &mut idle_sleep => {
                    return Err(subscription_idle_timeout_error(idle_timeout).into());
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
                    )? {
                        emitted_events += 1;
                        if max_events.is_some_and(|max| emitted_events >= max) {
                            return Ok(());
                        }
                        idle_sleep.as_mut().reset(tokio::time::Instant::now() + idle_timeout);
                    }
                }
            }
        }
    }

    while let Some(event) = events.next().await {
        if handle_subscription_event(
            event,
            subscription_label,
            &event_matches,
            &mut gap_pending,
            output,
        )? {
            emitted_events += 1;
            if max_events.is_some_and(|max| emitted_events >= max) {
                return Ok(());
            }
        }
    }

    Err(subscription_closed_error().into())
}

/// Apply one WebSocket event to the stream. Returns whether an event line was written.
fn handle_subscription_event<F, W>(
    event: WsEvent,
    subscription_label: &str,
    event_matches: &F,
    gap_pending: &mut bool,
    output: &mut W,
) -> Result<bool, anyhow::Error>
where
    F: Fn(&Incoming) -> bool,
    W: Write + ?Sized,
{
    match event {
        WsEvent::Message(message) if event_matches(&message) => {
            write_subscription_line(
                output,
                &serde_json::json!({
                    "type": "event",
                    "subscription": subscription_label,
                    "data": message,
                }),
            )?;
            Ok(true)
        }
        WsEvent::Message(_) => Ok(false),
        WsEvent::Disconnected => {
            if !*gap_pending {
                *gap_pending = true;
                if io::stdout().is_terminal() {
                    eprintln!("{CONNECTION_LOST_NOTICE}");
                }
            }
            Ok(false)
        }
        WsEvent::Connected => {
            if *gap_pending {
                *gap_pending = false;
                // Control line: written as-is so --select cannot drop the gap marker.
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

/// Return whether an incoming WebSocket message belongs to a subscribe command.
pub fn subscription_event_matches(kind: SubscribeEventKind, message: &Incoming) -> bool {
    match kind {
        SubscribeEventKind::Trades => matches!(message, Incoming::Trades(_)),
        SubscribeEventKind::Orderbook => matches!(message, Incoming::L2Book(_)),
        SubscribeEventKind::Candles => matches!(message, Incoming::Candle(_)),
        SubscribeEventKind::AllMids => matches!(message, Incoming::AllMids { .. }),
        SubscribeEventKind::OrderUpdates => matches!(message, Incoming::OrderUpdates(_)),
        SubscribeEventKind::Fills => matches!(
            message,
            Incoming::UserFills { .. } | Incoming::UserEvents(UserEvent::Fills { .. })
        ),
    }
}

fn write_subscription_line<W: Write + ?Sized>(
    output: &mut W,
    value: &serde_json::Value,
) -> Result<(), anyhow::Error> {
    let line = if output::json_projection_options_enabled() {
        let value = output::apply_current_json_stream_options(value.clone());
        serde_json::to_string(&value)?
    } else {
        serde_json::to_string(value)?
    };
    write_line(output, &line)
}

fn subscription_closed_error() -> CliError {
    CliError::Unavailable("WebSocket closed before receiving subscription events.".to_string())
}

fn subscription_idle_timeout_error(idle_timeout: Duration) -> CliError {
    CliError::Timeout(format!(
        "Timed out waiting for subscription events after {}.",
        format_duration_compact(idle_timeout)
    ))
}

/// Compact duration: `250ms`, `1.5s`, `2m`, `1h 5m`. Sub-units are truncated.
fn format_duration_compact(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1_000 {
        return format!("{millis}ms");
    }
    if millis < 60_000 {
        let tenths = millis / 100;
        let (whole, tenth) = (tenths / 10, tenths % 10);
        return if tenth == 0 {
            format!("{whole}s")
        } else {
            format!("{whole}.{tenth}s")
        };
    }

    let total_secs = millis / 1_000;
    let (hours, minutes, seconds) = (
        total_secs / 3_600,
        (total_secs % 3_600) / 60,
        total_secs % 60,
    );
    [(hours, 'h'), (minutes, 'm'), (seconds, 's')]
        .into_iter()
        .filter(|(value, _)| *value > 0)
        .map(|(value, unit)| format!("{value}{unit}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Render a [`TableData`] value as a compact single NDJSON line.
pub fn ndjson_line(data: &dyn TableData) -> Result<String, serde_json::Error> {
    output::render_json_compact(data)
}

async fn run_snapshot_watch_core<T, F, Fut, I, O>(
    title: &str,
    format: OutputFormat,
    max_ticks: Option<usize>,
    mut refresh: F,
    input: &mut I,
    output: &mut O,
) -> Result<WatchEnd, anyhow::Error>
where
    T: TableData,
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, anyhow::Error>>,
    I: WatchInput + ?Sized,
    O: WatchOutput + ?Sized,
{
    let mut rendered_ticks = 0usize;
    // Each tick is a full snapshot, so a dropped connection never loses data;
    // the flag only drives the TUI notice.
    let mut connection_lost = false;

    loop {
        let snapshot = refresh().await?;
        if format == OutputFormat::Json {
            output.emit_json(&ndjson_line(&snapshot)?)?;
        } else {
            output.render_tui(title, &snapshot, format, connection_lost)?;
        }

        rendered_ticks += 1;
        if max_ticks.is_some_and(|max| rendered_ticks >= max) {
            return Ok(WatchEnd::Finished);
        }

        match input.next_event().await? {
            WatchEvent::KeyQuit => return Ok(WatchEnd::Finished),
            WatchEvent::KeyInterrupt => return Ok(WatchEnd::Signal(EXIT_SIGINT)),
            WatchEvent::ConnectionLost => connection_lost = true,
            WatchEvent::ConnectionRestored => connection_lost = false,
            WatchEvent::Timer
            | WatchEvent::WebSocketMessage
            | WatchEvent::WebSocketClosed
            | WatchEvent::KeyRefresh
            | WatchEvent::Continue => {}
        }
    }
}

fn enter_watch_terminal() -> Result<TerminalGuard, anyhow::Error> {
    TerminalGuard::enter().map_err(|err| {
        CliError::Internal(anyhow::anyhow!(
            "failed to initialize watch terminal: {err}"
        ))
        .into()
    })
}

fn snapshot_watch_render_mode(
    format: OutputFormat,
    max_ticks: Option<usize>,
    stdin_is_tty: bool,
    stdout_is_tty: bool,
) -> Result<SnapshotWatchRenderMode, CliError> {
    if format == OutputFormat::Json {
        return Ok(SnapshotWatchRenderMode::Ndjson);
    }

    if stdin_is_tty && stdout_is_tty {
        return Ok(SnapshotWatchRenderMode::Tui(format));
    }

    if max_ticks.is_some() {
        return Ok(SnapshotWatchRenderMode::Ndjson);
    }

    Err(CliError::Unsupported(non_tty_watch_error_message(
        stdin_is_tty,
        stdout_is_tty,
    )))
}

fn non_tty_watch_error_message(stdin_is_tty: bool, stdout_is_tty: bool) -> String {
    let reason = match (stdin_is_tty, stdout_is_tty) {
        (false, false) => "stdin and stdout are not TTYs",
        (false, true) => "stdin is not a TTY",
        (true, false) => "stdout is not a TTY",
        (true, true) => unreachable!("called only for non-TTY watch contexts"),
    };

    format!(
        "Non-TTY watch output cannot enter the alternate-screen TUI safely ({reason}). \
         For scripts/agents, pass --format json and --max-ticks <N> \
         (or set HYPERLIQUID_WATCH_MAX_TICKS) to receive bounded NDJSON."
    )
}

fn render_tui(
    title: &str,
    data: &dyn TableData,
    format: OutputFormat,
    connection_lost: bool,
) -> Result<(), io::Error> {
    let body = match format {
        OutputFormat::Pretty => output::render_pretty(data),
        OutputFormat::Table => output::render_table(data),
        OutputFormat::Json => unreachable!("JSON watch mode bypasses TUI rendering"),
    };

    let mut stdout = io::stdout();
    execute!(stdout, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
    let (_, height) = terminal_size().unwrap_or((120, 36));
    let available_body_lines = usize::from(height.saturating_sub(4));
    let body_lines = body
        .lines()
        .take(available_body_lines)
        .collect::<Vec<_>>()
        .join("\n");

    writeln!(stdout, "{}", output::colors::cyan(title))?;
    writeln!(
        stdout,
        "{}",
        output::colors::gray(
            "q/Ctrl-C/Ctrl-D/Esc quit • r refresh • live updates via Hyperliquid WebSocket",
        )
    )?;
    if connection_lost {
        writeln!(stdout, "{}", output::colors::yellow(CONNECTION_LOST_NOTICE))?;
    } else {
        writeln!(stdout)?;
    }
    writeln!(stdout, "{body_lines}")?;
    stdout.flush()
}

fn watch_key_event_action(key: KeyEvent) -> WatchEvent {
    let has_control = key.modifiers.contains(KeyModifiers::CONTROL);
    let plain_or_shift = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();

    match key.code {
        KeyCode::Esc => WatchEvent::KeyQuit,
        KeyCode::Char('c' | 'C') if has_control => WatchEvent::KeyInterrupt,
        KeyCode::Char('d' | 'D') if has_control => WatchEvent::KeyQuit,
        KeyCode::Char('q' | 'Q') if plain_or_shift => WatchEvent::KeyQuit,
        KeyCode::Char('r' | 'R') if plain_or_shift => WatchEvent::KeyRefresh,
        _ => WatchEvent::Continue,
    }
}

async fn try_read_key_event() -> Result<Option<KeyEvent>, anyhow::Error> {
    tokio::task::spawn_blocking(|| -> Result<Option<KeyEvent>, io::Error> {
        if event::poll(Duration::ZERO)?
            && let TerminalEvent::Key(key) = event::read()?
        {
            return Ok(Some(key));
        }
        Ok(None)
    })
    .await
    .map_err(|err| anyhow::anyhow!("watch keyboard task failed: {err}"))?
    .map_err(|err| anyhow::anyhow!("failed to read watch keyboard input: {err}"))
}

fn websocket_for_chain(chain: Chain) -> WebSocket {
    match chain {
        Chain::Mainnet => hypercore::mainnet_ws(),
        Chain::Testnet => hypercore::testnet_ws(),
    }
}

pub fn max_count_from_env(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};
    use std::collections::VecDeque;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct JsonOnlyData;

    impl TableData for JsonOnlyData {
        fn headers(&self) -> Vec<&str> {
            vec!["Message"]
        }

        fn rows(&self) -> Vec<Vec<String>> {
            vec![vec!["ok".to_string()]]
        }

        fn to_json_value(&self) -> serde_json::Value {
            serde_json::json!({"message": "ok"})
        }
    }

    #[test]
    fn ndjson_line_is_single_compact_json_object() {
        let line = ndjson_line(&JsonOnlyData).unwrap();

        assert_eq!(line, r#"{"message":"ok"}"#);
        assert!(!line.contains('\n'));
    }

    #[test]
    fn snapshot_watch_render_mode_keeps_tui_for_interactive_terminals() {
        assert_eq!(
            snapshot_watch_render_mode(OutputFormat::Pretty, None, true, true).unwrap(),
            SnapshotWatchRenderMode::Tui(OutputFormat::Pretty)
        );
        assert_eq!(
            snapshot_watch_render_mode(OutputFormat::Table, Some(1), true, true).unwrap(),
            SnapshotWatchRenderMode::Tui(OutputFormat::Table)
        );
    }

    #[test]
    fn snapshot_watch_render_mode_uses_ndjson_for_json_format() {
        assert_eq!(
            snapshot_watch_render_mode(OutputFormat::Json, None, false, false).unwrap(),
            SnapshotWatchRenderMode::Ndjson
        );
    }

    #[test]
    fn snapshot_watch_render_mode_uses_ndjson_for_bounded_non_tty_watch() {
        assert_eq!(
            snapshot_watch_render_mode(OutputFormat::Pretty, Some(1), false, false).unwrap(),
            SnapshotWatchRenderMode::Ndjson
        );
        assert_eq!(
            snapshot_watch_render_mode(OutputFormat::Table, Some(1), true, false).unwrap(),
            SnapshotWatchRenderMode::Ndjson
        );
    }

    #[test]
    fn snapshot_watch_render_mode_rejects_unbounded_non_tty_tui() {
        let error =
            snapshot_watch_render_mode(OutputFormat::Pretty, None, false, true).unwrap_err();

        assert_eq!(error.exit_code(), 13);
        let message = error.to_string();
        assert!(message.contains("Non-TTY"));
        assert!(message.contains("--format json"));
        assert!(message.contains("--max-ticks"));
    }

    #[test]
    fn watch_key_ctrl_d_and_esc_quit() {
        for key in [
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        ] {
            assert_eq!(watch_key_event_action(key), WatchEvent::KeyQuit);
        }
    }

    #[test]
    fn watch_key_ctrl_c_interrupts() {
        for code in ['c', 'C'] {
            assert_eq!(
                watch_key_event_action(KeyEvent::new(KeyCode::Char(code), KeyModifiers::CONTROL)),
                WatchEvent::KeyInterrupt
            );
        }
    }

    #[test]
    fn watch_key_q_and_shift_q_still_quit() {
        for key in [
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT),
        ] {
            assert_eq!(watch_key_event_action(key), WatchEvent::KeyQuit);
        }
    }

    #[test]
    fn watch_key_preserves_modifiers_for_control_chords() {
        assert_eq!(
            watch_key_event_action(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE)),
            WatchEvent::Continue
        );
        assert_eq!(
            watch_key_event_action(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)),
            WatchEvent::Continue
        );
        assert_eq!(
            watch_key_event_action(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)),
            WatchEvent::Continue
        );
        assert_eq!(
            watch_key_event_action(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
            WatchEvent::KeyRefresh
        );
    }

    #[derive(Clone)]
    struct TickData {
        tick: usize,
    }

    impl TableData for TickData {
        fn headers(&self) -> Vec<&str> {
            vec!["Tick"]
        }

        fn rows(&self) -> Vec<Vec<String>> {
            vec![vec![self.tick.to_string()]]
        }

        fn to_json_value(&self) -> serde_json::Value {
            serde_json::json!({ "tick": self.tick })
        }
    }

    struct ScriptedInput {
        events: VecDeque<WatchEvent>,
    }

    impl ScriptedInput {
        fn new(events: impl Into<VecDeque<WatchEvent>>) -> Self {
            Self {
                events: events.into(),
            }
        }
    }

    impl WatchInput for ScriptedInput {
        async fn next_event(&mut self) -> Result<WatchEvent, anyhow::Error> {
            Ok(self.events.pop_front().unwrap_or(WatchEvent::Timer))
        }
    }

    #[derive(Default)]
    struct CapturingOutput {
        json_lines: Vec<String>,
        tui_renders: Vec<TuiRender>,
    }

    struct TuiRender {
        title: String,
        format: OutputFormat,
        rows: Vec<Vec<String>>,
    }

    impl WatchOutput for CapturingOutput {
        fn emit_json(&mut self, line: &str) -> Result<(), anyhow::Error> {
            self.json_lines.push(line.to_string());
            Ok(())
        }

        fn render_tui(
            &mut self,
            title: &str,
            data: &dyn TableData,
            format: OutputFormat,
            _connection_lost: bool,
        ) -> Result<(), anyhow::Error> {
            self.tui_renders.push(TuiRender {
                title: title.to_string(),
                format,
                rows: data.rows(),
            });
            Ok(())
        }
    }

    fn counting_refresh(
        counter: Arc<AtomicUsize>,
    ) -> impl FnMut() -> std::future::Ready<Result<TickData, anyhow::Error>> {
        move || {
            let tick = counter.fetch_add(1, Ordering::SeqCst) + 1;
            std::future::ready(Ok(TickData { tick }))
        }
    }

    #[tokio::test]
    async fn watch_json_max_tick_output_emits_expected_snapshots_and_terminates() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut input = ScriptedInput::new([WatchEvent::Timer]);
        let mut output = CapturingOutput::default();

        run_snapshot_watch_core(
            "Live test",
            OutputFormat::Json,
            Some(2),
            counting_refresh(Arc::clone(&counter)),
            &mut input,
            &mut output,
        )
        .await
        .unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 2);
        assert_eq!(output.json_lines, vec![r#"{"tick":1}"#, r#"{"tick":2}"#]);
        assert!(output.tui_renders.is_empty());
    }

    #[tokio::test]
    async fn watch_tui_render_path_is_test_covered() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut input = ScriptedInput::new([]);
        let mut output = CapturingOutput::default();

        run_snapshot_watch_core(
            "Live TUI",
            OutputFormat::Pretty,
            Some(1),
            counting_refresh(counter),
            &mut input,
            &mut output,
        )
        .await
        .unwrap();

        assert_eq!(output.tui_renders.len(), 1);
        assert_eq!(output.tui_renders[0].title, "Live TUI");
        assert_eq!(output.tui_renders[0].format, OutputFormat::Pretty);
        assert_eq!(output.tui_renders[0].rows, vec![vec!["1".to_string()]]);
        assert!(output.json_lines.is_empty());
    }

    #[tokio::test]
    async fn watch_q_quits_after_current_snapshot() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut input = ScriptedInput::new([WatchEvent::KeyQuit]);
        let mut output = CapturingOutput::default();

        run_snapshot_watch_core(
            "Live TUI",
            OutputFormat::Table,
            None,
            counting_refresh(Arc::clone(&counter)),
            &mut input,
            &mut output,
        )
        .await
        .unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 1);
        assert_eq!(output.tui_renders.len(), 1);
    }

    #[tokio::test]
    async fn watch_r_refreshes_before_q_quits() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut input = ScriptedInput::new([WatchEvent::KeyRefresh, WatchEvent::KeyQuit]);
        let mut output = CapturingOutput::default();

        run_snapshot_watch_core(
            "Live TUI",
            OutputFormat::Pretty,
            None,
            counting_refresh(Arc::clone(&counter)),
            &mut input,
            &mut output,
        )
        .await
        .unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 2);
        assert_eq!(output.tui_renders.len(), 2);
        assert_eq!(output.tui_renders[1].rows, vec![vec!["2".to_string()]]);
    }

    #[tokio::test]
    async fn watch_websocket_message_triggers_refresh() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut input = ScriptedInput::new([WatchEvent::WebSocketMessage]);
        let mut output = CapturingOutput::default();

        run_snapshot_watch_core(
            "Live TUI",
            OutputFormat::Pretty,
            Some(2),
            counting_refresh(Arc::clone(&counter)),
            &mut input,
            &mut output,
        )
        .await
        .unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 2);
        assert_eq!(output.tui_renders.len(), 2);
    }

    #[tokio::test]
    async fn watch_websocket_close_continues_without_error() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut input = ScriptedInput::new([WatchEvent::WebSocketClosed]);
        let mut output = CapturingOutput::default();

        run_snapshot_watch_core(
            "Live TUI",
            OutputFormat::Pretty,
            Some(2),
            counting_refresh(Arc::clone(&counter)),
            &mut input,
            &mut output,
        )
        .await
        .unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 2);
        assert_eq!(output.tui_renders.len(), 2);
    }

    #[tokio::test]
    async fn watch_refresh_error_propagates_clearly() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut input = ScriptedInput::new([WatchEvent::KeyRefresh]);
        let mut output = CapturingOutput::default();

        let result = run_snapshot_watch_core(
            "Live TUI",
            OutputFormat::Pretty,
            None,
            {
                let counter = Arc::clone(&counter);
                move || {
                    let tick = counter.fetch_add(1, Ordering::SeqCst) + 1;
                    std::future::ready(if tick == 1 {
                        Ok(TickData { tick })
                    } else {
                        Err(CliError::Unavailable(
                            "refresh failed in deterministic test".to_string(),
                        )
                        .into())
                    })
                }
            },
            &mut input,
            &mut output,
        )
        .await;

        let err = result.unwrap_err().to_string();
        assert!(err.contains("refresh failed in deterministic test"));
        assert_eq!(counter.load(Ordering::SeqCst), 2);
        assert_eq!(output.tui_renders.len(), 1);
    }

    #[tokio::test]
    async fn watch_ctrl_c_key_ends_with_sigint_status() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut input = ScriptedInput::new([WatchEvent::KeyInterrupt]);
        let mut output = CapturingOutput::default();

        let end = run_snapshot_watch_core(
            "Live TUI",
            OutputFormat::Pretty,
            None,
            counting_refresh(counter),
            &mut input,
            &mut output,
        )
        .await
        .unwrap();

        assert_eq!(end, WatchEnd::Signal(130));
    }

    #[tokio::test]
    async fn watch_connection_events_keep_refreshing() {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut input =
            ScriptedInput::new([WatchEvent::ConnectionLost, WatchEvent::ConnectionRestored]);
        let mut output = CapturingOutput::default();

        let end = run_snapshot_watch_core(
            "Live TUI",
            OutputFormat::Pretty,
            Some(3),
            counting_refresh(Arc::clone(&counter)),
            &mut input,
            &mut output,
        )
        .await
        .unwrap();

        assert_eq!(end, WatchEnd::Finished);
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

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

    #[test]
    fn compact_durations_use_the_largest_readable_unit() {
        let cases = [
            (5, "5ms"),
            (999, "999ms"),
            (1_000, "1s"),
            (1_500, "1.5s"),
            (10_000, "10s"),
            (59_999, "59.9s"),
            (60_000, "1m"),
            (130_000, "2m 10s"),
            (120_000, "2m"),
            (3_900_000, "1h 5m"),
        ];
        for (millis, expected) in cases {
            assert_eq!(
                format_duration_compact(Duration::from_millis(millis)),
                expected,
                "{millis}ms"
            );
        }
    }

    #[test]
    fn explicit_human_format_detection_follows_flag_then_env() {
        let args = |items: &[&str]| {
            items
                .iter()
                .map(|item| item.to_string())
                .collect::<Vec<_>>()
        };

        assert!(requested_human_format(
            args(&["--format", "table", "mids"]),
            None
        ));
        assert!(requested_human_format(args(&["-f", "pretty"]), None));
        assert!(requested_human_format(args(&["--format=table"]), None));
        assert!(requested_human_format(args(&["-ftable"]), None));
        assert!(!requested_human_format(args(&["--format", "json"]), None));
        assert!(!requested_human_format(args(&["mids"]), None));
        assert!(requested_human_format(args(&["mids"]), Some("table")));
        // An explicit flag wins over the environment.
        assert!(!requested_human_format(
            args(&["-f", "json"]),
            Some("table")
        ));
        assert!(!requested_human_format(args(&["mids"]), Some("json")));
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
}
