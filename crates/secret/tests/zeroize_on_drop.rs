// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! `Secret` zeroizes its value in `Drop`. Since `Secret` accepts only a sealed set of value
//! types, a test can no longer wrap a probe that counts `zeroize` calls, and reading freed
//! memory would need `unsafe`, which the workspace forbids. These tests check what can
//! still be checked from outside: that `Secret` has drop glue of its own and that every
//! allowed value type survives the drop paths.

use std::mem;
use std::panic;

use okoala_secret::Secret;

/// `[u8; N]` needs no drop on its own, so `Secret<[u8; N]>` needing one proves that
/// `Secret` has a `Drop` impl (the one that zeroizes).
#[test]
fn secret_has_its_own_drop() {
    assert!(!mem::needs_drop::<[u8; 32]>());
    assert!(mem::needs_drop::<Secret<[u8; 32]>>());
    assert!(mem::needs_drop::<Secret<[u8; 0]>>());
    assert!(mem::needs_drop::<Secret<String>>());
    assert!(mem::needs_drop::<Secret<Vec<u8>>>());
    assert!(mem::needs_drop::<Secret<Box<str>>>());
    assert!(mem::needs_drop::<Secret<Box<[u8]>>>());
}

#[test]
fn every_allowed_value_type_can_be_dropped() {
    drop(Secret::new(String::from("password")));
    drop(Secret::new(String::new()));
    drop(Secret::new(vec![1u8, 2, 3]));
    drop(Secret::new(Vec::<u8>::with_capacity(64)));
    drop(Secret::new(Box::<str>::from("password")));
    drop(Secret::new(Box::<[u8]>::from(&b"password"[..])));
    drop(Secret::new([7u8; 32]));
    drop(Secret::new([0u8; 0]));
    let mut grown = String::with_capacity(4);
    grown.push_str("a value longer than the first capacity");
    drop(Secret::new(grown));
}

#[test]
fn secrets_in_collections_and_options_can_be_dropped_and_replaced() {
    let secrets: Vec<_> = (0..5).map(|i| Secret::new(format!("token-{i}"))).collect();
    drop(secrets);

    let mut slot = Some(Secret::new(String::from("first")));
    assert!(slot.is_some());
    slot = Some(Secret::new(String::from("second")));
    assert!(slot.as_ref().is_some_and(|s| s.reveal_value() == "second"));
    drop(slot);
}

#[test]
fn unwinding_from_a_panic_drops_the_secret_cleanly() {
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(|| {
        let _secret = Secret::new(String::from("password"));
        panic!("request handler failed");
    });
    panic::set_hook(previous_hook);
    assert!(result.is_err());
}

#[test]
fn moving_a_secret_keeps_its_value() {
    let secret = Secret::new(String::from("password"));
    let moved = secret;
    let boxed = Box::new(moved);
    assert_eq!(boxed.reveal_value(), "password");
}
