//! The output session: how this run prints, decided once at startup.
//!
//! One value carries the resolved format, the JSON projection flags, the
//! terminal facts, and the start instant that the "Completed in" footer
//! measures from. Nothing here is process-global, so renderers called from tests
//! or from a second session never see each other's options.

use std::time::{Duration, Instant};

use super::terminal::OutputEnv;
use super::{JsonRenderOptions, OutputFormat, Rendered, TableData, colors, format, render_outcome};

#[derive(Debug, Clone)]
pub struct Session {
    format: OutputFormat,
    json: JsonRenderOptions,
    env: OutputEnv,
    started: Instant,
}

impl Session {
    /// A session that starts timing now.
    #[must_use]
    pub fn new(format: OutputFormat, json: JsonRenderOptions, env: OutputEnv) -> Self {
        Self {
            format,
            json,
            env,
            started: Instant::now(),
        }
    }

    /// A non-terminal session with no projection, for rendering assertions.
    #[cfg(test)]
    pub(crate) fn for_test(format: OutputFormat) -> Self {
        Self::new(format, JsonRenderOptions::default(), OutputEnv::default())
    }

    #[must_use]
    pub fn format(&self) -> OutputFormat {
        self.format
    }

    #[must_use]
    pub fn json_options(&self) -> &JsonRenderOptions {
        &self.json
    }

    #[must_use]
    pub fn env(&self) -> OutputEnv {
        self.env
    }

    /// The same session printing in `format`, for output that is always one format.
    #[must_use]
    pub fn with_format(&self, format: OutputFormat) -> Self {
        Self {
            format,
            ..self.clone()
        }
    }

    /// `data` as it would print to stdout in this session's format.
    #[must_use]
    pub fn render(&self, data: &dyn TableData) -> String {
        self.render_as(data, self.format)
    }

    /// `data` as it would print to stdout in `format`, with this session's projection.
    #[must_use]
    pub fn render_as(&self, data: &dyn TableData, format: OutputFormat) -> String {
        render_outcome(data, format, &self.json, self.env.stdout_tty).text
    }

    /// `data` as one compact JSON line with this session's projection, for NDJSON streams.
    pub fn render_json_line(&self, data: &dyn TableData) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.json.apply(data.to_json_value()))
    }

    /// Print the answer to stdout (empty-state and truncation notices to stderr),
    /// then the "Completed in" footer when the run is interactive.
    pub fn print(&self, data: &dyn TableData) {
        self.print_untimed(data);
        if let Some(line) = timing_footer(self.started.elapsed(), self.format, self.env) {
            eprintln!("{line}");
        }
    }

    /// Print the answer without the timing footer.
    pub fn print_untimed(&self, data: &dyn TableData) {
        print_rendered(&render_outcome(
            data,
            self.format,
            &self.json,
            self.env.stdout_tty,
        ));
    }
}

/// Write rendered output: the answer to stdout, notices to stderr.
fn print_rendered(rendered: &Rendered) {
    if !rendered.text.is_empty() {
        println!("{}", rendered.text);
    }
    for notice in &rendered.notices {
        eprintln!("{notice}");
    }
}

/// The timing footer for a finished command, or `None` when it must stay silent:
/// JSON output, agent or CI runs, and any stderr that is not a terminal.
fn timing_footer(duration: Duration, format: OutputFormat, env: OutputEnv) -> Option<String> {
    if format == OutputFormat::Json || !env.stderr_tty || env.agent || env.ci {
        return None;
    }
    Some(colors::stderr::gray(&format_timing(duration)))
}

fn format_timing(duration: Duration) -> String {
    format!("Completed in {}", format::duration(duration))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tty_env() -> OutputEnv {
        OutputEnv {
            stdout_tty: true,
            stderr_tty: true,
            ..OutputEnv::default()
        }
    }

    #[test]
    fn format_timing_is_compact() {
        assert_eq!(
            format_timing(Duration::from_millis(1234)),
            "Completed in 1.2s"
        );
        assert_eq!(
            format_timing(Duration::from_millis(42)),
            "Completed in 42ms"
        );
    }

    #[test]
    fn timing_footer_only_for_human_formats_on_a_terminal() {
        let duration = Duration::from_millis(1234);
        assert!(timing_footer(duration, OutputFormat::Json, tty_env()).is_none());
        assert!(timing_footer(duration, OutputFormat::Pretty, OutputEnv::default()).is_none());
        assert!(
            timing_footer(
                duration,
                OutputFormat::Pretty,
                OutputEnv {
                    agent: true,
                    ..tty_env()
                }
            )
            .is_none()
        );
        assert!(
            timing_footer(
                duration,
                OutputFormat::Table,
                OutputEnv {
                    ci: true,
                    ..tty_env()
                }
            )
            .is_none()
        );
        assert_eq!(
            timing_footer(duration, OutputFormat::Pretty, tty_env()).as_deref(),
            Some("Completed in 1.2s")
        );
    }

    #[test]
    fn sessions_do_not_share_projection() {
        struct Rows;
        impl TableData for Rows {
            fn headers(&self) -> Vec<&str> {
                vec!["Name"]
            }
            fn rows(&self) -> Vec<Vec<String>> {
                vec![vec!["BTC".to_string()]]
            }
            fn to_json_value(&self) -> serde_json::Value {
                serde_json::json!({"name": "BTC", "price": "1"})
            }
        }

        let selecting = Session::new(
            OutputFormat::Json,
            JsonRenderOptions::from_cli(Some("name"), false, None),
            OutputEnv::default(),
        );
        let plain = Session::new(
            OutputFormat::Json,
            JsonRenderOptions::default(),
            OutputEnv::default(),
        );
        assert_eq!(selecting.render(&Rows), r#"{"name":"BTC"}"#);
        assert_eq!(plain.render(&Rows), r#"{"name":"BTC","price":"1"}"#);
    }
}
