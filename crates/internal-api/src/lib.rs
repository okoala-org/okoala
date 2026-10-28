// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! # Okoala Internal API
//!
//! The internal listener every process runs (implementation plan §4.20, D20): one HTTP/1.1 listener, port 9090
//! by default, with
//!
//! - `GET /healthz` (or `HEAD`): 200 while the process is alive;
//! - `GET /readyz` (or `HEAD`): 200 when every registered [`ReadinessCheck`] passes and the process is not
//!   shutting down, 503 otherwise;
//! - `GET /metrics` (or `HEAD`): the process's metrics ([`okoala_telemetry::Metrics`]) in the OpenMetrics text
//!   format;
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
//! headers, no request body (a request with one is refused with 413), and a cap on concurrent connections.
//! A connection must deliver a complete request head within [`Limits::header_read_timeout`] (2 s) of being
//! accepted or of its previous response, so idle and slow sockets give their slot back quickly and probes
//! get through again within seconds; there is no longer idle keep-alive timeout. Slots are not reserved for
//! probes: a probe cannot be told apart before its bytes are read, so a flood of connections can still delay
//! probes by up to the header timeout. Malformed requests get a 400 from the HTTP parser and never reach a
//! handler.
//!
//! Every request is logged at `debug` only, with its route name, method and status; outside `/internal/v1/`
//! also its path, cut to 128 bytes. The `Authorization` header, query strings and paths under `/internal/v1/`
//! (a client could put a token in one) are never logged. Connections closed over the cap and accept errors
//! are summarised in one `warn` per 10 s at most; the request and rejection counters count every one.

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
    /// The installation's internal secret, required as `Authorization: Bearer` on `/internal/v1/*`: at least
    /// [`MIN_TOKEN_LEN`] characters of RFC 7235 `token68` (`A-Z a-z 0-9 - . _ ~ + /`, then optional `=`
    /// padding), such as a generated base64 value. A token that is shorter or has other characters is a
    /// configuration error.
    ///
    /// `Secret::compare_bytes` returns early when the lengths differ, so a caller can learn the token's length
    /// by timing. That is accepted: the token is a generated value of a fixed, public length, so its length is
    /// no secret; its bytes are compared in constant time.
    #[serde(deserialize_with = "token")]
    pub token: Secret<String>,
}

/// The shortest accepted internal token.
pub const MIN_TOKEN_LEN: usize = 32;

fn token<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Secret<String>, D::Error> {
    let token = okoala_config::secret::string(deserializer)?;
    // Empty only as the loader's placeholder for a secret it could not fetch, whose error it has already
    // recorded (text secrets must be a non-blank line), so it is not reported twice.
    if token.reveal_value().is_empty() {
        return Ok(token);
    }
    if is_token68(token.reveal_value().as_bytes()) && token.reveal_value().len() >= MIN_TOKEN_LEN {
        Ok(token)
    } else {
        // Neither the value nor its length is reported.
        Err(serde::de::Error::invalid_value(
            serde::de::Unexpected::Other("a token of another form"),
            &"at least 32 token68 characters (A-Z a-z 0-9 - . _ ~ + /, then optional = padding)",
        ))
    }
}

/// RFC 7235 `token68`: `1*( ALPHA / DIGIT / "-" / "." / "_" / "~" / "+" / "/" ) *"="`.
fn is_token68(bytes: &[u8]) -> bool {
    let body = bytes
        .iter()
        .position(|&b| b == b'=')
        .map_or(bytes, |i| &bytes[..i]);
    !body.is_empty()
        && body
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b"-._~+/".contains(&b))
        && bytes[body.len()..].iter().all(|&b| b == b'=')
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

#[cfg(test)]
mod tests {
    use super::is_token68;

    #[test]
    fn token68_is_recognised() {
        assert!(is_token68(b"abcXYZ019-._~+/"));
        assert!(is_token68(b"abc=="));
        assert!(!is_token68(b""));
        assert!(!is_token68(b"=="));
        assert!(!is_token68(b"ab=c"));
        assert!(!is_token68(b"ab c"));
        assert!(!is_token68(b"ab\"c"));
        assert!(!is_token68("é".as_bytes()));
    }
}
