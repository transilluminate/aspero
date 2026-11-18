//! aspero's diagnostics in Apple's unified log ([`enable_system_log`]):
//! the events DESIGN "Diagnostics events" lists, beside the embedder's
//! own, in Console, `log stream` and Instruments.
//!
//! A `tracing` subscriber of its own ([`SystemLog`]) rather than
//! `tracing-oslog`, which logs every message `%{public}s`: here events at
//! `debug` and below, which carry what was asked (queries, titles,
//! identifiers, URLs), are logged `%{private}s`, so the system redacts
//! them outside a debug session even when debug is on; info and above,
//! counts, codes and times only, are public. Each line is the event's
//! name, then its fields as `key=value` in order (a value with spaces or
//! quotes quoted). Categories are by area, from the event's target:
//! `net.<provider>` (or `net` without a `provider` field), `cache`,
//! `budget`, `engine`, `mcp`, and `aspero` for the rest. Only aspero's
//! events are logged, never spans.

// Off Apple platforms only the tests use the subscriber.
#![cfg_attr(not(target_vendor = "apple"), allow(dead_code))]

use std::fmt::{self, Write as _};

use tracing::field::{Field, Visit};
use tracing::level_filters::LevelFilter;
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Level, Metadata, Subscriber};

#[cfg(target_vendor = "apple")]
mod oslog;

/// How much of aspero's diagnostics reaches the system log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LogLevel {
    /// Events at info and above: a `request` per provider request, a
    /// `call` per engine call, a `tool` per MCP tool call, and warnings.
    /// Counts, codes and times: logged public.
    Info,
    /// Info's, and their companions at debug with what was asked (URLs,
    /// queries, identifiers): logged private, redacted outside a debug
    /// session.
    Debug,
}

impl LogLevel {
    fn filter(self) -> LevelFilter {
        match self {
            LogLevel::Info => LevelFilter::INFO,
            LogLevel::Debug => LevelFilter::DEBUG,
        }
    }
}

/// Sends aspero's diagnostics to Apple's unified log under `subsystem`
/// (the embedder's, e.g. `com.example.app`), at `level` and above, from
/// now until the process ends: what DESIGN "Diagnostics events" lists, by
/// category (`net.<provider>`, `cache`, `budget`, `engine`, `mcp`). It
/// installs the process's global `tracing` subscriber, so it works once:
/// false if one is already installed (by an earlier call or anything
/// else), if `subsystem` is empty or holds a NUL, and on platforms other
/// than Apple's. Never panics.
#[uniffi::export]
// UniFFI passes strings owned.
#[allow(clippy::needless_pass_by_value)]
pub fn enable_system_log(subsystem: String, level: LogLevel) -> bool {
    #[cfg(target_vendor = "apple")]
    {
        let Some(sink) = oslog::OsLog::new(&subsystem) else {
            return false;
        };
        tracing::subscriber::set_global_default(SystemLog::new(sink, level.filter())).is_ok()
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        let _ = (subsystem, level);
        false
    }
}

/// Where formatted lines go: the unified log, or a test's list.
pub(crate) trait Sink: Send + Sync + 'static {
    /// Whether a line at `level` in `category` would be kept, so one that
    /// wouldn't isn't formatted.
    fn enabled(&self, category: &str, level: Level) -> bool;

    /// Writes one event's line; `private` lines are redacted by the system
    /// outside a debug session.
    fn write(&self, category: &str, level: Level, private: bool, line: &str);
}

/// The subscriber: aspero's events at `max` and above, as lines to `sink`.
pub(crate) struct SystemLog<S> {
    sink: S,
    max: LevelFilter,
}

impl<S: Sink> SystemLog<S> {
    pub(crate) fn new(sink: S, max: LevelFilter) -> Self {
        SystemLog { sink, max }
    }

    /// Our events at `max` and above, and `tracing::enabled!`'s hints
    /// for them (a hint isn't an event: refused, the engine's
    /// `request_url` was never built); never spans.
    fn wants(&self, meta: &Metadata<'_>) -> bool {
        !meta.is_span() && meta.target().starts_with("aspero") && *meta.level() <= self.max
    }
}

/// An event's category, by its target (and provider, for the network's).
fn category<'a>(target: &str, provider: Option<&'a str>) -> std::borrow::Cow<'a, str> {
    let area = target.strip_prefix("aspero::").unwrap_or("");
    let area = area.split("::").next().unwrap_or("");
    match area {
        "net" => provider.map_or("net".into(), |p| format!("net.{p}").into()),
        "cache" => "cache".into(),
        "budget" => "budget".into(),
        "engine" => "engine".into(),
        "mcp" => "mcp".into(),
        _ => "aspero".into(),
    }
}

/// An event's `provider` field, for its category: read before the line
/// is formatted, so a line the log won't keep isn't.
#[derive(Default)]
struct Provider(Option<String>);

impl Visit for Provider {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "provider" {
            self.0 = Some(format!("{value:?}"));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "provider" {
            self.0 = Some(value.to_owned());
        }
    }
}

/// An event as a line: its name, then `key=value` fields in order.
#[derive(Default)]
struct Line {
    name: String,
    fields: String,
}

