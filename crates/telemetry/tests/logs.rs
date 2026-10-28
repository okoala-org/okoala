// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! The JSON log subscriber: one object per line, filtering, and redaction of `Secret` fields.

#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::sync::{Arc, Mutex};

use okoala_secret::Secret;
use okoala_telemetry::{TelemetrySection, subscriber};
use tracing_subscriber::fmt::MakeWriter;

const TEST_SECRET: &str = "telemetry-Secret-Never-Logged";

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Write for Buffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'w> MakeWriter<'w> for Buffer {
    type Writer = Buffer;
    fn make_writer(&'w self) -> Self::Writer {
        self.clone()
    }
}

impl Buffer {
    fn lines(&self) -> Vec<serde_json::Value> {
        let text = String::from_utf8(self.0.lock().unwrap().clone()).unwrap();
        text.lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

fn capture(filter: &str, f: impl FnOnce()) -> Vec<serde_json::Value> {
    let buffer = Buffer::default();
    let section = TelemetrySection {
        filter: filter.to_owned(),
    };
    let subscriber = subscriber(&section, buffer.clone()).unwrap();
    tracing::subscriber::with_default(subscriber, f);
    buffer.lines()
}

#[test]
fn events_are_json_objects_with_flattened_fields_and_span() {
    let lines = capture("info", || {
        let span = tracing::info_span!("jmap_request", request_id = 7);
        let _entered = span.enter();
        tracing::info!(status = 200, "request done");
    });
    assert_eq!(lines.len(), 1);
    let event = &lines[0];
    assert_eq!(event["level"], "INFO");
    assert_eq!(event["message"], "request done");
    assert_eq!(event["status"], 200);
    assert_eq!(event["span"]["name"], "jmap_request");
    assert_eq!(event["span"]["request_id"], 7);
    assert!(event["timestamp"].is_string());
    assert_eq!(event["target"], "logs");
}

#[test]
fn the_filter_selects_levels_per_target() {
    let lines = capture("warn,logs=debug", || {
        tracing::debug!("kept");
        tracing::trace!("dropped");
        tracing::debug!(target: "other", "dropped");
        tracing::warn!(target: "other", "kept");
    });
    let messages: Vec<_> = lines.iter().map(|l| l["message"].clone()).collect();
    assert_eq!(messages, ["kept", "kept"]);
}

#[derive(Debug)]
#[allow(dead_code)]
struct Section {
    listen: &'static str,
    token: Secret<String>,
}

#[test]
fn secrets_recorded_with_debug_are_redacted() {
    let section = Section {
        listen: "127.0.0.1:9090",
        token: Secret::new(TEST_SECRET.to_owned()),
    };
    let lines = capture("debug", || {
        tracing::debug!(config = ?section, token = ?section.token, "loaded");
    });
    let text = serde_json::to_string(&lines).unwrap();
    assert!(text.contains("[REDACTED]"), "{text}");
    assert!(text.contains("127.0.0.1:9090"), "{text}");
    assert!(!text.contains(TEST_SECRET), "{text}");
}

#[test]
fn the_section_defaults_to_info_and_rejects_bad_filters() {
    assert_eq!(TelemetrySection::default().filter, "info");
    assert!(
        subscriber(
            &TelemetrySection {
                filter: "x=loud".to_owned()
            },
            Buffer::default()
        )
        .is_err()
    );
}
