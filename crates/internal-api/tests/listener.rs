// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! The internal listener over real sockets: routes, authentication, bounds, readiness and graceful shutdown.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use okoala_config::{Loader, MapEnvironment, Registry};
use okoala_internal_api::{InternalApi, Limits, Readiness, ReadinessCheck, serve};
use okoala_telemetry::Metrics;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const TOKEN: &str = "internal-Token-of-the-Listener-Test";

struct Server {
    addr: SocketAddr,
    readiness: Arc<Readiness>,
    shutdown: CancellationToken,
    task: JoinHandle<Result<(), okoala_internal_api::InternalApiError>>,
}

impl Server {
    async fn start() -> Self {
        Self::start_with(Limits::default()).await
    }

    async fn start_with(limits: Limits) -> Self {
        let mut registry = Registry::new();
        for d in okoala_internal_api::config() {
            registry.register(d).unwrap();
        }
        let env = MapEnvironment::new().with("TEST_INTERNAL_TOKEN", TOKEN);
        let config = Loader::new(&registry, &env)
            .load_str("[internal]\ntoken = { secret_env = \"TEST_INTERNAL_TOKEN\" }\n")
            .unwrap();
        let metrics = Arc::new(Metrics::new("9.9.9"));
        let readiness = Arc::new(Readiness::new());
        let api = Arc::new(InternalApi::new(
            Arc::new(config),
            metrics,
            Arc::clone(&readiness),
        ));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(serve(listener, api, limits, shutdown.clone()));
        Self {
            addr,
            readiness,
            shutdown,
            task,
        }
    }

    /// Sends `raw` on a new connection and returns everything the server sends back until it closes.
    async fn raw(&self, raw: &[u8]) -> String {
        let mut stream = TcpStream::connect(self.addr).await.unwrap();
        stream.write_all(raw).await.unwrap();
        let mut out = Vec::new();
        tokio::time::timeout(Duration::from_secs(10), stream.read_to_end(&mut out))
            .await
            .expect("the server did not close the connection")
            .unwrap();
        String::from_utf8_lossy(&out).into_owned()
    }

    async fn get(&self, path: &str, headers: &[&str]) -> String {
        let mut request = format!("GET {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n");
        for h in headers {
            request.push_str(h);
            request.push_str("\r\n");
        }
        request.push_str("\r\n");
        self.raw(request.as_bytes()).await
    }

    async fn stop(self) {
        self.shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .expect("serve did not return after shutdown")
            .unwrap()
            .unwrap();
    }
}

fn status(response: &str) -> u16 {
    response
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("no status line in {response:?}"))
}

#[tokio::test]
async fn health_readiness_and_metrics_are_open() {
    let server = Server::start().await;
    let health = server.get("/healthz", &[]).await;
    assert_eq!(status(&health), 200, "{health}");
    assert!(health.ends_with("ok\n"));
    let ready = server.get("/readyz", &[]).await;
    assert_eq!(status(&ready), 200, "{ready}");
    let metrics = server.get("/metrics", &[]).await;
    assert_eq!(status(&metrics), 200, "{metrics}");
    assert!(
        metrics.contains("content-type: application/openmetrics-text"),
        "{metrics}"
    );
    assert!(metrics.contains("okoala_build_info{version=\"9.9.9\"} 1"));
    assert!(
        metrics.contains("okoala_internal_requests_total{route=\"healthz\",status=\"200\"} 1"),
        "{metrics}"
    );
    assert!(metrics.contains("# EOF"));
    assert_eq!(status(&server.get("/nope", &[]).await), 404);
    server.stop().await;
}

#[tokio::test]
async fn internal_routes_need_the_bearer_token() {
    let server = Server::start().await;
    let refused = server.get("/internal/v1/ping", &[]).await;
    assert_eq!(status(&refused), 401, "{refused}");
    assert!(refused.contains("www-authenticate: Bearer"), "{refused}");
    for header in [
        "Authorization: Bearer wrong-token".to_owned(),
        format!("Authorization: Bearer {TOKEN}x"),
        format!("Authorization: Bearer {}", &TOKEN[..TOKEN.len() - 1]),
        format!("Authorization: Basic {TOKEN}"),
        format!("Authorization: Bearer{TOKEN}"),
        format!("Authorization: {TOKEN}"),
        format!("Authorization: Bearer {TOKEN} {TOKEN}"),
        "Authorization:".to_owned(),
        "Authorization: Bearer".to_owned(),
        "Authorization: Bearer ".to_owned(),
    ] {
        let response = server.get("/internal/v1/ping", &[&header]).await;
        assert_eq!(status(&response), 401, "{header}: {response}");
    }
    // Two headers, even both right, are refused.
    let right = format!("Authorization: Bearer {TOKEN}");
    let twice = server.get("/internal/v1/ping", &[&right, &right]).await;
    assert_eq!(status(&twice), 401, "{twice}");
    for header in [
        format!("Authorization: Bearer {TOKEN}"),
        format!("Authorization: bearer {TOKEN}"),
        format!("authorization: BEARER {TOKEN}"),
        format!("Authorization: Bearer    {TOKEN}"),
        format!("Authorization:    Bearer {TOKEN}   "),
    ] {
        let response = server.get("/internal/v1/ping", &[&header]).await;
        assert_eq!(status(&response), 204, "{header}: {response}");
    }
    server.stop().await;
}

