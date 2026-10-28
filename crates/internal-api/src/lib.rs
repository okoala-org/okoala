// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! # Okoala Internal API
//!
//! The internal listener every process runs (implementation plan §4.20, D20): one HTTP/1.1 listener, port 9090
//! by default, with
//!
//! - `GET /healthz`: 200 while the process is alive;
//! - `GET /readyz`: 200 when every registered [`ReadinessCheck`] passes and the process is not shutting down,
//!   503 otherwise;
//! - `GET /metrics`: the process's metrics ([`okoala_telemetry::Metrics`]) in the OpenMetrics text format;
//! - `/internal/v1/*`: calls between processes of one installation, each requiring
//!   `Authorization: Bearer <internal secret>` (the `[internal]` section's `token`), compared in constant
//!   time. For now only `GET /internal/v1/ping` (204), used by tests.
//!
//! The first three are unauthenticated; the NetworkPolicy keeps the port inside the namespace. The listener
//! has no TLS and makes no loopback assumptions: a request from `127.0.0.1` is treated like any other.
//!
//! An L4 crate: it depends on L0 (`okoala-secret`) and L1 (`okoala-config`, `okoala-telemetry`), never on
//! `okoala-db`. Readiness checks that need the database are written by the crates that own it and registered
//! as [`ReadinessCheck`] objects, so this crate does not see the database.
//!
//! Requests are bounded: at most [`Limits::max_header_bytes`] of request head and [`Limits::max_headers`]
//! headers, no request body (a request with one is refused with 413), a header read timeout, a maximum
//! connection age, and a cap on concurrent connections. Malformed requests get a 400 from the HTTP parser and
//! never reach a handler. The `Authorization` header and query strings are never logged.

mod readiness;
mod router;
mod server;

use std::net::{Ipv4Addr, SocketAddr};

use okoala_config::{Section, SectionDescriptor};
use okoala_secret::Secret;
use serde::Deserialize;

pub use readiness::{Readiness, ReadinessCheck};
pub use router::InternalApi;
pub use server::{InternalApiError, Limits, serve};

/// The internal listener's port (D20).
pub const INTERNAL_PORT: u16 = 9090;

/// `[internal]`: the internal listener.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InternalSection {
    /// The address to listen on; default `0.0.0.0:9090`.
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    /// The installation's internal secret, required as `Authorization: Bearer` on `/internal/v1/*`.
    #[serde(deserialize_with = "okoala_config::secret::string")]
    pub token: Secret<String>,
}

fn default_listen() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::UNSPECIFIED, INTERNAL_PORT))
}

impl Section for InternalSection {
    const NAME: &'static str = "internal";
}

/// The descriptors of this crate's configuration sections.
pub fn config() -> Vec<SectionDescriptor> {
    vec![SectionDescriptor::of::<InternalSection>()]
}