impl Line {
    fn put(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            value.clone_into(&mut self.name);
            return;
        }
        if !self.fields.is_empty() {
            self.fields.push(' ');
        }
        let plain = !value.is_empty()
            && !value
                .chars()
                .any(|c| c.is_whitespace() || c == '"' || c == '=' || c.is_control());
        if plain {
            let _ = write!(self.fields, "{}={value}", field.name());
        } else {
            let _ = write!(self.fields, "{}={value:?}", field.name());
        }
    }

    fn text(&self) -> String {
        match (self.name.is_empty(), self.fields.is_empty()) {
            (_, true) => self.name.clone(),
            (true, false) => self.fields.clone(),
            (false, false) => format!("{} {}", self.name, self.fields),
        }
    }
}

impl Visit for Line {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, value);
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, if value { "true" } else { "false" });
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, &value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, &value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field, &value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.put(field, &format!("{value:?}"));
    }
}

impl<S: Sink> Subscriber for SystemLog<S> {
    fn register_callsite(&self, meta: &'static Metadata<'static>) -> Interest {
        if self.wants(meta) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        self.wants(meta)
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(self.max)
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        // Never asked for: no span is enabled.
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let meta = event.metadata();
        let level = *meta.level();
        let mut provider = Provider::default();
        event.record(&mut provider);
        let category = category(meta.target(), provider.0.as_deref());
        if !self.sink.enabled(&category, level) {
            return;
        }
        let mut line = Line::default();
        event.record(&mut line);
        self.sink
            .write(&category, level, level > Level::INFO, &line.text());
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    type Written = Arc<Mutex<Vec<(String, Level, bool, String)>>>;

    #[derive(Default, Clone)]
    struct Lines(Written);

    impl Sink for Lines {
        fn enabled(&self, _: &str, _: Level) -> bool {
            true
        }

        fn write(&self, category: &str, level: Level, private: bool, line: &str) {
            self.0
                .lock()
                .unwrap()
                .push((category.to_owned(), level, private, line.to_owned()));
        }
    }

    fn logged(max: LevelFilter, f: impl FnOnce()) -> Vec<(String, Level, bool, String)> {
        let lines = Lines::default();
        tracing::subscriber::with_default(SystemLog::new(lines.clone(), max), f);
        lines.0.lock().unwrap().clone()
    }

    #[test]
    fn requests_go_to_their_providers_category_as_public_lines() {
        let written = logged(LevelFilter::INFO, || {
            tracing::info!(
                target: "aspero::net",
                provider = %"openalex", op = "search", status = 200u16, ms = 12u64,
                cost_usd = %"0.001000", "request"
            );
        });
        assert_eq!(
            written,
            [(
                "net.openalex".to_owned(),
                Level::INFO,
                false,
                "request provider=openalex op=search status=200 ms=12 cost_usd=0.001000".to_owned()
            )]
        );
    }

    #[test]
    fn debug_companions_are_private_and_only_at_debug() {
        let emit = || {
            tracing::debug!(
                target: "aspero::net",
                provider = %"crossref", url = %"/works?query=sepsis lactate", "request_url"
            );
        };
        assert!(logged(LevelFilter::INFO, emit).is_empty());
        let written = logged(LevelFilter::DEBUG, emit);
        assert_eq!(written.len(), 1);
        let (category, _, private, line) = &written[0];
        assert_eq!(category, "net.crossref");
        assert!(private);
        assert_eq!(
            line,
            r#"request_url provider=crossref url="/works?query=sepsis lactate""#
        );
    }

    #[test]
    fn a_debug_hint_is_enabled_only_at_debug() {
        // The engine builds a request's URL for `request_url` only when
        // `enabled!` says debug is on.
        let hint = || {
            let on = tracing::enabled!(target: "aspero::net", Level::DEBUG);
            tracing::info!(target: "aspero::net", on, "hint");
        };
        let said = |max| logged(max, hint)[0].3.clone();
        assert_eq!(said(LevelFilter::DEBUG), "hint on=true");
        assert_eq!(said(LevelFilter::INFO), "hint on=false");
    }

    #[test]
    fn areas_have_categories_and_other_crates_are_left_out() {
        let written = logged(LevelFilter::DEBUG, || {
            tracing::info!(target: "aspero::engine", op = "verify", "call");
            tracing::info!(target: "aspero::cache", op = "lookup", "request");
            tracing::info!(target: "aspero::mcp", tool = "search", key = %"Claude Code", "tool");
            tracing::warn!(target: "aspero::budget", "couldn't ask the budget");
            tracing::info!(target: "aspero::net", "retry");
            tracing::warn!(target: "aspero_crossref", "elsewhere");
            tracing::info!(target: "hyper", "not ours");
        });
        let categories: Vec<&str> = written.iter().map(|(c, ..)| c.as_str()).collect();
        assert_eq!(
            categories,
            ["engine", "cache", "mcp", "budget", "net", "aspero"]
        );
        assert_eq!(written[2].3, r#"tool tool=search key="Claude Code""#);
        assert!(written.iter().all(|(_, _, private, _)| !private));
    }
}
