// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Routing and authentication of the internal listener's requests.

use std::fmt;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Body;
use hyper::header::{self, HeaderMap, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use okoala_config::Config;
use okoala_telemetry::Metrics;
use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;

use crate::InternalSection;
use crate::readiness::Readiness;

/// The prefix of the authenticated routes.
const INTERNAL_PREFIX: &str = "/internal/v1";

/// The most bytes of a path logged (paths outside `/internal/v1/`).
const MAX_LOGGED_PATH: usize = 128;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct RequestLabels {
    route: &'static str,
    status: u16,
}

/// The internal listener's request handler: the routes of §4.20 over the process's configuration, metrics and
/// readiness.
pub struct InternalApi {
    config: Arc<Config>,
    metrics: Arc<Metrics>,
    readiness: Arc<Readiness>,
    requests: Family<RequestLabels, Counter>,
    rejected: Counter,
}

impl fmt::Debug for InternalApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InternalApi")
            .field("readiness", &self.readiness)
            .finish_non_exhaustive()
    }
}

impl InternalApi {
    /// The handler for a process whose configuration is `config` (its `[internal]` section holds the token).
    /// Registers `okoala_internal_requests_total{route, status}` and `okoala_internal_connections_rejected_total`
    /// in `metrics`.
    ///
    /// If `config` has no `[internal]` section (it was loaded without this crate's descriptors), every
    /// `/internal/v1/*` request is refused.
    pub fn new(config: Arc<Config>, metrics: Arc<Metrics>, readiness: Arc<Readiness>) -> Self {
        let requests = Family::<RequestLabels, Counter>::default();
        metrics.register(
            "okoala_internal_requests",
            "Requests to the internal listener, by route and status",
            requests.clone(),
        );
        let rejected = Counter::default();
        metrics.register(
            "okoala_internal_connections_rejected",
            "Connections to the internal listener closed because all connection slots were taken",
            rejected.clone(),
        );
        Self {
            config,
            metrics,
            readiness,
            requests,
            rejected,
        }
    }

    /// Counts a connection closed over the connection cap.
    pub(crate) fn connection_rejected(&self) {
        self.rejected.inc();
    }

    /// The process readiness this handler reports.
    pub fn readiness(&self) -> &Arc<Readiness> {
        &self.readiness
    }

    /// Answers one request. Never panics, whatever the request.
    pub fn respond<B: Body>(&self, req: &Request<B>) -> Response<Full<Bytes>> {
        let (route, response) = self.route(req);
        let status = response.status().as_u16();
        self.requests
            .get_or_create(&RequestLabels { route, status })
            .inc();
        // Every request, refused ones included, is logged at `debug` only, so a client cannot flood the logs.
        // Headers (`Authorization`) and query strings are never logged; under `/internal/v1/` not even the path
        // is (a client could put a token in it by mistake), only the route name; elsewhere the path is cut.
        let method = truncate(req.method().as_str(), 16);
        if route == "other" {
            let path = truncate(req.uri().path(), MAX_LOGGED_PATH);
            tracing::debug!(method, route, path, status, "internal request");
        } else {
            tracing::debug!(method, route, status, "internal request");
        }
        response
    }

