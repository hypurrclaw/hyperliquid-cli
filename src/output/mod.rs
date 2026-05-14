//! Output formatting system for Hyperliquid CLI.
//!
//! Supports three output formats:
//! - `Pretty` — human-readable, colorized, tabwriter-aligned (default)
//! - `Table` — bordered tables via tabled
//! - `Json` — stable JSON output with snake_case keys
//!
//! Color theme:
//! - Cyan: headers
//! - Green: positive values (profit, gains)
//! - Red: negative values (loss, declines)
//! - Gray: muted/secondary text
//! - Yellow: warnings
//!
//! Error routing:
//! - JSON mode: errors printed to stdout as `{"error": "..."}`
//! - Pretty mode: errors printed to stderr with a red prefix
//! - Table mode: errors printed to stderr without ANSI color
//!
//! Timing feedback:
//! - "Completed in X.XXs" printed to stderr after output

#![allow(dead_code)]

use clap::ValueEnum;
use serde_json::Value;
use std::fmt;
use std::io::Write;
use std::sync::{OnceLock, RwLock};
use tabwriter::TabWriter;

// ── Color helpers ───────────────────────────────────────────────────────

/// ANSI color codes for the CLI color theme.
pub mod colors {
    pub const CYAN: &str = "\x1b[36m";
    pub const GREEN: &str = "\x1b[32m";
    pub const RED: &str = "\x1b[31m";
    pub const GRAY: &str = "\x1b[90m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BOLD: &str = "\x1b[1m";
    pub const RESET: &str = "\x1b[0m";

    /// Wrap text in cyan color.
    pub fn cyan(text: &str) -> String {
        format!("{CYAN}{text}{RESET}")
    }

    /// Wrap text in green color.
    pub fn green(text: &str) -> String {
        format!("{GREEN}{text}{RESET}")
    }

    /// Wrap text in red color.
    pub fn red(text: &str) -> String {
        format!("{RED}{text}{RESET}")
    }

    /// Wrap text in gray color (muted).
    pub fn gray(text: &str) -> String {
        format!("{GRAY}{text}{RESET}")
    }

    /// Wrap text in yellow color (warning).
    pub fn yellow(text: &str) -> String {
        format!("{YELLOW}{text}{RESET}")
    }

    /// Wrap text in bold.
    pub fn bold(text: &str) -> String {
        format!("{BOLD}{text}{RESET}")
    }

    /// Color a PnL-style value: green for positive, red for negative, default for zero.
    pub fn pnl<T: std::fmt::Display>(value: T, is_positive: bool) -> String {
        let s = format!("{value}");
        if is_positive {
            format!("{GREEN}{s}{RESET}")
        } else {
            format!("{RED}{s}{RESET}")
        }
    }

    /// Check if stdout is connected to a terminal (supports colors).
    pub fn should_colorize() -> bool {
        use std::io::IsTerminal;
        std::io::stdout().is_terminal()
    }
}

// ── OutputFormat enum ───────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Pretty,
    Table,
    Json,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pretty => write!(f, "pretty"),
            Self::Table => write!(f, "table"),
            Self::Json => write!(f, "json"),
        }
    }
}

// ── JSON projection options ─────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JsonRenderOptions {
    selected_fields: Option<Vec<String>>,
    results_only: bool,
    max_results: Option<usize>,
}

impl JsonRenderOptions {
    #[must_use]
    pub fn from_cli(select: Option<&str>, results_only: bool, max_results: Option<usize>) -> Self {
        Self {
            selected_fields: select.map(parse_selected_fields),
            results_only,
            max_results,
        }
    }

    #[must_use]
    pub fn max_results(&self) -> Option<usize> {
        self.max_results
    }

    #[must_use]
    pub fn selected_fields(&self) -> Option<&[String]> {
        self.selected_fields.as_deref()
    }
}

static JSON_OPTIONS: OnceLock<RwLock<JsonRenderOptions>> = OnceLock::new();

fn json_options() -> &'static RwLock<JsonRenderOptions> {
    JSON_OPTIONS.get_or_init(|| RwLock::new(JsonRenderOptions::default()))
}

/// Configure process-wide JSON rendering options for the current CLI invocation.
///
/// The binary is a one-command process, so this lets every command module keep
/// using the shared output renderer while global flags such as `--select` and
/// `--results-only` are still enforced consistently.
pub fn set_json_options(select: Option<&str>, results_only: bool) {
    set_json_options_with_limit(select, results_only, None);
}

/// Configure process-wide JSON rendering options, including an optional
/// top-level result limiter for agent context-window control.
pub fn set_json_options_with_limit(
    select: Option<&str>,
    results_only: bool,
    max_results: Option<usize>,
) {
    let mut options = json_options()
        .write()
        .expect("json options lock should not be poisoned");
    *options = JsonRenderOptions::from_cli(select, results_only, max_results);
}

