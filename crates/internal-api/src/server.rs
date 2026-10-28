// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! The accept loop: bounded connections, graceful shutdown with a deadline.

use std::convert::Infallible;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::server::graceful::GracefulShutdown;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::router::InternalApi;

/// Bounds on the listener's connections and requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most bytes of request line and headers buffered for one request (hyper's minimum is 8 KiB).
    pub max_header_bytes: usize,
    /// The most headers in one request.
    pub max_headers: usize,
    /// How long a client has to send a complete request head, counted from the moment the connection is
    /// accepted (and, on a kept-alive connection, from the end of the previous response). Short, so that idle
    /// or slow sockets cannot hold the connection slots that probes need.
    pub header_read_timeout: Duration,
    /// How long a connection may stay open in total; it is then closed.
    pub max_connection_age: Duration,
    /// The most connections served at once; further connections are closed on accept.
    pub max_connections: usize,
    /// How long shutdown waits for open connections to finish their requests. Always at least one second
    /// longer than `header_read_timeout`, so that a connection still sending its request head when shutdown
    /// starts is closed by that timeout, not counted as a drain failure.
    pub drain_deadline: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_header_bytes: 16 * 1024,
            max_headers: 64,
            header_read_timeout: Duration::from_secs(2),
            max_connection_age: Duration::from_secs(300),
            max_connections: 1024,
            drain_deadline: Duration::from_secs(10),
        }
    }
}

/// Why the listener stopped with an error.
#[derive(Debug)]
pub enum InternalApiError {
    /// Connections were still open when the drain deadline passed; they were dropped.
    DrainDeadline {
        /// The deadline that passed.
        deadline: Duration,
    },
}

impl fmt::Display for InternalApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DrainDeadline { deadline } => write!(
                f,
                "internal listener: connections still open after the {} ms drain deadline",
                deadline.as_millis()
            ),
        }
    }
}

impl std::error::Error for InternalApiError {}

/// Serves `api` on `listener` until `shutdown` is cancelled, then stops accepting, lets open connections
/// finish their current request (idle ones are closed at once) and returns; or returns
/// [`InternalApiError::DrainDeadline`] if they have not finished within `limits.drain_deadline`.
///
/// The caller binds the listener, so that a bind error is reported before anything runs and the bound
/// address (port 0 in tests) is known.
pub async fn serve(
    listener: TcpListener,
    api: Arc<InternalApi>,
    limits: Limits,
    shutdown: CancellationToken,
) -> Result<(), InternalApiError> {
    let graceful = GracefulShutdown::new();
    let mut over_cap = Summary::default();
    let mut accept_failures = Summary::default();
    let slots = Arc::new(Semaphore::new(limits.max_connections));
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .header_read_timeout(limits.header_read_timeout)
        .max_buf_size(limits.max_header_bytes.max(8 * 1024))
        .max_headers(limits.max_headers)
        .keep_alive(true);
    loop {
        let accepted = tokio::select! {
            biased;
            () = shutdown.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        let (stream, peer) = match accepted {
            Ok(accepted) => accepted,
            Err(error) => {
                // Out of file descriptors and the like: back off instead of spinning.
                if let Some(failures) = accept_failures.note() {
                    tracing::warn!(%error, failures, "internal listener: accept failed");
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
            api.connection_rejected();
            if let Some(rejected) = over_cap.note() {
                tracing::warn!(
                    rejected,
                    max_connections = limits.max_connections,
                    "internal listener: connections over the cap closed"
                );
            }
            drop(stream);
            continue;
        };
        let api = Arc::clone(&api);
        let service = service_fn(move |req| {
            let response = api.respond(&req);
            async move { Ok::<_, Infallible>(response) }
        });
        let connection = graceful.watch(builder.serve_connection(TokioIo::new(stream), service));
        let max_age = limits.max_connection_age;
        tokio::spawn(async move {
            let _slot = slot;
            match tokio::time::timeout(max_age, connection).await {
                Ok(Ok(())) => {}
                // Malformed requests, header timeouts, resets: the client's problem, not an operator's.
                Ok(Err(error)) => tracing::debug!(%peer, %error, "internal connection error"),
                Err(_) => tracing::debug!(%peer, "internal connection closed at its maximum age"),
            }
        });
    }
    drop(listener);
    // Kept-alive idle connections close at once; one still sending its request head closes when the header
    // timeout passes, which the deadline always outlasts.
    let deadline = limits
        .drain_deadline
        .max(limits.header_read_timeout + Duration::from_secs(1));
    tokio::select! {
        () = graceful.shutdown() => Ok(()),
        () = tokio::time::sleep(deadline) => Err(InternalApiError::DrainDeadline { deadline }),
    }
}

/// How often a repeated warning is logged at most.
const WARN_INTERVAL: Duration = Duration::from_secs(10);

/// Counts occurrences of one event and lets a warning through at most once per [`WARN_INTERVAL`], with the
/// number of occurrences since the last one, so a flood of events cannot flood the log.
#[derive(Default)]
struct Summary {
    count: u64,
    last: Option<Instant>,
}

impl Summary {
    /// Records one occurrence; `Some(count)` when a warning is due now.
    fn note(&mut self) -> Option<u64> {
        self.count += 1;
        let now = Instant::now();
        if self
            .last
            .is_some_and(|last| now.duration_since(last) < WARN_INTERVAL)
        {
            return None;
        }
        self.last = Some(now);
        Some(std::mem::take(&mut self.count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_let_one_warning_through_per_interval() {
        let mut summary = Summary::default();
        assert_eq!(summary.note(), Some(1));
        assert_eq!(summary.note(), None);
        assert_eq!(summary.note(), None);
        summary.last = Some(Instant::now() - WARN_INTERVAL);
        assert_eq!(summary.note(), Some(3));
    }
}
