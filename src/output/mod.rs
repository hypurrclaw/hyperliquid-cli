//! Output formatting system for Hyperliquid CLI.
//!
//! Supports three output formats:
//! - `Pretty`: human-readable, tabwriter-aligned, styled only when stdout styling is enabled (default)
//! - `Table`: bordered tables on a terminal, tab-separated header and rows on a pipe
//! - `Json`: stable JSON output with snake_case keys, indented on a terminal and compact on a pipe
//!
//! Color theme (see [`colors`] for when styling applies):
//! - Cyan: headers
//! - Green: positive values (profit, gains)
//! - Red: negative values (loss, declines)
//! - Gray: muted/secondary text
//! - Yellow: warnings
//!
//! Streams:
//! - stdout carries the answer. Empty-state sentences and `--max-results`
//!   truncation notices go to stderr in pretty and table modes.
//! - Errors are rendered by [`crate::errors`], which depends on this module.
//!
//! Timing feedback:
//! - "Completed in 1.2s" is printed to stderr only for human formats on a terminal

use clap::ValueEnum;
use serde_json::Value;
use std::fmt;
use std::io::Write;
use tabwriter::TabWriter;

mod session;

pub use session::Session;

pub mod colors;
pub mod format;
pub mod terminal;

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

/// `--select`, `--results-only` and `--max-results`, applied to JSON output.
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

    /// The `--select` field names, lowercased, when `--select` was given.
    #[must_use]
    pub fn selected_fields(&self) -> Option<&[String]> {
        self.selected_fields.as_deref()
    }

    /// Whether any projection flag was given.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.selected_fields.is_some() || self.results_only || self.max_results.is_some()
    }

    /// Project one JSON document.
    #[must_use]
    pub fn apply(&self, value: serde_json::Value) -> serde_json::Value {
        let value = if self.results_only {
            strip_results_envelope(value)
        } else {
            value
        };

        let value = if let Some(max_results) = self.max_results {
            limit_json_results(value, max_results)
        } else {
            value
        };

        if let Some(fields) = self.selected_fields.as_deref() {
            project_selected_fields(value, fields)
        } else {
            value
        }
    }

    /// Project one JSONL stream envelope.
    ///
    /// Stream envelopes keep their top-level `type`, `subscription`, and `data`
    /// keys unless the caller explicitly selects top-level fields. `--max-results`
    /// is therefore applied recursively to nested payloads instead of truncating
    /// the envelope itself.
    #[must_use]
    pub fn apply_stream(&self, value: serde_json::Value) -> serde_json::Value {
        let value = if self.results_only {
            strip_results_envelope(value)
        } else {
            value
        };
        let value = if let Some(max_results) = self.max_results {
            limit_json_stream_results(value, max_results)
        } else {
            value
        };

        if let Some(fields) = self.selected_fields.as_deref() {
            project_stream_selected_fields(value, fields)
        } else {
            value
        }
    }
}

fn parse_selected_fields(select: &str) -> Vec<String> {
    select
        .split(',')
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(|field| field.to_ascii_lowercase())
        .collect()
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

    /// Sentence for stderr when [`Self::rows`] is empty, for example `No orders.`.
    ///
    /// Pretty and table print only the header (on a pipe) or nothing (on a
    /// terminal) to stdout and this message to stderr. Implementors with an
    /// empty state should return no rows and set this instead of a placeholder
    /// row. Defaults to `No results.`.
    fn empty_message(&self) -> Option<&str> {
        None
    }
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
    empty_message: &'static str,
}

