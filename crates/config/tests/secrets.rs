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
        (b"value", "value"),
        (b" spaced value ", " spaced value "),
    ];
    for (i, (contents, expected)) in cases.iter().enumerate() {
        let path = secret_file(&dir, &format!("token-{i}"), contents, 0o400);
        assert_eq!(token(&load_token_file(&path, &k8s).unwrap()), *expected);
    }
}

#[test]
fn multi_line_text_secrets_are_rejected() {
    let dir = temp_dir("multi-line");
    for (i, contents) in [
        &b"value\n\n"[..],
        b"line one\nline two",
        b"value\r",
        b"a\rb\n",
        b"\nvalue",
    ]
    .iter()
    .enumerate()
    {
        let path = secret_file(&dir, &format!("token-{i}"), contents, 0o400);
        let errors = load_token_file(&path, &dir).unwrap_err();
        let error = &errors.errors()[0];
        assert_eq!(error.key(), Some("token"));
        assert!(error.message().contains("contains a line break"), "{error}");
    }
    // Byte secrets are exempt.
    let token = secret_file(&dir, "token", TEST_SECRET.as_bytes(), 0o400);
    let seed = secret_file(&dir, "seed", b"a\nb\r\n\n", 0o400);
    let text = format!(
        "{HTTP}[internal]\ntoken = {{ secret_file = '{}' }}\nseed = {{ secret_file = '{}' }}\n",
        token.display(),
        seed.display()
    );
    let config = load_with(&text, &MapEnvironment::new(), &dir).unwrap();
    let internal = config.get::<Internal>().unwrap();
    assert_eq!(
        internal.seed.as_ref().unwrap().reveal_value(),
        b"a\nb\r\n\n"
    );
}

#[test]
fn empty_and_blank_secrets_are_rejected() {
    let dir = temp_dir("empty");
    let k8s = temp_dir("empty-k8s");
    for (i, contents) in [&b""[..], b"\n", b"\r\n", b"   ", b" \t \n"]
        .iter()
        .enumerate()
    {
        let path = secret_file(&dir, &format!("token-{i}"), contents, 0o400);
        let errors = load_token_file(&path, &k8s).unwrap_err();
        let error = &errors.errors()[0];
        assert_eq!(error.key(), Some("token"));
        let message = error.message();
        assert!(
            message.ends_with("is empty") || message.ends_with("is empty or only whitespace"),
            "{error}"
        );
    }
    let seed = secret_file(&dir, "seed", b"", 0o400);
    let token = secret_file(&dir, "token", TEST_SECRET.as_bytes(), 0o400);
    let text = format!(
        "{HTTP}[internal]\ntoken = {{ secret_file = '{}' }}\nseed = {{ secret_file = '{}' }}\n",
        token.display(),
        seed.display()
    );
    let errors = load_with(&text, &MapEnvironment::new(), &k8s).unwrap_err();
    assert_eq!(errors.errors()[0].key(), Some("seed"), "{errors}");
    assert!(
        errors.errors()[0].message().ends_with("is empty"),
        "{errors}"
    );

    let env = MapEnvironment::new().with("TOKEN", "");
    let text = format!("{HTTP}[internal]\ntoken = {{ secret_env = \"TOKEN\" }}\n");
    let errors = load_with(&text, &env, &k8s).unwrap_err();
    assert!(
        errors
            .to_string()
            .contains("environment variable TOKEN is empty"),
        "{errors}"
    );
}

#[cfg(unix)]
#[test]
fn fifo_is_rejected_without_blocking() {
    let dir = temp_dir("fifo");
    let k8s = temp_dir("fifo-k8s");
    for (base, mode) in [(&dir, 0o400), (&k8s, 0o444)] {
        let path = base.join("token");
        let status = std::process::Command::new("mkfifo")
            .arg("-m")
            .arg(format!("{mode:o}"))
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        // Run the load on another thread so a regression shows as a failure, not a hung test run.
        let (send, receive) = std::sync::mpsc::channel();
        let k8s_dir = k8s.clone();
        std::thread::spawn(move || {
            let result = load_token_file(&path, &k8s_dir).map(|_| ());
            let _ = send.send(result.map_err(|e| e.to_string()));
        });
        let result = receive
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("loading a FIFO must not block");
        let error = result.unwrap_err();
        assert!(error.contains("is not a regular file"), "{error}");
    }
}

#[test]
fn secret_variable_with_an_override_name_is_rejected() {
    let dir = temp_dir("override-name");
    let env = MapEnvironment::new().with("OKOALA_INTERNAL__TOKEN", TEST_SECRET);
    let text = format!("{HTTP}[internal]\ntoken = {{ secret_env = \"OKOALA_INTERNAL__TOKEN\" }}\n");
    let errors = load_with(&text, &env, &dir).unwrap_err();
    assert_eq!(errors.len(), 1, "{errors}");
    let error = &errors.errors()[0];
    assert_eq!(error.key(), Some("token"));
    assert!(
        error
            .message()
            .contains("has the form of a configuration override"),
        "{error}"
    );
    assert_no_secret(&errors);
}

#[test]
fn secret_errors_are_reported_next_to_a_structural_error() {
    let dir = temp_dir("structural-and-secret");
    let text = format!(
        "{HTTP}[internal]\naaa = 1\ntoken = {{ secret_file = '/nonexistent' }}\n\
         seed = {{ secret_env = \"MISSING\" }}\n"
    );
    let errors = load_with(&text, &MapEnvironment::new(), &dir).unwrap_err();
    let rendered = errors.to_string();
    assert_eq!(errors.len(), 3, "{rendered}");
    assert!(
        rendered.contains("[internal] aaa: unknown field `aaa`"),
        "{rendered}"
    );
    assert!(
        rendered.contains("[internal] token: cannot open secret file /nonexistent"),
        "{rendered}"
    );
    assert!(
        rendered.contains("[internal] seed: environment variable MISSING is not set"),
        "{rendered}"
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
        "{HTTP}[internal]\ntoken = {{ secret_file = '{}' }}\nseed = {{ secret_file = '{}' }}\n",
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
        "{HTTP}[internal]\ntoken = {{ secret_env = \"OKOALA_INTERNAL_TOKEN\" }}\nhook_token = {{ secret_env = \"HOOK\" }}\n"
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
    let text = format!("{HTTP}[internal]\ntoken = {{ secret_env = \"NOT_SET\" }}\n");
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
        format!("{{ secret_file = \"/a\", secret_env = \"{TEST_SECRET}\" }}"),
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
                .contains("{ secret_file = \"<path>\" } or { secret_env = \"<VARIABLE>\" }"),
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
        "{HTTP}[internal]\ntoken = {{ secret_file = '{}' }}\nhook_token = {{ secret_env = \"MISSING\" }}\nseed = {{ secret_file = '/nonexistent' }}\n",
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

    // serde's own value deserializers do not get through either, and their errors do not echo the value.
    use serde::de::IntoDeserializer;
    use serde::de::value::{Error as ValueError, StringDeserializer};
    let plain: StringDeserializer<ValueError> = TEST_SECRET.to_owned().into_deserializer();
    let error = okoala_config::secret::string(plain)
        .unwrap_err()
        .to_string();
    assert!(!error.contains(TEST_SECRET), "{error}");
    let plain: StringDeserializer<ValueError> = TEST_SECRET.to_owned().into_deserializer();
    assert!(okoala_config::secret::bytes(plain).is_err());
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
