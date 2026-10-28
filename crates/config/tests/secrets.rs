// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Secret references: file modes, environment variables, trailing newlines, empty and inline secrets, and that
//! no error or `Debug` output contains a secret.

// Helpers outside `#[test]` functions unwrap too; a failure there is a test failure.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;

use common::{Internal, TEST_SECRET, assert_no_secret, registry, secret_file, temp_dir};
use okoala_config::{Config, ConfigErrors, Loader, MapEnvironment};

const HTTP: &str = "[http]\nhostname = \"mail.example.com\"\n\n";

fn load_with(
    text: &str,
    env: &MapEnvironment,
    kubernetes_dir: &Path,
) -> Result<Config, ConfigErrors> {
    let registry = registry();
    Loader::new(&registry, env)
        .with_kubernetes_secrets_dir(kubernetes_dir)
        .load_str(text)
}

fn load_token_file(path: &Path, kubernetes_dir: &Path) -> Result<Config, ConfigErrors> {
    load_with(
        &common::config_with_token(path),
        &MapEnvironment::new(),
        kubernetes_dir,
    )
}

fn token(config: &Config) -> &str {
    config.get::<Internal>().unwrap().token.reveal_value()
}

#[test]
fn accepted_modes() {
    let dir = temp_dir("accepted-modes");
    let elsewhere = temp_dir("accepted-modes-k8s");
    for mode in [0o400, 0o440] {
        let path = secret_file(
            &dir,
            &format!("token-{mode:o}"),
            TEST_SECRET.as_bytes(),
            mode,
        );
        let config = load_token_file(&path, &elsewhere).unwrap();
        assert_eq!(token(&config), TEST_SECRET);
    }
}

#[cfg(unix)]
#[test]
fn rejected_modes_name_the_file_and_mode() {
    let dir = temp_dir("rejected-modes");
    let elsewhere = temp_dir("rejected-modes-k8s");
    for mode in [0o600, 0o640, 0o644, 0o444, 0o404, 0o4400, 0o000, 0o777] {
        let path = secret_file(
            &dir,
            &format!("token-{mode:o}"),
            TEST_SECRET.as_bytes(),
            mode,
        );
        let errors = load_token_file(&path, &elsewhere).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors}");
        let error = &errors.errors()[0];
        assert_eq!(error.section(), Some("internal"));
        assert_eq!(error.key(), Some("token"));
        let message = error.message();
        assert!(message.contains(&path.display().to_string()), "{message}");
        // Mode 0000 cannot be opened by a non-root user, so the open fails before the mode check.
        if mode != 0o000 || message.contains("has mode") {
            assert!(message.contains(&format!("mode {mode:04o}")), "{message}");
        }
        assert_no_secret(&errors);
    }
}

#[cfg(unix)]
#[test]
fn world_readable_is_accepted_only_under_the_kubernetes_dir() {
    let k8s = temp_dir("k8s-dir");
    let path = secret_file(&k8s, "token", TEST_SECRET.as_bytes(), 0o444);
    let config = load_token_file(&path, &k8s).unwrap();
    assert_eq!(token(&config), TEST_SECRET);

    // Kubernetes mounts each key as a symlink into a timestamped directory; resolved, it stays under the mount.
    let data = k8s.join("..2026_10_28");
    std::fs::create_dir_all(&data).unwrap();
    let target = secret_file(&data, "linked", TEST_SECRET.as_bytes(), 0o444);
    let link = k8s.join("linked");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert_eq!(token(&load_token_file(&link, &k8s).unwrap()), TEST_SECRET);
}

#[cfg(unix)]
#[test]
fn symlink_from_the_kubernetes_dir_to_elsewhere_is_not_trusted() {
    let k8s = temp_dir("k8s-escape");
    let outside = temp_dir("k8s-escape-outside");
    let target = secret_file(&outside, "token", TEST_SECRET.as_bytes(), 0o444);
    let link = k8s.join("token");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let errors = load_token_file(&link, &k8s).unwrap_err();
    assert!(errors.to_string().contains("has mode 0444"), "{errors}");
    assert_no_secret(&errors);
}

#[test]
fn one_trailing_newline_is_stripped() {
    let dir = temp_dir("newline");
    let k8s = temp_dir("newline-k8s");
    let cases: [(&[u8], &str); 4] = [
        (b"value\n", "value"),
        (b"value\r\n", "value"),
        (b"value\n\n", "value\n"),
        (b"value", "value"),
    ];
    for (i, (contents, expected)) in cases.iter().enumerate() {
        let path = secret_file(&dir, &format!("token-{i}"), contents, 0o400);
        assert_eq!(token(&load_token_file(&path, &k8s).unwrap()), *expected);
    }
}

#[test]
fn empty_secrets_are_rejected() {
    let dir = temp_dir("empty");
    let k8s = temp_dir("empty-k8s");
    for (i, contents) in [&b""[..], b"\n", b"\r\n"].iter().enumerate() {
        let path = secret_file(&dir, &format!("token-{i}"), contents, 0o400);
        let errors = load_token_file(&path, &k8s).unwrap_err();
        let error = &errors.errors()[0];
        assert_eq!(error.key(), Some("token"));
        assert!(error.message().ends_with("is empty"), "{error}");
    }
    let env = MapEnvironment::new().with("TOKEN", "");
    let text = format!("{HTTP}[internal]\ntoken = {{ env = \"TOKEN\" }}\n");
    let errors = load_with(&text, &env, &k8s).unwrap_err();
    assert!(
        errors
            .to_string()
            .contains("environment variable TOKEN is empty"),
        "{errors}"
    );
}

