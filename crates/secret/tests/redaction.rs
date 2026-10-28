// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! A `Secret` must never show its value through formatting, however it is reached:
//! directly, nested in derived `Debug` output, in error messages or in panic messages.

use std::collections::BTreeMap;
use std::fmt;
use std::panic;

use okoala_secret::Secret;

const PASSWORD: &str = "hunter2-correct-horse-battery-staple";

fn assert_redacted(output: &str) {
    assert!(
        !output.contains(PASSWORD),
        "secret leaked into output: {output}"
    );
    assert!(
        output.contains("[REDACTED]"),
        "output does not say it was redacted: {output}"
    );
}

#[test]
fn debug_hides_the_value() {
    let secret = Secret::new(String::from(PASSWORD));
    assert_redacted(&format!("{secret:?}"));
}

#[test]
fn pretty_debug_hides_the_value() {
    let secret = Secret::new(String::from(PASSWORD));
    assert_redacted(&format!("{secret:#?}"));
}

#[test]
fn format_flags_do_not_change_redaction() {
    let secret = Secret::new(String::from(PASSWORD));
    for output in [
        format!("{secret:>80?}"),
        format!("{secret:<80?}"),
        format!("{secret:^80?}"),
        format!("{secret:*^80?}"),
        format!("{secret:.3?}"),
    ] {
        assert_redacted(&output);
    }
}

#[test]
fn reveal_value_returns_the_value() {
    let secret = Secret::new(String::from(PASSWORD));
    assert_eq!(secret.reveal_value(), PASSWORD);
}

#[derive(Debug)]
#[allow(dead_code)]
struct Login {
    user_name: String,
    password: Secret<String>,
}

#[test]
fn derived_debug_of_a_containing_struct_hides_the_value() {
    let login = Login {
        user_name: String::from("user1"),
        password: Secret::new(String::from(PASSWORD)),
    };
    let output = format!("{login:?}");
    assert_redacted(&output);
    assert!(output.contains("user1"), "non-secret fields stay visible");
    assert_redacted(&format!("{login:#?}"));
}

#[test]
fn collections_and_wrappers_hide_the_value() {
    let in_vec = vec![Secret::new(String::from(PASSWORD))];
    let in_option = Some(Secret::new(String::from(PASSWORD)));
    let in_tuple = (1, Secret::new(String::from(PASSWORD)));
    let in_box = Box::new(Secret::new(String::from(PASSWORD)));
    let mut in_map = BTreeMap::new();
    in_map.insert("db", Secret::new(String::from(PASSWORD)));
    let in_result: Result<(), Secret<String>> = Err(Secret::new(String::from(PASSWORD)));

    assert_redacted(&format!("{in_vec:?}"));
    assert_redacted(&format!("{in_option:?}"));
    assert_redacted(&format!("{in_tuple:?}"));
    assert_redacted(&format!("{in_box:?}"));
    assert_redacted(&format!("{in_map:?}"));
    assert_redacted(&format!("{in_result:?}"));
}

#[test]
fn byte_secrets_are_hidden_too() {
    let bytes: Vec<u8> = vec![0xde, 0xad, 0xbe, 0xef];
    let secret = Secret::new(bytes.clone());
    let output = format!("{secret:?}");
    assert!(output.contains("[REDACTED]"));
    assert!(
        !output.contains(&format!("{bytes:?}")),
        "bytes leaked: {output}"
    );
    assert!(!output.contains("222"), "first byte leaked: {output}");

    let array = Secret::new([7u8; 32]);
    let output = format!("{array:?}");
    assert!(output.contains("[REDACTED]"));
    assert!(!output.contains("7, 7"), "array leaked: {output}");
}

#[test]
fn every_allowed_value_type_is_redacted() {
    let outputs = [
        format!("{:?}", Secret::new(String::from(PASSWORD))),
        format!("{:?}", Secret::new(Box::<str>::from(PASSWORD))),
        format!("{:?}", Secret::new(PASSWORD.as_bytes().to_vec())),
        format!("{:?}", Secret::new(Box::<[u8]>::from(PASSWORD.as_bytes()))),
    ];
    for output in outputs {
        assert_redacted(&output);
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct ConnectError {
    url: String,
    password: Secret<String>,
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot connect to {} with {:?}", self.url, self.password)
    }
}

impl std::error::Error for ConnectError {}

#[test]
fn error_messages_hide_the_value() {
    let error = ConnectError {
        url: String::from("postgres://okoala@db/okoala"),
        password: Secret::new(String::from(PASSWORD)),
    };
    assert_redacted(&error.to_string());
    assert_redacted(&format!("{error:?}"));

    let boxed: Box<dyn std::error::Error> = Box::new(error);
    assert_redacted(&boxed.to_string());
    assert_redacted(&format!("{boxed:?}"));
}

#[test]
fn panic_messages_hide_the_value() {
    let secret = Secret::new(String::from(PASSWORD));
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        panic!("login failed for {secret:?}");
    }));
    panic::set_hook(previous_hook);

    let payload = result.expect_err("the closure panics");
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert_redacted(&message);
}