    fn route<B: Body>(&self, req: &Request<B>) -> (&'static str, Response<Full<Bytes>>) {
        let path = req.uri().path();
        let internal = path == INTERNAL_PREFIX
            || path
                .strip_prefix(INTERNAL_PREFIX)
                .is_some_and(|rest| rest.starts_with('/'));
        // The route (the metric label) is found first; the checks below only decide the answer.
        let route = match path {
            "/healthz" => "healthz",
            "/readyz" => "readyz",
            "/metrics" => "metrics",
            "/internal/v1/ping" => "ping",
            _ if internal => "internal_unknown",
            _ => "other",
        };
        // Authentication comes first, so an unauthenticated caller learns nothing about which routes exist.
        if internal && !self.authorized(req.headers()) {
            let mut response = text(StatusCode::UNAUTHORIZED, "unauthorized\n");
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
            return (route, response);
        }
        // No route takes a body; refusing one keeps the request bounded.
        if !req.body().is_end_stream() {
            let mut response = text(StatusCode::PAYLOAD_TOO_LARGE, "request body not allowed\n");
            response
                .headers_mut()
                .insert(header::CONNECTION, HeaderValue::from_static("close"));
            return (route, response);
        }
        if matches!(route, "internal_unknown" | "other") {
            return (route, text(StatusCode::NOT_FOUND, "not found\n"));
        }
        // The probe and scrape routes also answer HEAD (hyper sends no body for it).
        let (allowed, allow) = if route == "ping" {
            (req.method() == Method::GET, "GET")
        } else {
            (
                req.method() == Method::GET || req.method() == Method::HEAD,
                "GET, HEAD",
            )
        };
        if !allowed {
            let mut response = text(StatusCode::METHOD_NOT_ALLOWED, "method not allowed\n");
            response
                .headers_mut()
                .insert(header::ALLOW, HeaderValue::from_static(allow));
            return (route, response);
        }
        let response = match route {
            "healthz" => text(StatusCode::OK, "ok\n"),
            "readyz" => match self.readiness.check() {
                Ok(()) => text(StatusCode::OK, "ready\n"),
                Err(reasons) => text(
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("not ready: {}\n", reasons.join(", ")),
                ),
            },
            "metrics" => match self.metrics.encode() {
                Ok(body) => {
                    let mut response = text(StatusCode::OK, body);
                    response.headers_mut().insert(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static(okoala_telemetry::CONTENT_TYPE),
                    );
                    response
                }
                Err(_) => text(StatusCode::INTERNAL_SERVER_ERROR, "metrics unavailable\n"),
            },
            _ => empty(StatusCode::NO_CONTENT),
        };
        (route, response)
    }

    fn authorized(&self, headers: &HeaderMap) -> bool {
        let Some(section) = self.config.get::<InternalSection>() else {
            return false;
        };
        bearer_token(headers).is_some_and(|token| section.token.compare_bytes(token))
    }
}

/// The token of the request's one `Authorization: Bearer <token>` header (RFC 6750 §2.1): the scheme in any
/// case, one or more spaces, then a token without whitespace. `None` if the header is missing, repeated,
/// another scheme, or has an empty or malformed token.
fn bearer_token(headers: &HeaderMap) -> Option<&[u8]> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    let bytes = value.as_bytes();
    let space = bytes.iter().position(|&b| b == b' ')?;
    let (scheme, rest) = bytes.split_at(space);
    if !scheme.eq_ignore_ascii_case(b"bearer") {
        return None;
    }
    let token = rest.trim_ascii();
    if token.is_empty() || token.iter().any(|b| b.is_ascii_whitespace()) {
        return None;
    }
    Some(token)
}

/// At most `max` bytes of `s`, cut on a character boundary.
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn text(status: StatusCode, body: impl Into<Bytes>) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(body.into()));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn empty(status: StatusCode) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::new()));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(values: &[&'static str]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for v in values {
            map.append(header::AUTHORIZATION, HeaderValue::from_static(v));
        }
        map
    }

    #[test]
    fn truncation_keeps_whole_characters() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abcdef", 3), "abc");
        assert_eq!(truncate("aé", 2), "a");
        assert_eq!(truncate("", 0), "");
    }

    #[test]
    fn bearer_tokens_are_parsed_strictly() {
        assert_eq!(bearer_token(&headers(&["Bearer abc"])), Some(&b"abc"[..]));
        assert_eq!(bearer_token(&headers(&["bearer abc"])), Some(&b"abc"[..]));
        assert_eq!(bearer_token(&headers(&["BEARER abc"])), Some(&b"abc"[..]));
        assert_eq!(bearer_token(&headers(&["Bearer   abc"])), Some(&b"abc"[..]));
        assert_eq!(bearer_token(&headers(&["Bearer abc  "])), Some(&b"abc"[..]));
        assert_eq!(bearer_token(&headers(&[])), None);
        assert_eq!(bearer_token(&headers(&[""])), None);
        assert_eq!(bearer_token(&headers(&["Bearer"])), None);
        assert_eq!(bearer_token(&headers(&["Bearer "])), None);
        assert_eq!(bearer_token(&headers(&["Bearer    "])), None);
        assert_eq!(bearer_token(&headers(&["Bearer a b"])), None);
        assert_eq!(bearer_token(&headers(&["Bearer a\tb"])), None);
        assert_eq!(bearer_token(&headers(&["Bearer\tabc"])), None);
        assert_eq!(bearer_token(&headers(&["Basic abc"])), None);
        assert_eq!(bearer_token(&headers(&["Bearerabc"])), None);
        assert_eq!(bearer_token(&headers(&["Bearer abc", "Bearer abc"])), None);
    }
}
