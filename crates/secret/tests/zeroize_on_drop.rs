// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Dropping a `Secret` must zeroize its value exactly once, on every path that runs the
//! drop. Reading freed memory would need `unsafe`, which the workspace forbids, so these
//! tests use a probe value that counts how often `zeroize` is called on it.

use std::cell::Cell;
use std::mem;
use std::panic;
use std::rc::Rc;

use okoala_secret::Secret;
use zeroize::Zeroize;

/// Counts calls to `zeroize`; the counter is shared so the test can read it after the
/// probe is gone.
struct Probe {
    zeroized: Rc<Cell<u32>>,
}

impl Zeroize for Probe {
    fn zeroize(&mut self) {
        self.zeroized.set(self.zeroized.get() + 1);
    }
}

fn probe() -> (Probe, Rc<Cell<u32>>) {
    let zeroized = Rc::new(Cell::new(0));
    (
        Probe {
            zeroized: Rc::clone(&zeroized),
        },
        zeroized,
    )
}

#[test]
fn explicit_drop_zeroizes_once() {
    let (value, zeroized) = probe();
    let secret = Secret::new(value);
    assert_eq!(zeroized.get(), 0, "not zeroized while alive");
    drop(secret);
    assert_eq!(zeroized.get(), 1);
}

#[test]
fn leaving_scope_zeroizes_once() {
    let (value, zeroized) = probe();
    {
        let _secret = Secret::new(value);
    }
    assert_eq!(zeroized.get(), 1);
}

#[test]
fn reveal_value_does_not_zeroize() {
    let (value, zeroized) = probe();
    let secret = Secret::new(value);
    let _ = secret.reveal_value();
    let _ = secret.reveal_value();
    assert_eq!(zeroized.get(), 0);
    drop(secret);
    assert_eq!(zeroized.get(), 1);
}

#[test]
fn moving_a_secret_does_not_zeroize_it_early() {
    let (value, zeroized) = probe();
    let secret = Secret::new(value);
    let moved = secret;
    let boxed = Box::new(moved);
    assert_eq!(zeroized.get(), 0);
    drop(boxed);
    assert_eq!(zeroized.get(), 1);
}

#[test]
fn dropping_a_collection_zeroizes_every_secret() {
    let probes: Vec<_> = (0..5).map(|_| probe()).collect();
    let counters: Vec<_> = probes.iter().map(|(_, c)| Rc::clone(c)).collect();
    let secrets: Vec<_> = probes.into_iter().map(|(p, _)| Secret::new(p)).collect();
    drop(secrets);
    for counter in counters {
        assert_eq!(counter.get(), 1);
    }
}

#[test]
fn replacing_a_secret_zeroizes_the_old_one() {
    let (first, first_zeroized) = probe();
    let (second, second_zeroized) = probe();
    let mut slot = Some(Secret::new(first));
    assert!(slot.is_some());
    slot = Some(Secret::new(second));
    assert_eq!(first_zeroized.get(), 1, "old value zeroized on overwrite");
    assert_eq!(second_zeroized.get(), 0);
    assert!(slot.is_some());
    drop(slot);
    assert_eq!(second_zeroized.get(), 1);
}

#[test]
fn unwinding_from_a_panic_zeroizes() {
    let (value, zeroized) = probe();
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(panic::AssertUnwindSafe(move || {
        let _secret = Secret::new(value);
        panic!("request handler failed");
    }));
    panic::set_hook(previous_hook);
    assert!(result.is_err());
    assert_eq!(zeroized.get(), 1);
}

/// Documents a limit rather than a feature: zeroizing happens in `Drop`, so a secret that
/// is never dropped (`mem::forget`, `Box::leak`, reference cycles) is never zeroized.
#[test]
fn forgetting_a_secret_skips_zeroizing() {
    let (value, zeroized) = probe();
    mem::forget(Secret::new(value));
    assert_eq!(zeroized.get(), 0);
}

#[test]
fn real_value_types_can_be_wrapped_and_dropped() {
    drop(Secret::new(String::from("password")));
    drop(Secret::new(String::new()));
    drop(Secret::new(vec![1u8, 2, 3]));
    drop(Secret::new([0u8; 32]));
    let mut grown = String::with_capacity(4);
    grown.push_str("a value longer than the first capacity");
    drop(Secret::new(grown));
}
