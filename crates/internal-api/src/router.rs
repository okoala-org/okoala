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
    /// Registers `okoala_internal_requests_total{route, status}` in `metrics`.
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
        Self {
            config,
            metrics,
            readiness,
            requests,
        }
    }

    /// The process readiness this handler reports.
    pub fn readiness(&self) -> &Arc<Readiness> {
        &self.readiness
    }

    /// Answers one request. Never panics, whatever the request.
    pub fn respond<B: Body>(&self, req: &Request<B>) -> Response<Full<Bytes>> {
        let path = req.uri().path();
        let (route, response) = self.route(req, path);
        let status = response.status();
        self.requests
            .get_or_create(&RequestLabels {
                route,
                status: status.as_u16(),
            })
            .inc();
        // The path only: query strings and headers (`Authorization`) are never logged.
        if status == StatusCode::UNAUTHORIZED {
            tracing::info!(method = %req.method(), path, status = status.as_u16(), "internal request refused");
        } else {
            tracing::debug!(method = %req.method(), path, status = status.as_u16(), "internal request");
        }
        response
    }

    fn route<B: Body>(
        &self,
        req: &Request<B>,
        path: &str,
    ) -> (&'static str, Response<Full<Bytes>>) {
        let internal = path == INTERNAL_PREFIX
            || path
                .strip_prefix(INTERNAL_PREFIX)
                .is_some_and(|rest| rest.starts_with('/'));
        // Authentication comes first, so an unauthenticated caller learns nothing about which routes exist.
        if internal && !self.authorized(req.headers()) {
            let mut response = text(StatusCode::UNAUTHORIZED, "unauthorized\n");
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
            return ("internal", response);
        }
        // No route takes a body; refusing one keeps the request bounded and the connection reusable.
        if !req.body().is_end_stream() {
            let mut response = text(StatusCode::PAYLOAD_TOO_LARGE, "request body not allowed\n");
            response
                .headers_mut()
                .insert(header::CONNECTION, HeaderValue::from_static("close"));
            return (if internal { "internal" } else { "other" }, response);
        }
        let route = match path {
            "/healthz" => "healthz",
            "/readyz" => "readyz",
            "/metrics" => "metrics",
            "/internal/v1/ping" => "internal",
            _ if internal => return ("internal", text(StatusCode::NOT_FOUND, "not found\n")),
            _ => return ("other", text(StatusCode::NOT_FOUND, "not found\n")),
        };
        if req.method() != Method::GET {
            let mut response = text(StatusCode::METHOD_NOT_ALLOWED, "method not allowed\n");
            response
                .headers_mut()
                .insert(header::ALLOW, HeaderValue::from_static("GET"));
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