fn current_json_options() -> JsonRenderOptions {
    json_options()
        .read()
        .expect("json options lock should not be poisoned")
        .clone()
}

pub fn json_projection_options_enabled() -> bool {
    let options = current_json_options();
    options.selected_fields.is_some() || options.results_only || options.max_results.is_some()
}

fn parse_selected_fields(select: &str) -> Vec<String> {
    select
        .split(',')
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(|field| field.to_ascii_lowercase())
        .collect()
}

fn apply_json_options(value: serde_json::Value, options: &JsonRenderOptions) -> serde_json::Value {
    let value = if options.results_only {
        strip_results_envelope(value)
    } else {
        value
    };

    let value = if let Some(max_results) = options.max_results {
        limit_json_results(value, max_results)
    } else {
        value
    };

    if let Some(fields) = options.selected_fields.as_deref() {
        project_selected_fields(value, fields)
    } else {
        value
    }
}

/// Apply the current global JSON options to an arbitrary JSON value.
#[must_use]
pub fn apply_current_json_options_to_value(value: serde_json::Value) -> serde_json::Value {
    apply_json_options(value, &current_json_options())
}

/// Apply global JSON options to a JSONL stream envelope.
///
/// Stream envelopes should keep their top-level `type`, `subscription`, and `data`
/// keys unless the caller explicitly selects top-level fields. `--max-results`
/// is therefore applied recursively to nested payloads instead of truncating the
/// envelope itself.
#[must_use]
pub fn apply_current_json_stream_options(value: serde_json::Value) -> serde_json::Value {
    let options = current_json_options();
    apply_json_stream_options(value, &options)
}

/// Apply explicit JSON options to a JSONL stream envelope.
#[must_use]
pub fn apply_json_stream_options(
    value: serde_json::Value,
    options: &JsonRenderOptions,
) -> serde_json::Value {
    let value = if options.results_only {
        strip_results_envelope(value)
    } else {
        value
    };
    let value = if let Some(max_results) = options.max_results {
        limit_json_stream_results(value, max_results)
    } else {
        value
    };

    if let Some(fields) = options.selected_fields.as_deref() {
        project_stream_selected_fields(value, fields)
    } else {
        value
    }
}

fn limit_json_results(value: serde_json::Value, max_results: usize) -> serde_json::Value {
    match value {
        serde_json::Value::Array(rows) => {
            serde_json::Value::Array(rows.into_iter().take(max_results).collect())
        }
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .into_iter()
                .take(max_results)
                .collect::<serde_json::Map<String, serde_json::Value>>(),
        ),
        scalar => scalar,
    }
}

fn limit_json_stream_results(value: serde_json::Value, max_results: usize) -> serde_json::Value {
    match value {
        serde_json::Value::Array(rows) => {
            serde_json::Value::Array(rows.into_iter().take(max_results).collect())
        }
        serde_json::Value::Object(object) if is_stream_envelope(&object) => {
            serde_json::Value::Object(
                object
                    .into_iter()
                    .map(|(key, value)| (key, limit_json_stream_results(value, max_results)))
                    .collect(),
            )
        }
        serde_json::Value::Object(object) => {
            let is_leaf_object = object.values().all(|value| {
                !matches!(
                    value,
                    serde_json::Value::Array(_) | serde_json::Value::Object(_)
                )
            });
            let entries = object
                .into_iter()
                .map(|(key, value)| (key, limit_json_stream_results(value, max_results)));

            if is_leaf_object {
                serde_json::Value::Object(
                    entries
                        .take(max_results)
                        .collect::<serde_json::Map<String, serde_json::Value>>(),
                )
            } else {
                serde_json::Value::Object(entries.collect())
            }
        }
        scalar => scalar,
    }
}

fn is_stream_envelope(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    object.contains_key("type")
        && (object.contains_key("subscription") || object.contains_key("data"))
}

fn strip_results_envelope(value: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(mut object) = value else {
        return value;
    };

    for key in ["data", "results", "result"] {
        if let Some(result) = object.remove(key) {
            return result;
        }
    }

    serde_json::Value::Object(object)
}

fn project_selected_fields(value: serde_json::Value, fields: &[String]) -> serde_json::Value {
    match value {
        serde_json::Value::Array(rows) => serde_json::Value::Array(
            rows.into_iter()
                .map(|row| project_selected_fields(row, fields))
                .collect(),
        ),
        serde_json::Value::Object(object) => {
            if object.values().all(serde_json::Value::is_object) {
                serde_json::Value::Object(
                    object
                        .into_iter()
                        .map(|(key, value)| (key, project_selected_fields(value, fields)))
                        .collect(),
                )
            } else {
                serde_json::Value::Object(project_object_fields(&object, fields))
            }
        }
        scalar => scalar,
    }
}