#[tokio::test]
async fn unknown_internal_routes_are_hidden_behind_authentication() {
    let server = Server::start().await;
    let right = format!("Authorization: Bearer {TOKEN}");
    assert_eq!(status(&server.get("/internal/v1/search", &[]).await), 401);
    assert_eq!(status(&server.get("/internal/v1", &[]).await), 401);
    assert_eq!(
        status(&server.get("/internal/v1/search", &[&right]).await),
        404
    );
    // Not under the prefix: an ordinary unknown path.
    assert_eq!(status(&server.get("/internal/v10", &[]).await), 404);
    // The query string is not part of the route.
    assert_eq!(
        status(&server.get("/internal/v1/ping?x=1", &[&right]).await),
        204
    );
    server.stop().await;
}

#[tokio::test]
async fn methods_bodies_and_malformed_requests_are_refused() {
    let server = Server::start().await;
    let post = server
        .raw(b"POST /healthz HTTP/1.1\r\nHost: t\r\nConnection: close\r\nContent-Length: 0\r\n\r\n")
        .await;
    assert_eq!(status(&post), 405, "{post}");
    assert!(post.contains("allow: GET"), "{post}");
    let body = server
        .raw(b"GET /healthz HTTP/1.1\r\nHost: t\r\nContent-Length: 5\r\n\r\nhello")
        .await;
    assert_eq!(status(&body), 413, "{body}");
    let chunked = server
        .raw(b"GET /healthz HTTP/1.1\r\nHost: t\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n")
        .await;
    assert_eq!(status(&chunked), 413, "{chunked}");
    let garbage = server.raw(b"\x00\x01GARBAGE\xff\r\n\r\n").await;
    assert_eq!(status(&garbage), 400, "{garbage}");
    let bad_header = server
        .raw(b"GET /healthz HTTP/1.1\r\nHo st: t\r\n\r\n")
        .await;
    assert_eq!(status(&bad_header), 400, "{bad_header}");
    let huge = format!(
        "GET /healthz HTTP/1.1\r\nHost: t\r\nX-Big: {}\r\n\r\n",
        "a".repeat(64 * 1024)
    );
    let huge = server.raw(huge.as_bytes()).await;
    assert_eq!(status(&huge), 431, "{huge}");
    let mut many = String::from("GET /healthz HTTP/1.1\r\nHost: t\r\n");
    for i in 0..200 {
        many.push_str(&format!("X-{i}: v\r\n"));
    }
    many.push_str("\r\n");
    let many = server.raw(many.as_bytes()).await;
    assert_eq!(status(&many), 431, "{many}");
    // The listener still works.
    assert_eq!(status(&server.get("/healthz", &[]).await), 200);
    server.stop().await;
}

#[tokio::test]
async fn slow_clients_are_timed_out() {
    let server = Server::start_with(Limits {
        header_read_timeout: Duration::from_millis(200),
        ..Limits::default()
    })
    .await;
    // Half a request line, then nothing: the server closes the connection.
    let mut stream = TcpStream::connect(server.addr).await.unwrap();
    stream.write_all(b"GET /hea").await.unwrap();
    let mut out = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut out))
        .await
        .expect("the slow connection was not closed")
        .ok();
    server.stop().await;
}

struct Flag(bool);

impl ReadinessCheck for Flag {
    fn name(&self) -> &str {
        "database"
    }
    fn is_ready(&self) -> bool {
        self.0
    }
}

#[tokio::test]
async fn readiness_reports_failing_checks_and_shutdown() {
    let server = Server::start().await;
    server.readiness.register(Arc::new(Flag(true)));
    assert_eq!(status(&server.get("/readyz", &[]).await), 200);
    server.readiness.register(Arc::new(Flag(false)));
    let failing = server.get("/readyz", &[]).await;
    assert_eq!(status(&failing), 503, "{failing}");
    assert!(failing.ends_with("not ready: database\n"), "{failing}");
    server.readiness.start_shutdown();
    let stopping = server.get("/readyz", &[]).await;
    assert!(
        stopping.ends_with("not ready: shutting down, database\n"),
        "{stopping}"
    );
    // Health is unaffected.
    assert_eq!(status(&server.get("/healthz", &[]).await), 200);
    server.stop().await;
}

#[tokio::test]
async fn shutdown_closes_idle_connections_and_stops_accepting() {
    let server = Server::start().await;
    // A kept-alive connection that has had one response and is now idle.
    let mut idle = TcpStream::connect(server.addr).await.unwrap();
    idle.write_all(b"GET /healthz HTTP/1.1\r\nHost: t\r\n\r\n")
        .await
        .unwrap();
    let mut buf = [0u8; 1024];
    let n = idle.read(&mut buf).await.unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 200"));
    let addr = server.addr;
    // Returns promptly although `idle` is still open.
    server.stop().await;
    let mut rest = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), idle.read_to_end(&mut rest))
        .await
        .expect("the idle connection was not closed")
        .ok();
    assert!(TcpStream::connect(addr).await.is_err());
}

#[tokio::test]
async fn connections_over_the_cap_are_closed() {
    let server = Server::start_with(Limits {
        max_connections: 1,
        ..Limits::default()
    })
    .await;
    let mut first = TcpStream::connect(server.addr).await.unwrap();
    first
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: t\r\n\r\n")
        .await
        .unwrap();
    let mut buf = [0u8; 1024];
    let n = first.read(&mut buf).await.unwrap();
    assert!(String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 200"));
    // `first` holds the only slot: the second connection is closed without an answer.
    let mut second = TcpStream::connect(server.addr).await.unwrap();
    let _ = second
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: t\r\n\r\n")
        .await;
    let mut out = Vec::new();
    // Closed (or reset) without a response.
    let _ = tokio::time::timeout(Duration::from_secs(5), second.read_to_end(&mut out))
        .await
        .expect("the connection over the cap was not closed");
    assert!(out.is_empty(), "{}", String::from_utf8_lossy(&out));
    drop(first);
    server.stop().await;
}
