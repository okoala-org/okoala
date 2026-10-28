// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! The process's metrics registry.

use std::fmt;
use std::sync::atomic::AtomicU64;
use std::sync::{Mutex, MutexGuard};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use prometheus_client::encoding::text::encode;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::info::Info;
use prometheus_client::registry::{Metric, Registry};

/// The `Content-Type` of [`Metrics::encode`]'s output (OpenMetrics text, which Prometheus scrapes).
pub const CONTENT_TYPE: &str = "application/openmetrics-text; version=1.0.0; charset=utf-8";

/// The process's metrics: a Prometheus registry shared by every crate of the process (it becomes the `Metrics`
/// field of the process `Context`, §4.2).
///
/// Crates create their metrics (counters, gauges, histograms, families of them, from
/// [`prometheus_client::metrics`]), keep a clone to update, and hand one to [`Metrics::register`] at startup.
/// Registered from the start:
///
/// - `okoala_build_info{version="..."} 1`;
/// - `process_start_time_seconds`, the Unix time the process started;
/// - `okoala_uptime_seconds`, seconds since the registry was created, updated at each [`Metrics::encode`].
///
/// Label values must come from a small fixed set (a route name, a status code, a job kind): never an id that
/// grows with users or messages, and never a credential.
pub struct Metrics {
    registry: Mutex<Registry>,
    started: Instant,
    uptime: Gauge<f64, AtomicU64>,
}

impl fmt::Debug for Metrics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Metrics").finish_non_exhaustive()
    }
}

impl Metrics {
    /// A registry with the build and process metrics, for a build of version `version`.
    pub fn new(version: &str) -> Self {
        let mut registry = Registry::default();
        registry.register(
            "okoala_build",
            "Build information",
            Info::new(vec![("version".to_owned(), version.to_owned())]),
        );
        let start: Gauge<f64, AtomicU64> = Gauge::default();
        start.set(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0.0, |d| d.as_secs_f64()),
        );
        registry.register(
            "process_start_time_seconds",
            "Start time of the process since the Unix epoch, in seconds",
            start,
        );
        let uptime: Gauge<f64, AtomicU64> = Gauge::default();
        registry.register(
            "okoala_uptime_seconds",
            "Time since the process started, in seconds",
            uptime.clone(),
        );
        Self {
            registry: Mutex::new(registry),
            started: Instant::now(),
            uptime,
        }
    }

    /// Registers `metric` under `name` (`snake_case`, prefixed `okoala_` by convention; a counter gets `_total`
    /// appended) with a one-line `help`. Keep a clone of `metric` to update it.
    pub fn register(&self, name: &str, help: &str, metric: impl Metric) {
        self.lock().register(name, help, metric);
    }

    /// Every registered metric in the OpenMetrics text format ([`CONTENT_TYPE`]), ending with `# EOF`.
    pub fn encode(&self) -> Result<String, fmt::Error> {
        self.uptime.set(self.started.elapsed().as_secs_f64());
        let mut text = String::new();
        encode(&mut text, &self.lock())?;
        Ok(text)
    }

    fn lock(&self) -> MutexGuard<'_, Registry> {
        // A panic while the lock was held cannot leave the registry half-updated in a way that matters for
        // exposition, so a poisoned lock is used as it is.
        self.registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use prometheus_client::metrics::counter::Counter;

    use super::*;

    #[test]
    fn exposes_build_and_process_metrics() {
        let metrics = Metrics::new("1.2.3");
        let text = metrics.encode().unwrap();
        assert!(
            text.contains("okoala_build_info{version=\"1.2.3\"} 1"),
            "{text}"
        );
        assert!(
            text.contains("# TYPE process_start_time_seconds gauge"),
            "{text}"
        );
        assert!(
            text.contains("# TYPE okoala_uptime_seconds gauge"),
            "{text}"
        );
        assert!(text.ends_with("# EOF\n"), "{text}");
    }

    #[test]
    fn registered_metrics_are_exposed() {
        let metrics = Metrics::new("0.0.0");
        let counter: Counter = Counter::default();
        metrics.register("okoala_things", "Things", counter.clone());
        counter.inc_by(3);
        let text = metrics.encode().unwrap();
        assert!(text.contains("okoala_things_total 3"), "{text}");
    }
}
