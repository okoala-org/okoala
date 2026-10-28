// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! `OKOALA_<SECTION>__<KEY>` overrides, through an injected environment (the process environment is never
//! touched, so these tests run in parallel).

// Helpers outside `#[test]` functions unwrap too; a failure there is a test failure.
#![allow(clippy::unwrap_used)]

mod common;

use common::{Http, Internal, TEST_SECRET, registry, secret_file, temp_dir};
use okoala_config::{ConfigErrors, Loader, MapEnvironment};

fn base(test: &str) -> String {
    let dir = temp_dir(test);
    let token = secret_file(&dir, "token", TEST_SECRET.as_bytes(), 0o400);
    common::config_with_token(&token)
}

fn load(text: &str, env: &MapEnvironment) -> Result<okoala_config::Config, ConfigErrors> {
    let registry = registry();
    Loader::new(&registry, env).load_str(text)
}

#[test]
fn overrides_typed_and_string_values() {
    let env = MapEnvironment::new()
        .with("OKOALA_HTTP__PORT", "9443")
        .with("OKOALA_HTTP__HOSTNAME", "other.example.com")
        .with("OKOALA_HTTP__TLS__ENABLED", "true")
        .with("OKOALA_HTTP__TLS__CIPHERS", "[\"a\", \"b\"]");
    let config = load(&base("typed"), &env).unwrap();
    let http = config.get::<Http>().unwrap();
    assert_eq!(http.port, 9443);
    assert_eq!(http.hostname, "other.example.com");
    assert!(http.tls.enabled);
    assert_eq!(http.tls.ciphers, ["a", "b"]);
}

#[test]
fn quoted_value_stays_a_string() {
    let env = MapEnvironment::new().with("OKOALA_HTTP__HOSTNAME", "\"1234\"");
    let config = load(&base("quoted"), &env).unwrap();
    assert_eq!(config.get::<Http>().unwrap().hostname, "1234");
}

#[test]
fn override_can_supply_a_whole_section() {
    let dir = temp_dir("whole-section");
    let token = secret_file(&dir, "token", TEST_SECRET.as_bytes(), 0o400);
    let env = MapEnvironment::new()
        .with("OKOALA_HTTP__HOSTNAME", "from-env.example.com")
        .with("OKOALA_INTERNAL__TOKEN__FILE", token.display().to_string());
    let config = load("", &env).unwrap();
    assert_eq!(
        config.get::<Http>().unwrap().hostname,
        "from-env.example.com"
    );
    assert_eq!(
        config.get::<Internal>().unwrap().token.reveal_value(),
        TEST_SECRET
    );
}

#[test]
fn unrelated_variables_are_ignored() {
    let env = MapEnvironment::new()
        .with("OKOALA_LOG", "debug")
        .with("PATH", "/usr/bin")
        .with("HOME__X", "y");
    assert!(load(&base("ignored"), &env).is_ok());
}

#[test]
fn unknown_section_and_bad_values_from_env_are_all_reported() {
    let env = MapEnvironment::new()
        .with("OKOALA_IMAPP__PORT", "993")
        .with("OKOALA_HTTP__PORT", "not-a-number")
        .with("OKOALA_HTTP__NOPE", "1")
        .with("OKOALA_HTTP__", "1");
    let errors = load(&base("env-errors"), &env).unwrap_err();
    let rendered = errors.to_string();
    assert!(
        rendered
            .contains("[imapp]: unknown section (from environment variable OKOALA_IMAPP__PORT)"),
        "{rendered}"
    );
    assert!(
        rendered.contains("environment variable OKOALA_HTTP__: an override is named"),
        "{rendered}"
    );
    // `http` stops at its first structural error, which names the variable that set it.
    assert!(
        rendered.contains("(value from environment variable OKOALA_HTTP__"),
        "{rendered}"
    );
    assert_eq!(errors.len(), 3, "{rendered}");
}

#[test]
fn override_through_a_non_table_is_an_error() {
    let env = MapEnvironment::new().with("OKOALA_HTTP__HOSTNAME__X", "1");
    let errors = load(&base("non-table"), &env).unwrap_err();
    let error = &errors.errors()[0];
    assert_eq!(error.section(), Some("http"));
    assert!(
        error.message().contains("`http.hostname` is not a table"),
        "{error}"
    );
}

#[test]
fn secret_value_set_directly_by_override_is_rejected_without_echo() {
    let env = MapEnvironment::new().with("OKOALA_INTERNAL__TOKEN", TEST_SECRET);
    let errors = load(&base("direct-secret"), &env).unwrap_err();
    let error = &errors.errors()[0];
    assert_eq!(error.key(), Some("token"));
    assert!(
        error
            .message()
            .contains("inline secret values are not allowed"),
        "{error}"
    );
    assert!(
        error.message().contains("OKOALA_INTERNAL__TOKEN"),
        "{error}"
    );
    common::assert_no_secret(&errors);
}
