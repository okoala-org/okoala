// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! The accept loop: bounded connections, graceful shutdown with a deadline.

use std::convert::Infallible;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

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
    /// How long a client has to send a request's headers once it starts (and, on an idle kept-alive
    /// connection, the next request's).
    pub header_read_timeout: Duration,
    /// How long a connection may stay open in total; it is then closed.
    pub max_connection_age: Duration,
    /// The most connections served at once; further connections are closed on accept.
    pub max_connections: usize,
    /// How long shutdown waits for open connections to finish their requests.
    pub drain_deadline: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_header_bytes: 16 * 1024,
            max_headers: 64,
            header_read_timeout: Duration::from_secs(10),
            max_connection_age: Duration::from_secs(300),
            max_connections: 256,
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
                tracing::warn!(%error, "internal listener: accept failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
            tracing::warn!(%peer, "internal listener: too many connections, closing one");
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
    tokio::select! {
        () = graceful.shutdown() => Ok(()),
        () = tokio::time::sleep(limits.drain_deadline) => Err(InternalApiError::DrainDeadline {
            deadline: limits.drain_deadline,
        }),
    }
}
