// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! `okoala serve` as a running process: the internal listener, graceful shutdown on SIGTERM and SIGINT, JSON
//! logs, and the log-scrubbing test (§4.20, I10): no known test secret may appear in the output.

#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use common::{Signal, TestConfig, leaked_form, random_value};

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
        "internal request",
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
#[should_panic(expected = "a known test secret leaked to stdout")]
fn a_secret_file_value_reaching_the_logs_fails_the_test() {
    let mut config = TestConfig::new("leak");
    let path = config.secret_file("other");
    let value = std::fs::read_to_string(path).unwrap();
    let okoala = config.start();
    // Paths outside /internal/v1/ are logged (at debug, cut to 128 bytes): the value reaches the log.
    assert_eq!(okoala.get(&format!("/{}", value.trim()), &[]).0, 404);
    let _ = okoala.stop();
}

#[test]
fn internal_paths_are_never_logged() {
    let mut config = TestConfig::new("internal-path");
    let wrong = random_value();
    config.known_secret(wrong.clone());
    let okoala = config.start();
    // A client that puts a token in the path by mistake.
    assert_eq!(okoala.get(&format!("/internal/v1/{wrong}"), &[]).0, 401);
    let right = format!("Authorization: Bearer {}", okoala.internal_token());
    assert_eq!(
        okoala.get(&format!("/internal/v1/{wrong}"), &[&right]).0,
        404
    );
    let finished = okoala.stop();
    let requests: Vec<_> = finished
        .log_events()
        .into_iter()
        .filter(|e| e["message"] == "internal request")
        .collect();
    assert_eq!(requests.len(), 2, "{requests:?}");
    for e in requests {
        assert_eq!(e["route"], "internal_unknown", "{e}");
        assert!(e.get("path").is_none(), "{e}");
        assert_eq!(e["level"], "DEBUG", "{e}");
    }
}

#[test]
fn shutdown_with_a_half_sent_request_exits_0_quickly() {
    let okoala = TestConfig::new("half-request").start();
    let mut half = std::net::TcpStream::connect(okoala.internal_addr()).unwrap();
    std::io::Write::write_all(&mut half, b"GET /hea").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    let start = std::time::Instant::now();
    let finished = okoala.stop();
    assert_eq!(finished.status.code(), Some(0), "{}", finished.stdout);
    // Closed by the 2 s header timeout, well inside the drain deadline.
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    drop(half);
}

#[test]
fn a_second_signal_during_shutdown_aborts() {
    for (signal, code) in [(Signal::Term, 143), (Signal::Int, 130)] {
        let mut okoala = TestConfig::new("second-signal").start();
        // Holds the drain open until its header timeout (2 s).
        let mut half = std::net::TcpStream::connect(okoala.internal_addr()).unwrap();
        std::io::Write::write_all(&mut half, b"GET /hea").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        okoala.signal(Signal::Term);
        okoala.wait_for_log("shutdown requested");
        okoala.signal(signal);
        let finished = okoala.wait();
        assert_eq!(finished.status.code(), Some(code), "{}", finished.stdout);
        assert!(
            finished
                .stdout
                .contains("second signal during shutdown; aborting"),
            "{}",
            finished.stdout
        );
        drop(half);
    }
}

#[test]
fn every_encoded_form_of_a_secret_is_caught() {
    let secret = "6f3a0c1e9b7d2a4f8e6c0b1d3f5a7c9e";
    let caught = |text: &str| leaked_form(secret, text).map(|(form, _)| form);
    assert_eq!(caught("nothing here"), None);
    assert_eq!(caught(&format!("x{}y", &secret[3..17])), Some("as is"));
    assert_eq!(caught(&secret[..20].to_uppercase()), Some("upper case"));
    // "6f3a0c" as hex is 366633613063.
    assert_eq!(caught("..366633613063.."), Some("hex"));
    assert_eq!(caught("..3666336130633165.."), Some("hex"));
    // base64("6f3a0c1e9") = "NmYzYTBjMWU5".
    assert_eq!(caught("token=NmYzYTBjMWU5"), Some("base64"));
    assert_eq!(caught("%36%66%33%61%30%63"), Some("percent-encoded"));
    assert_eq!(
        caught("%36%66%33%61%30%63".to_lowercase().as_str()),
        Some("percent-encoded")
    );
    assert_eq!(
        caught("[54, 102, 51, 97, 48, 99, 49, 101, 57]"),
        Some("Debug of the bytes")
    );
    // Seven values are not enough.
    assert_eq!(caught("[54, 102, 51, 97, 48, 99, 49]"), None);
}

#[test]
fn url_safe_base64_differs_where_it_should() {
    // Bytes that encode with `+` and `/` in standard base64.
    let secret = "\u{3e}\u{3f}\u{3e}\u{3f}\u{3e}\u{3f}\u{3e}\u{3f}\u{3e}\u{3f}\u{3e}\u{3f}";
    let forms = common::encoded_forms(secret);
    let standard = &forms.iter().find(|(f, _)| *f == "base64").unwrap().1;
    let url = &forms
        .iter()
        .find(|(f, _)| *f == "URL-safe base64")
        .unwrap()
        .1;
    assert_eq!(standard, "Pj8+Pz4/Pj8+Pz4/");
    assert_eq!(url, "Pj8-Pz4_Pj8-Pz4_");
    assert_eq!(
        leaked_form(secret, "Pj8-Pz4_Pj8-").map(|(f, _)| f),
        Some("URL-safe base64")
    );
}