fn project_stream_selected_fields(
    value: serde_json::Value,
    fields: &[String],
) -> serde_json::Value {
    let serde_json::Value::Object(mut object) = value else {
        return project_selected_fields(value, fields);
    };

    let top_level_projection = project_object_fields(&object, fields);
    let nested_projection = object
        .remove("data")
        .map(|data| project_selected_fields(data, fields));

    if !top_level_projection.is_empty() {
        let mut projection = top_level_projection;
        if !projection.contains_key("data")
            && let Some(data) = nested_projection.filter(|data| !data.is_null())
        {
            let include_data = match &data {
                serde_json::Value::Object(map) => !map.is_empty(),
                serde_json::Value::Array(items) => !items.is_empty(),
                _ => true,
            };
            if include_data {
                projection.insert("data".to_string(), data);
            }
        }
        return serde_json::Value::Object(projection);
    }

    if let Some(data) = nested_projection {
        object.insert("data".to_string(), data);
    }
    serde_json::Value::Object(object)
}

fn project_object_fields(
    object: &serde_json::Map<String, serde_json::Value>,
    fields: &[String],
) -> serde_json::Map<String, serde_json::Value> {
    let mut projected = serde_json::Map::new();

    for field in fields {
        if let Some(value) = object.get(field) {
            projected.insert(field.clone(), value.clone());
            continue;
        }

        if let Some((alias, value)) = alias_value_for_field(object, field) {
            projected.insert(alias.to_string(), value.clone());
        }
    }

    projected
}

fn alias_value_for_field<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<(&'static str, &'a serde_json::Value)> {
    match field {
        "coin" => object
            .get("coin")
            .or_else(|| object.get("name"))
            .or_else(|| object.get("asset"))
            .map(|value| ("coin", value)),
        "price" => object
            .get("price")
            .or_else(|| object.get("limit_price"))
            .or_else(|| object.get("limit_px"))
            .or_else(|| object.get("limitPx"))
            .or_else(|| object.get("mid"))
            .or_else(|| object.get("mark_price"))
            .or_else(|| object.get("oracle_price"))
            .map(|value| ("price", value)),
        "size" => object
            .get("size")
            .or_else(|| object.get("sz"))
            .or_else(|| object.get("amount"))
            .map(|value| ("size", value)),
        _ => None,
    }
}

// ── Table data trait ────────────────────────────────────────────────────

/// Trait for data that can be rendered in all output formats.
///
/// Implement this trait for any data type that needs to be displayed
/// through the output system. Each method returns a string representation
/// suitable for the corresponding format.
pub trait TableData {
    /// Column headers for the data.
    fn headers(&self) -> Vec<&str>;

    /// Row data as string vectors. Each inner Vec is one row.
    fn rows(&self) -> Vec<Vec<String>>;

    /// Row data for pretty output.
    ///
    /// Implementors can override this to apply human-facing ANSI styling to
    /// selected cells while keeping [`Self::rows`] plain for table output.
    fn pretty_rows(&self) -> Vec<Vec<String>> {
        self.rows()
    }

    /// Convert to a JSON-serializable value.
    fn to_json_value(&self) -> serde_json::Value;
}

/// Generic output wrapper for raw JSON info endpoints.
///
/// Command modules should prefer typed rows when they have a stable domain
/// model. This wrapper is for documented API endpoints whose nested response
/// shape is valuable for agents after normalizing object keys to snake_case.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonValueOutput {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    value: Value,
}

impl JsonValueOutput {
    #[must_use]
    pub fn new(value: Value, empty_message: &'static str) -> Self {
        let value = snake_case_json_keys(value);
        let (headers, rows) = json_value_table_rows(&value, empty_message);
        Self {
            headers,
            rows,
            value,
        }
    }
}

impl TableData for JsonValueOutput {
    fn headers(&self) -> Vec<&str> {
        self.headers.iter().map(String::as_str).collect()
    }

    fn rows(&self) -> Vec<Vec<String>> {
        self.rows.clone()
    }

    fn to_json_value(&self) -> serde_json::Value {
        self.value.clone()
    }
}

#[must_use]
pub fn snake_case_json_keys(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (to_snake_case(&key), snake_case_json_keys(value)))
                .collect(),
        ),
        Value::Array(values) => {
            Value::Array(values.into_iter().map(snake_case_json_keys).collect())
        }
        scalar => scalar,
    }
}

fn json_value_table_rows(
    value: &Value,
    empty_message: &'static str,
) -> (Vec<String>, Vec<Vec<String>>) {
    match value {
        Value::Array(values) if values.is_empty() => (
            vec!["Message".to_string()],
            vec![vec![empty_message.to_string()]],
        ),
        Value::Array(values) => array_table_rows(values),
        Value::Object(object) if object.is_empty() => (
            vec!["Message".to_string()],
            vec![vec![empty_message.to_string()]],
        ),
        Value::Object(object) => (
            vec!["Field".to_string(), "Value".to_string()],
            object
                .iter()
                .map(|(key, value)| vec![key.clone(), json_cell(value)])
                .collect(),
        ),
        scalar => (vec!["Value".to_string()], vec![vec![json_cell(scalar)]]),
    }
}

