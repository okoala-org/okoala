// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! `okoala serve` as a running process: the internal listener, graceful shutdown on SIGTERM and SIGINT, JSON
//! logs, and the log-scrubbing test (§4.20, I10): no known test secret may appear in the output.

#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use common::{Signal, TestConfig, leaked_piece, random_value};

#[test]
fn the_listener_serves_and_no_secret_reaches_the_logs() {
    let mut config = TestConfig::new("scrub");
    let wrong = random_value();
    config.known_secret(wrong.clone());
    // A token one character off: compared, refused, and not logged either.
    let mut near = config.internal_token().to_owned();
    near.pop();
    near.push('x');
    config.known_secret(near.clone());
    let token = config.internal_token().to_owned();
    let okoala = config.start();

    assert_eq!(okoala.get("/healthz", &[]).0, 200);
    let (status, body) = okoala.get("/readyz", &[]);
    assert_eq!((status, body.as_str()), (200, "ready\n"));
    let (status, metrics) = okoala.get("/metrics", &[]);
    assert_eq!(status, 200);
    assert!(metrics.contains("okoala_build_info{version="), "{metrics}");
    assert!(metrics.contains("okoala_uptime_seconds"), "{metrics}");

    assert_eq!(okoala.get("/internal/v1/ping", &[]).0, 401);
    for header in [
        format!("Authorization: Bearer {wrong}"),
        format!("Authorization: Bearer {near}"),
        format!("Authorization: Basic {wrong}"),
        format!("Authorization: {token}x"),
        "Authorization:".to_owned(),
    ] {
        assert_eq!(
            okoala.get("/internal/v1/ping", &[&header]).0,
            401,
            "{header}"
        );
    }
    // A wrong token in the query string is not logged either.
    assert_eq!(
        okoala
            .get(&format!("/internal/v1/ping?token={wrong}"), &[])
            .0,
        401
    );
    let right = format!("Authorization: Bearer {token}");
    assert_eq!(okoala.get("/internal/v1/ping", &[&right]).0, 204);

    // Malformed requests get a 400 from the parser; the process carries on.
    let garbage = okoala.request(format!("\x01{token} garbage\r\n\r\n").as_bytes());
    assert_eq!(common::status_of(&garbage), 400, "{garbage}");
    let bad_header = okoala.request(
        format!(
            "GET /internal/v1/ping HTTP/1.1\r\nAuthorization: Bearer {wrong}\r\nBad Header\r\n\r\n"
        )
        .as_bytes(),
    );
    assert_eq!(common::status_of(&bad_header), 400, "{bad_header}");
    assert_eq!(okoala.get("/healthz", &[]).0, 200);

    let finished = okoala.stop();
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    let events = finished.log_events();
    let messages: Vec<_> = events
        .iter()
        .filter_map(|e| e["message"].as_str())
        .collect();
    for expected in [
        "internal listener configuration",
        "internal listener started",
        "okoala started",
        "internal request refused",
        "shutdown requested",
        "okoala stopped",
    ] {
        assert!(messages.contains(&expected), "{expected}: {messages:?}");
    }
    // The configuration was logged, with the token redacted.
    let config_event = events
        .iter()
        .find(|e| e["message"] == "internal listener configuration")
        .unwrap();
    assert!(
        config_event["section"]
            .as_str()
            .unwrap()
            .contains("[REDACTED]"),
        "{config_event}"
    );
    let stop = events
        .iter()
        .find(|e| e["message"] == "shutdown requested")
        .unwrap();
    assert_eq!(stop["reason"], "SIGTERM");
}

#[test]
fn sigint_also_shuts_down_cleanly() {
    let okoala = TestConfig::new("sigint").start();
    assert_eq!(okoala.get("/healthz", &[]).0, 200);
    let finished = okoala.stop_with_signal(Signal::Int);
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stderr);
    assert!(finished.stdout.contains("\"reason\":\"SIGINT\""));
}

#[test]
fn every_secret_file_is_a_known_secret() {
    let mut config = TestConfig::new("extra-secret");
    let path = config.secret_file("other");
    let value = std::fs::read_to_string(&path).unwrap();
    // The scrubbing check would see it, whole or in part.
    assert!(leaked_piece(value.trim(), &format!("..{}..", &value[5..20])).is_some());
    assert!(leaked_piece(value.trim(), "nothing of it").is_none());
    let finished = config.start().stop();
    assert!(finished.status.success());
}

#[test]
#[should_panic(expected = "a known test secret leaked to stdout")]
fn the_scrubbing_check_fails_on_a_leak() {
    // A message the process always logs, declared a known secret: the check must catch it.
    let mut config = TestConfig::new("leak");
    config.known_secret("internal listener started");
    let _ = config.start().stop();
}
