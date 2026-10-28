// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! The registry and the loader's handling of sections and keys, without secrets in play.

// Helpers outside `#[test]` functions unwrap too; a failure there is a test failure.
#![allow(clippy::unwrap_used)]

mod common;

use common::{Http, Internal, TEST_SECRET, registry, secret_file, temp_dir};
use okoala_config::{
    ConfigErrors, Loader, MapEnvironment, Registry, RegistryError, Section, SectionDescriptor,
};
use serde::Deserialize;

fn load(text: &str) -> Result<okoala_config::Config, ConfigErrors> {
    let registry = registry();
    let env = MapEnvironment::new();
    Loader::new(&registry, &env).load_str(text)
}

fn with_valid_token(test: &str, rest: &str) -> String {
    let dir = temp_dir(test);
    let token = secret_file(&dir, "token", TEST_SECRET.as_bytes(), 0o400);
    format!("{}{rest}", common::config_with_token(&token))
}

#[test]
fn loads_sections_with_defaults() {
    let config = load(&with_valid_token("defaults", "")).unwrap();
    let http = config.get::<Http>().unwrap();
    assert_eq!(http.port, 8080);
    assert_eq!(http.hostname, "mail.example.com");
    assert!(!http.tls.enabled);
    assert!(config.get::<Internal>().is_some());
    assert_eq!(
        config.section_names().collect::<Vec<_>>(),
        ["http", "internal"]
    );
}

#[test]
fn unknown_section_is_an_error() {
    let errors = load(&with_valid_token(
        "unknown-section",
        "\n[imapp]\nport = 993\n",
    ))
    .unwrap_err();
    assert_eq!(errors.len(), 1);
    let error = &errors.errors()[0];
    assert_eq!(error.section(), Some("imapp"));
    assert_eq!(error.key(), None);
    assert!(error.message().contains("unknown section"), "{error}");
    assert!(error.message().contains("http, internal"), "{error}");
}

#[test]
fn unknown_key_is_reported_with_its_path() {
    let text = with_valid_token("unknown-key", "").replace("[http]\n", "[http]\nprot = 1\n");
    let errors = load(&text).unwrap_err();
    assert_eq!(errors.len(), 1, "{errors}");
    let error = &errors.errors()[0];
    assert_eq!(error.section(), Some("http"));
    assert_eq!(error.key(), Some("prot"));
    assert!(error.message().contains("unknown field `prot`"), "{error}");
}

#[test]
fn nested_unknown_key_is_reported_with_its_path() {
    let errors = load(&with_valid_token(
        "nested-unknown",
        "\n[http.tls]\nenabeld = true\n",
    ))
    .unwrap_err();
    let error = &errors.errors()[0];
    assert_eq!(error.section(), Some("http"));
    assert_eq!(error.key(), Some("tls.enabeld"));
}

#[test]
fn wrong_type_in_array_is_reported_with_its_index() {
    let errors = load(&with_valid_token(
        "array-index",
        "\n[http.tls]\nciphers = [\"a\", 2]\n",
    ))
    .unwrap_err();
    let error = &errors.errors()[0];
    assert_eq!(error.key(), Some("tls.ciphers[1]"), "{error}");
}

#[test]
fn missing_required_field_is_reported() {
    let dir = temp_dir("missing-field");
    let token = secret_file(&dir, "token", TEST_SECRET.as_bytes(), 0o400);
    let text = format!(
        "[internal]\ntoken = {{ secret_file = '{}' }}\n",
        token.display()
    );
    let errors = load(&text).unwrap_err();
    assert_eq!(errors.len(), 1, "{errors}");
    let error = &errors.errors()[0];
    assert_eq!(error.section(), Some("http"));
    assert!(
        error.message().contains("missing field `hostname`"),
        "{error}"
    );
}

#[test]
fn all_errors_are_reported_together() {
    let text = "[http]\nhostname = \"x\"\nport = 70000\n\n[internal]\ntoken = { secret_file = '/nonexistent/token' }\n\n[smtp]\n\n[imap]\n";
    let errors = load(text).unwrap_err();
    let rendered = errors.to_string();
    assert_eq!(errors.len(), 4, "{rendered}");
    assert!(
        rendered.starts_with("4 configuration errors:"),
        "{rendered}"
    );
    assert!(rendered.contains("[http] port:"), "{rendered}");
    assert!(
        rendered.contains("[internal] token: cannot open secret file /nonexistent/token"),
        "{rendered}"
    );
    assert!(rendered.contains("[smtp]: unknown section"), "{rendered}");
    assert!(rendered.contains("[imap]: unknown section"), "{rendered}");
}

#[test]
fn syntax_error_gives_position_without_quoting_the_line() {
    let errors = load(&format!(
        "[http]\nhostname = \"x\"\npassword = {TEST_SECRET}\n"
    ))
    .unwrap_err();
    let rendered = errors.to_string();
    assert!(
        rendered.contains("TOML syntax error at line 3"),
        "{rendered}"
    );
    common::assert_no_secret(&errors);
}

#[test]
fn wrong_type_error_does_not_echo_the_string() {
    let text = with_valid_token("no-echo", "")
        .replace("[http]\n", &format!("[http]\nport = \"{TEST_SECRET}\"\n"));
    let errors = load(&text).unwrap_err();
    assert!(
        errors
            .to_string()
            .contains("invalid type: a string, expected u16"),
        "{errors}"
    );
    common::assert_no_secret(&errors);
}