impl JsonValueOutput {
    /// `empty_message` is the sentence printed to stderr in pretty and table
    /// modes when the response has no rows, for example `No orders.`.
    #[must_use]
    pub fn new(value: Value, empty_message: &'static str) -> Self {
        let value = snake_case_json_keys(value);
        let (headers, rows) = json_value_table_rows(&value);
        Self {
            headers,
            rows,
            value,
            empty_message,
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

    fn empty_message(&self) -> Option<&str> {
        Some(self.empty_message)
    }

    fn to_json_value(&self) -> serde_json::Value {
        self.value.clone()
    }
}

fn snake_case_json_keys(value: Value) -> Value {
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

fn json_value_table_rows(value: &Value) -> (Vec<String>, Vec<Vec<String>>) {
    match value {
        Value::Array(values) if values.is_empty() => (vec!["Value".to_string()], Vec::new()),
        Value::Array(values) => array_table_rows(values),
        Value::Object(object) if object.is_empty() => {
            (vec!["Field".to_string(), "Value".to_string()], Vec::new())
        }
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

// ── Cell sanitization ───────────────────────────────────────────────────

/// Characters that render as nothing or reorder text without being C0/C1
/// controls: soft hyphen, bidi marks and overrides, zero-width characters,
/// line and paragraph separators, BOM, and interlinear annotation marks.
fn is_invisible_format_char(ch: char) -> bool {
    matches!(
        u32::from(ch),
        0x00AD
            | 0x061C
            | 0x180E
            | 0x200B..=0x200F
            | 0x2028..=0x202E
            | 0x2060..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
    )
}

/// Length of a style sequence this module's color helpers emit at the start of `text`.
fn known_style_len(text: &str) -> Option<usize> {
    [
        colors::CYAN,
        colors::GREEN,
        colors::RED,
        colors::GRAY,
        colors::YELLOW,
        colors::BOLD,
        colors::RESET,
    ]
    .iter()
    .find(|code| text.starts_with(**code))
    .map(|code| code.len())
}

/// Make a table cell safe to print: control, bidi, zero-width and other
/// invisible format characters become visible escapes (`\n`, `\t`, `\x1b`,
/// `\u{202e}`). With `keep_styles`, the exact color sequences from [`colors`]
/// pass through so styled cells keep their theme.
fn sanitize_cell(cell: &str, keep_styles: bool) -> String {
    let mut out = String::with_capacity(cell.len());
    let mut rest = cell;
    while let Some(ch) = rest.chars().next() {
        if keep_styles
            && ch == '\x1b'
            && let Some(len) = known_style_len(rest)
        {
            out.push_str(&rest[..len]);
            rest = &rest[len..];
            continue;
        }
        match ch {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            ch if ch.is_control() => out.push_str(&format!("\\x{:02x}", u32::from(ch))),
            ch if is_invisible_format_char(ch) => {
                out.push_str(&format!("\\u{{{:x}}}", u32::from(ch)));
            }
            ch => out.push(ch),
        }
        rest = &rest[ch.len_utf8()..];
    }
    out
}

// ── Rendering functions ─────────────────────────────────────────────────

/// Text for stdout plus notices that belong on stderr.
struct Rendered {
    text: String,
    notices: Vec<String>,
}

/// Notices for human formats: the empty-state sentence, or how many rows
/// `--max-results` clipped.
fn row_notices(data: &dyn TableData, total: usize, shown: usize) -> Vec<String> {
    if total == 0 {
        return vec![data.empty_message().unwrap_or("No results.").to_string()];
    }
    if shown < total {
        return vec![format!(
            "Showing {} of {}. Remove --max-results to see all.",
            format::thousands(shown as u64),
            format::thousands(total as u64)
        )];
    }
    Vec::new()
}

fn limit_rows(rows: Vec<Vec<String>>, max_results: Option<usize>) -> (usize, Vec<Vec<String>>) {
    let total = rows.len();
    let rows = match max_results {
        Some(max) => rows.into_iter().take(max).collect(),
        None => rows,
    };
    (total, rows)
}

fn render_pretty_inner(data: &dyn TableData, max_results: Option<usize>, tty: bool) -> Rendered {
    let keep_styles = colors::stdout_enabled();
    let headers = data
        .headers()
        .into_iter()
        .map(|header| sanitize_cell(header, false))
        .collect::<Vec<_>>();
    let (total, rows) = limit_rows(data.pretty_rows(), max_results);
    let notices = row_notices(data, total, rows.len());

    // An empty result prints nothing on a terminal and only the real header on a pipe.
    if rows.is_empty() && tty {
        return Rendered {
            text: String::new(),
            notices,
        };
    }

    let mut tw = TabWriter::new(Vec::new());
    let header_line = headers
        .iter()
        .map(|header| colors::cyan(header))
        .collect::<Vec<_>>()
        .join("\t");
    writeln!(tw, "{header_line}").unwrap();

    if !rows.is_empty() {
        let sep_line = headers
            .iter()
            .map(|_| "─".repeat(12))
            .collect::<Vec<_>>()
            .join("\t");
        writeln!(tw, "{sep_line}").unwrap();
    }

    for row in &rows {
        let line = row
            .iter()
            .map(|cell| sanitize_cell(cell, keep_styles))
            .collect::<Vec<_>>()
            .join("\t");
        writeln!(tw, "{line}").unwrap();
    }

    tw.flush().unwrap();
    let output = String::from_utf8_lossy(&tw.into_inner().unwrap()).to_string();

    Rendered {
        text: output.trim_end().to_string(),
        notices,
    }
}

fn render_table_inner(data: &dyn TableData, max_results: Option<usize>, tty: bool) -> Rendered {
    let headers = data
        .headers()
        .into_iter()
        .map(|header| sanitize_cell(header, false))
        .collect::<Vec<_>>();
    let (total, rows) = limit_rows(data.rows(), max_results);
    let notices = row_notices(data, total, rows.len());
    let rows = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| sanitize_cell(cell, false))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    if !tty {
        let mut lines = Vec::with_capacity(rows.len() + 1);
        lines.push(headers.join("\t"));
        lines.extend(rows.iter().map(|row| row.join("\t")));
        return Rendered {
            text: lines.join("\n"),
            notices,
        };
    }

    if rows.is_empty() {
        return Rendered {
            text: String::new(),
            notices,
        };
    }

    use tabled::builder::Builder;
    use tabled::settings::Style;

    let mut builder = Builder::default();
    builder.push_record(headers);
    for row in rows {
        builder.push_record(row);
    }
    let table = builder.build().with(Style::rounded()).to_string();

    Rendered {
        text: table.trim_end().to_string(),
        notices,
    }
}

/// Serialize JSON for stdout: indented with 2 spaces on a terminal, compact
/// single-line otherwise.
fn json_string(value: &Value, tty: bool) -> String {
    let rendered = if tty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    };
    rendered.unwrap_or_else(|_| "[]".to_string())
}

fn render_outcome(
    data: &dyn TableData,
    format: OutputFormat,
    options: &JsonRenderOptions,
    tty: bool,
) -> Rendered {
    match format {
        OutputFormat::Pretty => render_pretty_inner(data, options.max_results(), tty),
        OutputFormat::Table => render_table_inner(data, options.max_results(), tty),
        OutputFormat::Json => Rendered {
            text: json_string(&options.apply(data.to_json_value()), tty),
            notices: Vec::new(),
        },
    }
}

/// A warning line for stderr: `! message` with a yellow `!` in rich output,
/// `Warning: message` otherwise.
fn warning_line(message: &str) -> String {
    if colors::stderr_enabled() {
        format!("{} {message}", colors::stderr::yellow("!"))
    } else {
        format!("Warning: {message}")
    }
}

/// Print a warning to stderr (all formats).
pub fn warn(message: &str) {
    eprintln!("{}", warning_line(message));
}

// ── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_env() -> terminal::OutputEnv {
        terminal::OutputEnv::default()
    }

    fn tty_env() -> terminal::OutputEnv {
        terminal::OutputEnv {
            stdout_tty: true,
            ..terminal::OutputEnv::default()
        }
    }

    fn render_with(data: &dyn TableData, format: OutputFormat, env: terminal::OutputEnv) -> String {
        Session::new(format, JsonRenderOptions::default(), env).render(data)
    }

    fn render(data: &dyn TableData, format: OutputFormat) -> String {
        render_with(data, format, plain_env())
    }

    fn render_pretty(data: &dyn TableData) -> String {
        render(data, OutputFormat::Pretty)
    }

    fn render_table(data: &dyn TableData) -> String {
        render(data, OutputFormat::Table)
    }

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
    fn test_render_pretty_has_ansi_colors_when_enabled() {
        colors::with_streams_for_test(true, false, || {
            let output = render_pretty(&sample_data());
            assert!(output.contains(colors::CYAN));
            assert!(output.contains(colors::RESET));
        });
    }

    #[test]
    fn test_render_pretty_has_separator() {
        let data = sample_data();
        let output = render_pretty(&data);
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
    fn test_render_table_has_borders_on_terminal() {
        let output = render_with(&sample_data(), OutputFormat::Table, tty_env());
        // Rounded table style uses these border characters
        assert!(output.contains('│'));
        assert!(output.contains('╭'));
        assert!(output.contains('╯'));
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
        let output = render(&data, OutputFormat::Json);
        let parsed: serde_json::Value =
            serde_json::from_str(&output).expect("Should be valid JSON");
        assert!(parsed.is_array());
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn test_render_json_has_correct_fields() {
        let data = sample_data();
        let output = render(&data, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let first = &parsed.as_array().unwrap()[0];
        assert!(first.get("name").is_some());
        assert!(first.get("price").is_some());
        assert!(first.get("change").is_some());
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

        let limited = options.apply_stream(value);

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

        let limited = options.apply_stream(value);

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

        let projected = options.apply_stream(value);

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

        let projected = options.apply_stream(value);

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

        let projected = options.apply_stream(value);

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

        colors::with_streams_for_test(true, false, || {
            let pretty = render_with(&data, OutputFormat::Pretty, tty_env());
            let table = render_with(&data, OutputFormat::Table, tty_env());
            let json = render_with(&data, OutputFormat::Json, tty_env());

            assert!(pretty.contains(colors::CYAN));
            assert!(table.contains('│') || table.contains('─'));
            assert!(json.contains('\n'));
            serde_json::from_str::<serde_json::Value>(&json).unwrap();
        });
    }

    #[test]
    fn test_pretty_is_plain_when_colors_are_off() {
        let output = render_pretty(&sample_data());
        assert!(!output.contains('\x1b'));
        assert!(output.contains("Name"));
    }

    #[test]
    fn test_table_on_pipe_is_tab_separated() {
        let output = render_table(&sample_data());
        assert_eq!(
            output,
            "Name\tPrice\tChange\nBTC\t50000.00\t+2.5%\nETH\t3000.00\t-1.2%"
        );
    }

    #[test]
    fn test_table_on_terminal_is_bordered() {
        let output = render_with(&sample_data(), OutputFormat::Table, tty_env());
        assert!(output.contains('╭'));
        assert!(output.contains("BTC"));
    }

    #[test]
    fn test_json_is_compact_on_pipe_and_indented_on_terminal() {
        let data = sample_data();
        let compact = render(&data, OutputFormat::Json);
        assert!(!compact.contains('\n'));
        let value: serde_json::Value = serde_json::from_str(&compact).unwrap();

        let pretty = render_with(&data, OutputFormat::Json, tty_env());
        assert!(pretty.contains("\n  "));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&pretty).unwrap(),
            value
        );
    }

    #[test]
    fn test_colors_helpers_style_when_enabled() {
        colors::with_streams_for_test(true, true, || {
            assert_eq!(colors::cyan("test"), colors::paint(colors::CYAN, "test"));
            assert_eq!(colors::green("test"), colors::paint(colors::GREEN, "test"));
            assert_eq!(colors::red("test"), colors::paint(colors::RED, "test"));
            assert_eq!(colors::gray("test"), colors::paint(colors::GRAY, "test"));
            assert_eq!(
                colors::yellow("test"),
                colors::paint(colors::YELLOW, "test")
            );
        });
    }

    #[test]
    fn test_warning_line_uses_glyph_when_rich() {
        assert_eq!(warning_line("careful"), "Warning: careful");
        colors::with_streams_for_test(false, true, || {
            assert_eq!(
                warning_line("careful"),
                format!("{} careful", colors::paint(colors::YELLOW, "!"))
            );
        });
    }

    fn empty_data() -> TestData {
        TestData {
            headers: vec!["Name".to_string(), "Price".to_string()],
            rows: vec![],
            json: serde_json::json!([]),
        }
    }

    #[test]
    fn test_empty_data_prints_only_header_on_pipe() {
        let data = empty_data();
        assert_eq!(render_table(&data), "Name\tPrice");
        let pretty = render_pretty(&data);
        assert!(pretty.contains("Name"));
        assert!(!pretty.contains('─'));

        let json = render(&data, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_empty_data_prints_nothing_on_terminal() {
        let data = empty_data();
        assert_eq!(render_with(&data, OutputFormat::Table, tty_env()), "");
        assert_eq!(render_with(&data, OutputFormat::Pretty, tty_env()), "");
    }

    #[test]
    fn test_empty_data_notice_goes_to_stderr_stream() {
        let options = JsonRenderOptions::default();
        let outcome = render_outcome(&empty_data(), OutputFormat::Table, &options, false);
        assert_eq!(outcome.notices, vec!["No results.".to_string()]);

        let output = JsonValueOutput::new(serde_json::json!([]), "No orders.");
        let outcome = render_outcome(&output, OutputFormat::Table, &options, false);
        assert_eq!(outcome.text, "Value");
        assert_eq!(outcome.notices, vec!["No orders.".to_string()]);

        let outcome = render_outcome(&output, OutputFormat::Json, &options, false);
        assert_eq!(outcome.text, "[]");
        assert!(outcome.notices.is_empty());
    }

    #[test]
    fn test_max_results_truncation_notice_for_human_formats_only() {
        let options = JsonRenderOptions::from_cli(None, false, Some(1));
        let data = sample_data();

        let table = render_outcome(&data, OutputFormat::Table, &options, false);
        assert_eq!(table.text, "Name\tPrice\tChange\nBTC\t50000.00\t+2.5%");
        assert_eq!(
            table.notices,
            vec!["Showing 1 of 2. Remove --max-results to see all.".to_string()]
        );
        let pretty = render_outcome(&data, OutputFormat::Pretty, &options, false);
        assert_eq!(pretty.notices, table.notices);
        let json = render_outcome(&data, OutputFormat::Json, &options, false);
        assert!(json.notices.is_empty());

        let unclipped = JsonRenderOptions::from_cli(None, false, Some(2));
        assert!(
            render_outcome(&data, OutputFormat::Table, &unclipped, false)
                .notices
                .is_empty()
        );
    }

    #[test]
    fn test_cells_escape_control_and_format_characters() {
        let data = TestData {
            headers: vec!["Name".to_string()],
            rows: vec![vec![
                "evil\x1b[31m\nFORGED\tROW\u{202e}REV\u{200b}".to_string(),
            ]],
            json: serde_json::json!([{"name": "x"}]),
        };
        let expected = "evil\\x1b[31m\\nFORGED\\tROW\\u{202e}REV\\u{200b}";

        let table = render_table(&data);
        assert_eq!(table.lines().count(), 2);
        assert!(table.contains(expected));

        let pretty = render_pretty(&data);
        assert!(pretty.contains(expected));
        assert_eq!(pretty.lines().count(), 3);

        assert!(render_with(&data, OutputFormat::Table, tty_env()).contains(expected));
    }

    #[test]
    fn test_pretty_keeps_theme_styles_but_escapes_other_escapes() {
        colors::with_streams_for_test(true, false, || {
            let cell = format!("{}\x1b[2J", colors::green("10"));
            assert_eq!(
                sanitize_cell(&cell, true),
                format!("{}\\x1b[2J", colors::paint(colors::GREEN, "10"))
            );
            assert_eq!(
                sanitize_cell(&colors::green("10"), false),
                "\\x1b[32m10\\x1b[0m"
            );
        });
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

        let json = render(&data, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 1);
    }
}
