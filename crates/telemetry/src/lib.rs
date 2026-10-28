// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! # Okoala Telemetry
//!
//! Logs and metrics for every process (implementation plan §8.5). An L1 crate: it knows nothing of protocols
//! or domains, only how events and numbers leave the process.
//!
//! - **Logs**: [`init`] installs a `tracing` subscriber that writes one JSON object per line to stdout, filtered
//!   by the `[telemetry]` section's `filter` ([`TelemetrySection`]).
//! - **Metrics**: [`Metrics`] is the process's Prometheus registry, with build information and uptime
//!   registered, exposed in the text format by the internal listener's `/metrics` (§4.20).
//!
//! ## Spans and fields
//!
//! The convention every crate follows, so that logs can be searched across roles:
//!
//! - One span per connection and one per request (or command, or job), named after the unit of work in
//!   `snake_case` (`imap_connection`, `jmap_request`, `job`). Events inside a span inherit its fields; do not
//!   repeat them on each event.
//! - Field names are `snake_case` and mean the same everywhere: `role`, `listener`, `peer` (remote socket
//!   address), `tenant_id`, `account_id`, `session_id`, `request_id`, `job_kind`, `job_id`, `method`, `path`
//!   (the URL path only, never the query string), `status`, `error`, `elapsed_ms`.
//! - Record ids, never content: no message bodies, subjects, addresses beyond what an id stands for, search
//!   terms or calendar data.
//! - Messages are short lowercase sentences without trailing punctuation (`connection closed`); the
//!   variable parts go in fields, not in the message.
//! - Levels: `error` needs an operator, `warn` is unexpected but handled, `info` is a lifecycle event (start,
//!   stop, listener bound, leader change), `debug` is per request, `trace` is per protocol step.
//!
//! ## Secrets
//!
//! Credentials never reach a log (invariant I10, checked by the log-scrubbing test). An
//! [`okoala_secret::Secret`] (or a struct holding one) may be recorded with `?` (`tracing::debug!(config =
//! ?section)`), because its `Debug` prints `[REDACTED]`. The value returned by `Secret::reveal_value()` must
//! never be logged, formatted or put in an error, and neither may request headers or bodies that carry
//! credentials (`Authorization`, `Cookie`, passwords in protocol commands): log that a credential was
//! rejected, not what it was.

mod logs;
mod metrics;

pub use logs::{TelemetryError, TelemetrySection, config, init, install_panic_hook, subscriber};
pub use metrics::{CONTENT_TYPE, Metrics};