fn array_table_rows(values: &[Value]) -> (Vec<String>, Vec<Vec<String>>) {
    if values.iter().any(|value| !value.is_object()) {
        return (
            vec!["Value".to_string()],
            values.iter().map(|value| vec![json_cell(value)]).collect(),
        );
    }

    let headers = values
        .iter()
        .filter_map(Value::as_object)
        .flat_map(|object| object.keys().cloned())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let rows = values
        .iter()
        .filter_map(Value::as_object)
        .map(|object| {
            headers
                .iter()
                .map(|header| object.get(header).map(json_cell).unwrap_or_default())
                .collect()
        })
        .collect();

    (headers, rows)
}

fn json_cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| String::new())
        }
    }
}

fn to_snake_case(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut previous_was_underscore = false;
    let mut previous_was_lower_or_digit = false;

    for ch in input.chars() {
        if matches!(ch, '-' | ' ' | '.') {
            if !output.is_empty() && !previous_was_underscore {
                output.push('_');
                previous_was_underscore = true;
            }
            previous_was_lower_or_digit = false;
            continue;
        }

        if ch == '_' {
            if !output.is_empty() && !previous_was_underscore {
                output.push('_');
            }
            previous_was_underscore = true;
            previous_was_lower_or_digit = false;
            continue;
        }

        if ch.is_ascii_uppercase() {
            if previous_was_lower_or_digit && !previous_was_underscore {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
            previous_was_underscore = false;
            previous_was_lower_or_digit = false;
            continue;
        }

        output.push(ch);
        previous_was_underscore = false;
        previous_was_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
    }

    output.trim_matches('_').to_string()
}

// ── Rendering functions ─────────────────────────────────────────────────

/// Render data in pretty format (tabwriter-aligned, colorized).
///
/// Uses tabwriter for column alignment with tab-separated input.
/// Headers are rendered in cyan. Values use contextual coloring.
pub fn render_pretty(data: &dyn TableData) -> String {
    render_pretty_with_limit(data, current_json_options().max_results())
}

/// Render data in pretty format with an explicit row limit.
pub fn render_pretty_with_limit(data: &dyn TableData, max_results: Option<usize>) -> String {
    let headers = data.headers();
    let mut rows = data.pretty_rows();
    if let Some(max_results) = max_results {
        rows.truncate(max_results);
    }

    let mut tw = TabWriter::new(Vec::new());

    // Write header line
    let header_line = headers
        .iter()
        .map(|h| colors::cyan(h))
        .collect::<Vec<_>>()
        .join("\t");
    writeln!(tw, "{header_line}").unwrap();

    // Write separator
    let sep_line = headers
        .iter()
        .map(|_| "─".repeat(12))
        .collect::<Vec<_>>()
        .join("\t");
    writeln!(tw, "{sep_line}").unwrap();

    // Write data rows
    for row in &rows {
        let line = row.join("\t");
        writeln!(tw, "{line}").unwrap();
    }

    tw.flush().unwrap();
    let output = String::from_utf8_lossy(&tw.into_inner().unwrap()).to_string();

    // Remove trailing newline for consistency
    output.trim_end().to_string()
}

/// Render data in table format (bordered tables via tabled).
///
/// Creates a clean bordered table using the tabled Builder for dynamic data.
pub fn render_table(data: &dyn TableData) -> String {
    render_table_with_limit(data, current_json_options().max_results())
}

/// Render data in table format with an explicit row limit.
pub fn render_table_with_limit(data: &dyn TableData, max_results: Option<usize>) -> String {
    use tabled::builder::Builder;
    use tabled::settings::Style;

    let headers = data.headers();
    let mut rows = data.rows();
    if let Some(max_results) = max_results {
        rows.truncate(max_results);
    }

    let mut builder = Builder::default();

    // Add header row
    builder.push_record(headers.iter().map(|s| s.to_string()));

    // Add data rows
    for row in &rows {
        builder.push_record(row.iter().cloned());
    }

    let table = builder.build().with(Style::rounded()).to_string();

    table.trim_end().to_string()
}

/// Render data as JSON (stable output with snake_case keys).
///
/// Produces a JSON array of objects where each object represents a row.
/// Keys come from the headers (converted to snake_case).
pub fn render_json(data: &dyn TableData) -> String {
    let value = json_value(data);
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "[]".to_string())
}

/// Render data as JSON using explicit per-call projection options.
pub fn render_json_with_options(data: &dyn TableData, options: &JsonRenderOptions) -> String {
    let value = json_value_with_options(data, options);
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "[]".to_string())
}