#[test]
fn invalid_utf8_text_secret_is_rejected() {
    let dir = temp_dir("utf8");
    let path = secret_file(&dir, "token", &[0xff, 0xfe, b'a'], 0o400);
    let errors = load_token_file(&path, &dir).unwrap_err();
    assert!(
        errors.to_string().contains("is not valid UTF-8"),
        "{errors}"
    );
}

#[test]
fn byte_secrets_are_taken_as_they_are() {
    let dir = temp_dir("bytes");
    let token = secret_file(&dir, "token", TEST_SECRET.as_bytes(), 0o400);
    let seed = secret_file(&dir, "seed", &[0, 1, 0xff, b'\n'], 0o440);
    let text = format!(
        "{HTTP}[internal]\ntoken = {{ file = '{}' }}\nseed = {{ file = '{}' }}\n",
        token.display(),
        seed.display()
    );
    let config = load_with(&text, &MapEnvironment::new(), &dir).unwrap();
    let internal = config.get::<Internal>().unwrap();
    assert_eq!(
        internal.seed.as_ref().unwrap().reveal_value(),
        &[0, 1, 0xff, b'\n']
    );
    assert!(internal.hook_token.is_none());
}

#[test]
fn secret_from_an_environment_variable() {
    let dir = temp_dir("env-secret");
    let env = MapEnvironment::new()
        .with("OKOALA_INTERNAL_TOKEN", format!("{TEST_SECRET}\n"))
        .with("HOOK", "hook-value");
    let text = format!(
        "{HTTP}[internal]\ntoken = {{ env = \"OKOALA_INTERNAL_TOKEN\" }}\nhook_token = {{ env = \"HOOK\" }}\n"
    );
    let config = load_with(&text, &env, &dir).unwrap();
    assert_eq!(token(&config), TEST_SECRET);
    let internal = config.get::<Internal>().unwrap();
    assert_eq!(
        internal.hook_token.as_ref().unwrap().reveal_value(),
        "hook-value"
    );
}

#[test]
fn unset_environment_variable_is_an_error() {
    let dir = temp_dir("env-unset");
    let text = format!("{HTTP}[internal]\ntoken = {{ env = \"NOT_SET\" }}\n");
    let errors = load_with(&text, &MapEnvironment::new(), &dir).unwrap_err();
    assert!(
        errors
            .to_string()
            .contains("[internal] token: environment variable NOT_SET is not set"),
        "{errors}"
    );
}

#[test]
fn inline_and_malformed_references_are_rejected_without_echo() {
    let dir = temp_dir("inline");
    for reference in [
        format!("\"{TEST_SECRET}\""),
        format!("{{ value = \"{TEST_SECRET}\" }}"),
        format!("{{ file = \"/a\", env = \"{TEST_SECRET}\" }}"),
        "42".to_owned(),
        format!("[\"{TEST_SECRET}\"]"),
    ] {
        let text = format!("{HTTP}[internal]\ntoken = {reference}\n");
        let errors = load_with(&text, &MapEnvironment::new(), &dir).unwrap_err();
        let error = &errors.errors()[0];
        assert_eq!(error.key(), Some("token"), "{error}");
        assert!(
            error
                .message()
                .contains("{ file = \"<path>\" } or { env = \"<VARIABLE>\" }"),
            "{error}"
        );
        assert_no_secret(&errors);
    }
}

#[test]
fn every_secret_error_is_reported() {
    let dir = temp_dir("all-secret-errors");
    let bad = secret_file(&dir, "bad", TEST_SECRET.as_bytes(), 0o644);
    let text = format!(
        "{HTTP}[internal]\ntoken = {{ file = '{}' }}\nhook_token = {{ env = \"MISSING\" }}\nseed = {{ file = '/nonexistent' }}\n",
        bad.display()
    );
    let errors = load_with(&text, &MapEnvironment::new(), &dir).unwrap_err();
    assert_eq!(errors.len(), 3, "{errors}");
    let keys: Vec<_> = errors.errors().iter().map(|e| e.key()).collect();
    assert!(
        keys.contains(&Some("token"))
            && keys.contains(&Some("hook_token"))
            && keys.contains(&Some("seed"))
    );
    assert_no_secret(&errors);
}

#[test]
fn secret_fields_cannot_be_deserialized_outside_the_loader() {
    let result = toml::from_str::<Internal>(&format!("token = \"{TEST_SECRET}\""));
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("a secret reference loaded by okoala-config"),
        "{error}"
    );
}

#[test]
fn debug_output_never_shows_the_secret() {
    let dir = temp_dir("debug");
    let path = secret_file(&dir, "token", TEST_SECRET.as_bytes(), 0o400);
    let config = load_token_file(&path, &dir).unwrap();
    let env = MapEnvironment::new().with("X", TEST_SECRET);
    let registry = registry();
    let loader = Loader::new(&registry, &env);
    let all = format!(
        "{config:?} {:?} {:#?} {env:?} {loader:?}",
        config.get::<Internal>().unwrap(),
        config.get::<Internal>().unwrap()
    );
    assert!(!all.contains(TEST_SECRET), "{all}");
    assert!(all.contains("[REDACTED]"), "{all}");
}