#[test]
fn missing_file_is_an_error() {
    let registry = registry();
    let env = MapEnvironment::new();
    let errors = Loader::new(&registry, &env)
        .load_file(std::path::Path::new("/nonexistent/okoala.toml"))
        .unwrap_err();
    assert!(
        errors
            .to_string()
            .contains("cannot read configuration file /nonexistent/okoala.toml")
    );
}

#[test]
fn load_file_reads_the_file() {
    let dir = temp_dir("load-file");
    let path = dir.join("okoala.toml");
    std::fs::write(&path, with_valid_token("load-file-token", "")).unwrap();
    let registry = registry();
    let env = MapEnvironment::new();
    let config = Loader::new(&registry, &env).load_file(&path).unwrap();
    assert!(config.get::<Http>().is_some());
}

#[derive(Debug, Deserialize)]
struct Other {}

impl Section for Other {
    const NAME: &'static str = "http";
}

#[derive(Debug, Deserialize)]
struct BadName {}

impl Section for BadName {
    const NAME: &'static str = "smtp-in";
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
enum Mode {
    Single,
    Split { replicas: u8 },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Deploy {
    mode: Mode,
    #[serde(default)]
    other: Option<Mode>,
}

impl Section for Deploy {
    const NAME: &'static str = "deploy";
}

#[test]
fn registry_rejects_conflicts_and_bad_names() {
    let mut registry = Registry::new();
    registry.register(SectionDescriptor::of::<Http>()).unwrap();
    // The same type twice is fine (two roles sharing a section).
    registry.register(SectionDescriptor::of::<Http>()).unwrap();
    let conflict = registry
        .register(SectionDescriptor::of::<Other>())
        .unwrap_err();
    assert!(
        matches!(conflict, RegistryError::Conflict { name: "http", .. }),
        "{conflict}"
    );
    let bad = registry
        .register(SectionDescriptor::of::<BadName>())
        .unwrap_err();
    assert_eq!(bad, RegistryError::InvalidName { name: "smtp-in" });
    assert_eq!(registry.names().collect::<Vec<_>>(), ["http"]);
}

#[test]
fn enums_load_from_strings_and_tables() {
    let mut registry = Registry::new();
    registry
        .register(SectionDescriptor::of::<Deploy>())
        .unwrap();
    let env = MapEnvironment::new();
    let loader = Loader::new(&registry, &env);
    let config = loader
        .load_str("[deploy]\nmode = \"Single\"\nother = { Split = { replicas = 3 } }\n")
        .unwrap();
    let deploy = config.get::<Deploy>().unwrap();
    assert!(matches!(deploy.mode, Mode::Single));
    assert!(matches!(deploy.other, Some(Mode::Split { replicas: 3 })));
    let errors = loader
        .load_str("[deploy]\nmode = { Split = { replicas = 300 } }\n")
        .unwrap_err();
    assert_eq!(
        errors.errors()[0].key(),
        Some("mode.Split.replicas"),
        "{errors}"
    );
}

#[test]
fn get_returns_none_for_an_unregistered_section() {
    let mut registry = Registry::new();
    registry
        .register(SectionDescriptor::of::<Deploy>())
        .unwrap();
    let env = MapEnvironment::new();
    let config = Loader::new(&registry, &env)
        .load_str("[deploy]\nmode = \"Single\"\n")
        .unwrap();
    assert!(config.get::<Http>().is_none());
    assert_eq!(format!("{config:?}"), "Config { sections: [\"deploy\"] }");
}

/// A field type whose own error message quotes its input, as many `FromStr`-based types do.
#[derive(Debug)]
struct Echo;

impl<'de> Deserialize<'de> for Echo {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let input = String::deserialize(deserializer)?;
        Err(serde::de::Error::custom(format!("cannot parse {input:?}")))
    }
}

// Fields are only deserialized: every test input is invalid.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Echoing {
    #[serde(default)]
    echo: Option<Echo>,
    #[serde(default)]
    addr: Option<std::net::SocketAddr>,
    #[serde(default)]
    mode: Option<Mode>,
}

impl Section for Echoing {
    const NAME: &'static str = "echoing";
}

#[test]
fn field_values_are_never_echoed() {
    let mut registry = Registry::new();
    registry
        .register(SectionDescriptor::of::<Echoing>())
        .unwrap();
    for key in ["echo", "addr", "mode"] {
        let from_file = format!("[echoing]\n{key} = \"{TEST_SECRET}\"\n");
        let env = MapEnvironment::new();
        let errors = Loader::new(&registry, &env)
            .load_str(&from_file)
            .unwrap_err();
        assert_eq!(errors.errors()[0].key(), Some(key), "{errors}");
        common::assert_no_secret(&errors);

        let env = MapEnvironment::new().with(
            format!("OKOALA_ECHOING__{}", key.to_uppercase()),
            TEST_SECRET,
        );
        let errors = Loader::new(&registry, &env).load_str("").unwrap_err();
        assert_eq!(errors.errors()[0].key(), Some(key), "{errors}");
        common::assert_no_secret(&errors);
    }
    let env = MapEnvironment::new();
    let errors = Loader::new(&registry, &env)
        .load_str(&format!("[echoing]\nmode = \"{TEST_SECRET}\"\n"))
        .unwrap_err();
    assert!(
        errors.errors()[0]
            .message()
            .contains("unknown variant; expected one of `Single`, `Split`"),
        "{errors}"
    );
    let errors = Loader::new(&registry, &env)
        .load_str(&format!("[echoing]\necho = \"{TEST_SECRET}\"\n"))
        .unwrap_err();
    assert!(
        errors.errors()[0].message().starts_with("invalid value"),
        "{errors}"
    );
}