/// Return the JSON value after applying global projection/result options.
pub fn json_value(data: &dyn TableData) -> serde_json::Value {
    apply_json_options(data.to_json_value(), &current_json_options())
}

/// Return the JSON value after applying explicit per-call projection options.
pub fn json_value_with_options(
    data: &dyn TableData,
    options: &JsonRenderOptions,
) -> serde_json::Value {
    apply_json_options(data.to_json_value(), options)
}

/// Render data as a compact JSON string after applying global JSON options.
pub fn render_json_compact(data: &dyn TableData) -> Result<String, serde_json::Error> {
    serde_json::to_string(&json_value(data))
}

/// Render data using the specified format.
pub fn render(data: &dyn TableData, format: OutputFormat) -> String {
    match format {
        OutputFormat::Pretty => render_pretty(data),
        OutputFormat::Table => render_table(data),
        OutputFormat::Json => render_json(data),
    }
}

/// Render data using explicit per-call projection options.
pub fn render_with_json_options(
    data: &dyn TableData,
    format: OutputFormat,
    options: &JsonRenderOptions,
) -> String {
    match format {
        OutputFormat::Pretty => render_pretty_with_limit(data, options.max_results()),
        OutputFormat::Table => render_table_with_limit(data, options.max_results()),
        OutputFormat::Json => render_json_with_options(data, options),
    }
}

// ── Print functions ─────────────────────────────────────────────────────

/// Print data to stdout using the specified format, with timing feedback on stderr.
///
/// This is the main entry point for printing command output.
/// - Prints formatted data to stdout
/// - Prints timing feedback ("Completed in X.XXs") to stderr
pub fn print_data(data: &dyn TableData, format: OutputFormat, duration: std::time::Duration) {
    let output = render(data, format);
    println!("{output}");
    print_timing(duration);
}

/// Print data to stdout using the specified format, without timing feedback.
pub fn print_data_no_timing(data: &dyn TableData, format: OutputFormat) {
    let output = render(data, format);
    println!("{output}");
}

/// Print timing feedback to stderr.
pub fn print_timing(duration: std::time::Duration) {
    eprintln!("{}", format_timing(duration));
}

/// Format timing feedback string.
pub fn format_timing(duration: std::time::Duration) -> String {
    format!("Completed in {:.2}s", duration.as_secs_f64())
}

/// Print a plain error message with proper routing based on output format.
///
/// For richer error handling with [`crate::errors::CliError`], use
/// [`crate::errors::print_error`] instead.
///
/// - JSON mode: prints `{"error": "..."}` to **stdout**
/// - Pretty mode: prints a red `Error` prefix to **stderr**
/// - Table mode: prints a plain `Error` prefix to **stderr**
pub fn print_error(message: &str, format: OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                crate::errors::ErrorEnvelope::new(message.to_string()).to_json()
            );
        }
        OutputFormat::Pretty => {
            eprintln!("{}: {message}", colors::red("Error"));
        }
        OutputFormat::Table => {
            eprintln!("Error: {message}");
        }
    }
}

/// Print a warning message to stderr (always stderr, all formats).
pub fn print_warning(message: &str) {
    eprintln!("{}: {message}", colors::yellow("Warning"));
}

/// Print a separator line to stderr for visual feedback.
pub fn print_separator() {
    eprintln!("{}", "-".repeat(60));
}

// ── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Simple test data struct for testing the output system.
    struct TestData {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        json: serde_json::Value,
    }

    impl TableData for TestData {
        fn headers(&self) -> Vec<&str> {
            self.headers.iter().map(|s| s.as_str()).collect()
        }

        fn rows(&self) -> Vec<Vec<String>> {
            self.rows.clone()
        }

        fn to_json_value(&self) -> serde_json::Value {
            self.json.clone()
        }
    }

    fn sample_data() -> TestData {
        TestData {
            headers: vec![
                "Name".to_string(),
                "Price".to_string(),
                "Change".to_string(),
            ],
            rows: vec![
                vec![
                    "BTC".to_string(),
                    "50000.00".to_string(),
                    "+2.5%".to_string(),
                ],
                vec![
                    "ETH".to_string(),
                    "3000.00".to_string(),
                    "-1.2%".to_string(),
                ],
            ],
            json: serde_json::json!([
                {"name": "BTC", "price": "50000.00", "change": "+2.5%"},
                {"name": "ETH", "price": "3000.00", "change": "-1.2%"}
            ]),
        }
    }

    #[test]
    fn json_value_output_arrays_use_union_headers_for_objects() {
        let output = JsonValueOutput::new(
            serde_json::json!([
                {"firstKey": "a"},
                {"secondKey": "b", "firstKey": "c"}
            ]),
            "empty",
        );

        assert_eq!(output.headers(), vec!["first_key", "second_key"]);
        assert_eq!(
            output.rows(),
            vec![
                vec!["a".to_string(), String::new()],
                vec!["c".to_string(), "b".to_string()],
            ]
        );
    }

    #[test]
    fn json_value_output_mixed_arrays_use_value_column() {
        let output =
            JsonValueOutput::new(serde_json::json!([{"firstKey": "a"}, "raw", 7]), "empty");

        assert_eq!(output.headers(), vec!["Value"]);
        assert_eq!(
            output.rows(),
            vec![
                vec![r#"{"first_key":"a"}"#.to_string()],
                vec!["raw".to_string()],
                vec!["7".to_string()],
            ]
        );
    }

    #[test]
    fn test_output_format_default() {
        assert_eq!(OutputFormat::default(), OutputFormat::Pretty);
    }

    #[test]
    fn test_output_format_display() {
        assert_eq!(format!("{}", OutputFormat::Pretty), "pretty");
        assert_eq!(format!("{}", OutputFormat::Table), "table");
        assert_eq!(format!("{}", OutputFormat::Json), "json");
    }

    #[test]
    fn test_render_pretty_contains_headers() {
        let data = sample_data();
        let output = render_pretty(&data);
        // Should contain header text (with ANSI codes for cyan)
        assert!(output.contains("Name"));
        assert!(output.contains("Price"));
        assert!(output.contains("Change"));
    }

    #[test]
    fn test_render_pretty_contains_data() {
        let data = sample_data();
        let output = render_pretty(&data);
        assert!(output.contains("BTC"));
        assert!(output.contains("50000.00"));
        assert!(output.contains("ETH"));
        assert!(output.contains("3000.00"));
    }

    #[test]
    fn test_render_pretty_has_ansi_colors() {
        let data = sample_data();
        let output = render_pretty(&data);
        // Should contain cyan ANSI code for headers
        assert!(output.contains(colors::CYAN));
        assert!(output.contains(colors::RESET));
    }

    #[test]
    fn test_render_pretty_has_separator() {
        let data = sample_data();
        let output = render_pretty(&data);
        // Should contain separator line with dashes
        assert!(output.contains("─"));
    }

    #[test]
    fn test_render_table_contains_headers() {
        let data = sample_data();
        let output = render_table(&data);
        assert!(output.contains("Name"));
        assert!(output.contains("Price"));
        assert!(output.contains("Change"));
    }

    #[test]
    fn test_render_table_contains_data() {
        let data = sample_data();
        let output = render_table(&data);
        assert!(output.contains("BTC"));
        assert!(output.contains("50000.00"));
        assert!(output.contains("ETH"));
    }

    #[test]
    fn test_render_table_has_borders() {
        let data = sample_data();
        let output = render_table(&data);
        // Rounded table style uses these border characters
        assert!(
            output.contains('│')
                || output.contains('┌')
                || output.contains('┐')
                || output.contains('└')
                || output.contains('┘')
                || output.contains('─')
                || output.contains('╭')
                || output.contains('╮')
                || output.contains('╰')
                || output.contains('╯')
        );
    }

    #[test]
    fn test_render_table_no_ansi_colors() {
        let data = sample_data();
        let output = render_table(&data);
        // Table mode should NOT contain ANSI color codes
        assert!(!output.contains(colors::CYAN));
    }

    #[test]
    fn test_render_json_valid() {
        let data = sample_data();
        let output = render_json(&data);
        let parsed: serde_json::Value =
            serde_json::from_str(&output).expect("Should be valid JSON");
        assert!(parsed.is_array());
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn test_render_json_has_correct_fields() {
        let data = sample_data();
        let output = render_json(&data);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let first = &parsed.as_array().unwrap()[0];
        assert!(first.get("name").is_some());
        assert!(first.get("price").is_some());
        assert!(first.get("change").is_some());
    }

    #[test]
    fn test_render_json_snake_case_keys() {
        let data = TestData {
            headers: vec!["Asset Name".to_string(), "Max Leverage".to_string()],
            rows: vec![vec!["BTC".to_string(), "50".to_string()]],
            json: serde_json::json!([
                {"asset_name": "BTC", "max_leverage": "50"}
            ]),
        };
        let output = render_json(&data);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let first = &parsed.as_array().unwrap()[0];
        assert!(first.get("asset_name").is_some());
        assert!(first.get("max_leverage").is_some());
    }

    #[test]
    fn test_project_selected_fields_filters_array_objects() {
        let value = serde_json::json!([
            {"coin": "BTC", "price": "50000", "size": "0.1", "side": "buy"},
            {"coin": "ETH", "price": "3000", "size": "1.5", "side": "sell"}
        ]);

        let projected = project_selected_fields(
            value,
            &["coin".to_string(), "price".to_string(), "size".to_string()],
        );

        let rows = projected.as_array().unwrap();
        for row in rows {
            let object = row.as_object().unwrap();
            assert_eq!(object.len(), 3);
            assert!(object.contains_key("coin"));
            assert!(object.contains_key("price"));
            assert!(object.contains_key("size"));
        }
    }

    #[test]
    fn test_project_selected_fields_supports_agent_aliases() {
        let value = serde_json::json!([
            {"name": "BTC", "mid": "50000", "max_leverage": 50}
        ]);

        let projected = project_selected_fields(value, &["coin".to_string(), "price".to_string()]);

        assert_eq!(projected[0]["coin"], "BTC");
        assert_eq!(projected[0]["price"], "50000");
        assert_eq!(projected[0].as_object().unwrap().len(), 2);
    }

    #[test]
    fn test_project_selected_fields_supports_order_price_aliases() {
        let snake_case_value = serde_json::json!([
            {"coin": "BTC", "limit_price": "50000", "size": "0.1", "oid": 12345}
        ]);
        let sdk_value = serde_json::json!([
            {"coin": "ETH", "limit_px": "3000", "sz": "1.5", "oid": 67890}
        ]);
        let api_value = serde_json::json!([
            {"coin": "SOL", "limitPx": "100", "sz": "2", "oid": 24680}
        ]);

        let fields = &["coin".to_string(), "price".to_string(), "size".to_string()];

        let snake_case_projected = project_selected_fields(snake_case_value, fields);
        assert_eq!(snake_case_projected[0]["coin"], "BTC");
        assert_eq!(snake_case_projected[0]["price"], "50000");
        assert_eq!(snake_case_projected[0]["size"], "0.1");
        assert_eq!(snake_case_projected[0].as_object().unwrap().len(), 3);

        let sdk_projected = project_selected_fields(sdk_value, fields);
        assert_eq!(sdk_projected[0]["coin"], "ETH");
        assert_eq!(sdk_projected[0]["price"], "3000");
        assert_eq!(sdk_projected[0]["size"], "1.5");
        assert_eq!(sdk_projected[0].as_object().unwrap().len(), 3);

        let api_projected = project_selected_fields(api_value, fields);
        assert_eq!(api_projected[0]["coin"], "SOL");
        assert_eq!(api_projected[0]["price"], "100");
        assert_eq!(api_projected[0]["size"], "2");
        assert_eq!(api_projected[0].as_object().unwrap().len(), 3);
    }

    #[test]
    fn test_project_selected_fields_filters_top_level_object() {
        let value = serde_json::json!({
            "name": "BTC",
            "max_leverage": 50,
            "sz_decimals": 5
        });

        let projected =
            project_selected_fields(value, &["name".to_string(), "max_leverage".to_string()]);

        assert_eq!(projected["name"], "BTC");
        assert_eq!(projected["max_leverage"], 50);
        assert_eq!(projected.as_object().unwrap().len(), 2);
    }

    #[test]
    fn test_stream_options_limit_nested_payload_without_truncating_envelope() {
        let options = JsonRenderOptions::from_cli(None, false, Some(2));
        let value = serde_json::json!({
            "type": "event",
            "subscription": "allMids(None)",
            "data": {
                "channel": "allMids",
                "data": {
                    "mids": {
                        "BTC": "50000",
                        "ETH": "3000",
                        "SOL": "100"
                    }
                }
            }
        });

        let limited = apply_json_stream_options(value, &options);

        assert_eq!(limited["type"], "event");
        assert_eq!(limited["subscription"], "allMids(None)");
        let mids = limited["data"]["data"]["mids"].as_object().unwrap();
        assert_eq!(mids.len(), 2);
        assert!(mids.contains_key("BTC"));
        assert!(mids.contains_key("ETH"));
    }

    #[test]
    fn test_stream_options_limit_one_preserves_intermediate_wrappers() {
        let options = JsonRenderOptions::from_cli(None, false, Some(1));
        let value = serde_json::json!({
            "type": "event",
            "subscription": "allMids(None)",
            "data": {
                "channel": "allMids",
                "data": {
                    "mids": {
                        "BTC": "50000",
                        "ETH": "3000"
                    }
                }
            }
        });

        let limited = apply_json_stream_options(value, &options);

        assert_eq!(limited["data"]["channel"], "allMids");
        let payload = limited["data"]["data"]["mids"].as_object().unwrap();
        assert_eq!(payload.len(), 1);
        assert!(payload.contains_key("BTC"));
    }

    #[test]
    fn test_stream_options_project_top_level_fields() {
        let options = JsonRenderOptions::from_cli(Some("type,subscription"), false, None);
        let value = serde_json::json!({
            "type": "subscribed",
            "subscription": "allMids(None)",
            "data": {"ignored": true}
        });

        let projected = apply_json_stream_options(value, &options);

        assert_eq!(
            projected,
            serde_json::json!({
                "type": "subscribed",
                "subscription": "allMids(None)"
            })
        );
    }

    #[test]
    fn test_stream_options_project_top_level_and_nested_fields() {
        let options = JsonRenderOptions::from_cli(Some("type,coin"), false, None);
        let value = serde_json::json!({
            "type": "event",
            "subscription": "allMids(None)",
            "data": {
                "coin": "BTC",
                "price": "50000"
            }
        });

        let projected = apply_json_stream_options(value, &options);

        assert_eq!(
            projected,
            serde_json::json!({
                "type": "event",
                "data": {
                    "coin": "BTC"
                }
            })
        );
    }

    #[test]
    fn test_stream_options_explicit_data_selection_keeps_full_payload() {
        let options = JsonRenderOptions::from_cli(Some("data"), false, None);
        let value = serde_json::json!({
            "type": "event",
            "subscription": "trades(BTC)",
            "data": {
                "channel": "trades",
                "data": [{ "coin": "BTC", "price": "50000" }]
            }
        });

        let projected = apply_json_stream_options(value, &options);

        assert_eq!(projected["data"]["channel"], "trades");
        assert!(projected["data"]["data"].is_array());
    }

    #[test]
    fn test_results_only_strips_common_envelope_keys() {
        let value = serde_json::json!({
            "data": [{"coin": "BTC"}],
            "metadata": {"duration_ms": 12}
        });

        let stripped = strip_results_envelope(value);

        assert_eq!(stripped, serde_json::json!([{"coin": "BTC"}]));
    }

    #[test]
    fn test_render_dispatches_correctly() {
        let data = sample_data();

        let pretty = render(&data, OutputFormat::Pretty);
        let table = render(&data, OutputFormat::Table);
        let json = render(&data, OutputFormat::Json);

        // Pretty should have ANSI colors
        assert!(pretty.contains(colors::CYAN));
        // Table should have border chars
        assert!(table.contains('│') || table.contains('─'));
        // JSON should be valid
        serde_json::from_str::<serde_json::Value>(&json).unwrap();
    }

    #[test]
    fn test_format_timing() {
        let duration = std::time::Duration::from_millis(1234);
        let timing = format_timing(duration);
        assert!(timing.contains("Completed in"));
        assert!(timing.contains("1.23s"));
    }

    #[test]
    fn test_format_timing_zero() {
        let duration = std::time::Duration::from_millis(0);
        let timing = format_timing(duration);
        assert!(timing.contains("0.00s"));
    }

    #[test]
    fn test_format_timing_sub_second() {
        let duration = std::time::Duration::from_micros(500_000);
        let timing = format_timing(duration);
        assert!(timing.contains("0.50s"));
    }

    #[test]
    fn test_colors_cyan() {
        let colored = colors::cyan("test");
        assert!(colored.contains(colors::CYAN));
        assert!(colored.contains("test"));
        assert!(colored.contains(colors::RESET));
    }

    #[test]
    fn test_colors_green() {
        let colored = colors::green("test");
        assert!(colored.contains(colors::GREEN));
    }

    #[test]
    fn test_colors_red() {
        let colored = colors::red("test");
        assert!(colored.contains(colors::RED));
    }

    #[test]
    fn test_colors_gray() {
        let colored = colors::gray("test");
        assert!(colored.contains(colors::GRAY));
    }

    #[test]
    fn test_colors_yellow() {
        let colored = colors::yellow("test");
        assert!(colored.contains(colors::YELLOW));
    }

    #[test]
    fn test_colors_bold() {
        let colored = colors::bold("test");
        assert!(colored.contains(colors::BOLD));
    }

    #[test]
    fn test_colors_pnl_positive() {
        let colored = colors::pnl("+100.50", true);
        assert!(colored.contains(colors::GREEN));
        assert!(colored.contains("+100.50"));
    }

    #[test]
    fn test_colors_pnl_negative() {
        let colored = colors::pnl("-50.25", false);
        assert!(colored.contains(colors::RED));
        assert!(colored.contains("-50.25"));
    }

    #[test]
    fn test_empty_data() {
        let data = TestData {
            headers: vec!["Name".to_string()],
            rows: vec![],
            json: serde_json::json!([]),
        };

        let pretty = render_pretty(&data);
        assert!(pretty.contains("Name")); // header exists

        let table = render_table(&data);
        assert!(table.contains("Name")); // header exists

        let json = render_json(&data);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_single_row_data() {
        let data = TestData {
            headers: vec!["Coin".to_string()],
            rows: vec![vec!["BTC".to_string()]],
            json: serde_json::json!([{"coin": "BTC"}]),
        };

        let pretty = render_pretty(&data);
        assert!(pretty.contains("BTC"));
        assert!(pretty.contains("Coin"));

        let json = render_json(&data);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 1);
    }
}
