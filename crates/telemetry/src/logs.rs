// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! The `[telemetry]` section and the JSON log subscriber.

use std::fmt;

use okoala_config::{Section, SectionDescriptor};
use serde::{Deserialize, Deserializer};
use tracing::Subscriber;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

/// `[telemetry]`: logging.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelemetrySection {
    /// Which events are logged, in `RUST_LOG` syntax: a default level and per-crate levels, such as
    /// `info,okoala_imap=debug`. Default `info`. Set it from the environment with `OKOALA_TELEMETRY__FILTER`;
    /// `RUST_LOG` itself is not read, so the configuration stays the one source.
    #[serde(default = "default_filter", deserialize_with = "filter")]
    pub filter: String,
}

impl Default for TelemetrySection {
    fn default() -> Self {
        Self {
            filter: default_filter(),
        }
    }
}

fn default_filter() -> String {
    "info".to_owned()
}

/// Accepts only a filter that parses, so a bad one is reported with the other configuration errors.
fn filter<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let text = String::deserialize(deserializer)?;
    parse_filter(&text).map_err(|_| serde::de::Error::custom("invalid log filter"))?;
    Ok(text)
}

fn parse_filter(text: &str) -> Result<EnvFilter, TelemetryError> {
    EnvFilter::builder()
        .with_regex(false)
        .parse(text)
        .map_err(|_| TelemetryError::InvalidFilter)
}

impl Section for TelemetrySection {
    const NAME: &'static str = "telemetry";
}

/// The descriptors of this crate's configuration sections.
pub fn config() -> Vec<SectionDescriptor> {
    vec![SectionDescriptor::of::<TelemetrySection>()]
}

/// Why logging could not be set up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelemetryError {
    /// The `filter` does not parse. The value is not repeated.
    InvalidFilter,
    /// A global subscriber is already installed.
    AlreadyInitialized,
}

impl fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFilter => f.write_str("[telemetry] filter: invalid log filter"),
            Self::AlreadyInitialized => f.write_str("logging is already initialized"),
        }
    }
}

impl std::error::Error for TelemetryError {}

/// The JSON subscriber for `section`, writing to `writer`: one object per line, with `timestamp`, `level`,
/// `target`, the event's fields at the top level, and the current span's fields under `span`. No ANSI colours.
///
/// [`init`] installs it globally with stdout; tests install it with a buffer through
/// `tracing::subscriber::with_default`.
pub fn subscriber<W>(
    section: &TelemetrySection,
    writer: W,
) -> Result<impl Subscriber + Send + Sync + 'static, TelemetryError>
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    let filter = parse_filter(&section.filter)?;
    Ok(tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_current_span(true)
        .with_span_list(false)
        .with_target(true)
        .with_ansi(false)
        .with_env_filter(filter)
        .with_writer(writer)
        .finish())
}

/// Installs the JSON subscriber for `section` as the process's global subscriber, writing to stdout.
pub fn init(section: &TelemetrySection) -> Result<(), TelemetryError> {
    let subscriber = subscriber(section, std::io::stdout)?;
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|_| TelemetryError::AlreadyInitialized)
}

/// Replaces the default panic hook with one that logs the panic as an `error` event (message and location), so
/// a panic in any task shows up in the JSON log stream rather than as plain text on stderr. Call it after
/// [`init`].
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("(non-string panic payload)");
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let thread = std::thread::current();
        tracing::error!(
            panic.message = message,
            panic.location = location,
            thread = thread.name().unwrap_or("(unnamed)"),
            "panic"
        );
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_parse_like_rust_log() {
        assert!(parse_filter("info").is_ok());
        assert!(parse_filter("warn,okoala_imap=debug").is_ok());
        assert!(matches!(
            parse_filter("okoala_imap=verbose"),
            Err(TelemetryError::InvalidFilter)
        ));
    }
}
