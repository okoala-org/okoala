// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Compile-time checks of which traits `Secret` has. Each trait it must not have would
//! open a way for the value to escape; if one is added, this file stops compiling and
//! the change has to be a deliberate decision. (`Serialize` needs no check: the crate
//! does not depend on serde.)

use std::borrow::Borrow;
use std::fmt::Display;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};

use okoala_secret::{Secret, ValidSecretT};

/// Fails to compile if `$ty` implements `$tr`: with the impl, the call below has two
/// candidate impls and type inference cannot choose between them.
macro_rules! assert_not_impl {
    ($ty:ty: $tr:path) => {
        const _: fn() = || {
            trait AmbiguousIfImpl<A> {
                fn some_item() {}
            }
            impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
            #[allow(dead_code)]
            struct Invalid;
            impl<T: ?Sized + $tr> AmbiguousIfImpl<Invalid> for T {}
            let _ = <$ty as AmbiguousIfImpl<_>>::some_item;
        };
    };
}

// No operators for comparing: comparisons go through the explicit, constant-time
// `compare` and `compare_value`, so they stand out in review.
assert_not_impl!(Secret<String>: PartialEq);
assert_not_impl!(Secret<String>: PartialEq<str>);
assert_not_impl!(Secret<String>: PartialEq<&'static str>);
assert_not_impl!(Secret<Vec<u8>>: PartialEq<[u8]>);
assert_not_impl!(Secret<String>: PartialOrd);

// No way to print the value except `Debug`, which redacts.
assert_not_impl!(Secret<String>: Display);

// No silent copies of the value.
assert_not_impl!(Secret<String>: Clone);
assert_not_impl!(Secret<Vec<u8>>: Clone);

// No hashing: the hasher would receive the raw value.
assert_not_impl!(Secret<String>: Hash);
assert_not_impl!(Secret<Vec<u8>>: Hash);

// No implicit access to the value; `reveal_value()` is the only way in.
assert_not_impl!(Secret<String>: Deref);
assert_not_impl!(Secret<String>: DerefMut);
assert_not_impl!(Secret<String>: AsRef<str>);
assert_not_impl!(Secret<String>: AsRef<[u8]>);
assert_not_impl!(Secret<Vec<u8>>: AsRef<[u8]>);
assert_not_impl!(Secret<String>: Borrow<str>);
assert_not_impl!(Secret<String>: Borrow<String>);

// No implicit conversion into a secret (the copy from `&str` was removed on purpose).
assert_not_impl!(Secret<String>: From<&'static str>);

// Review finding 2: only a closed set of value types can be secrets, so no outside type
// can bring a misbehaving `AsRef` or `Zeroize`.
assert_not_impl!(u64: ValidSecretT);
assert_not_impl!(char: ValidSecretT);
assert_not_impl!(Vec<u16>: ValidSecretT);
assert_not_impl!(Vec<char>: ValidSecretT);
assert_not_impl!(Box<String>: ValidSecretT);
assert_not_impl!(Option<String>: ValidSecretT);
assert_not_impl!([u16; 4]: ValidSecretT);

fn assert_valid<T: ValidSecretT>() {}

/// The value types `Secret` is meant for.
#[test]
fn allowed_value_types() {
    assert_valid::<String>();
    assert_valid::<Box<str>>();
    assert_valid::<Vec<u8>>();
    assert_valid::<Box<[u8]>>();
    assert_valid::<[u8; 0]>();
    assert_valid::<[u8; 32]>();
}

fn assert_send_sync<T: Send + Sync>() {}

/// Secrets are held in shared configuration and moved across async tasks, so they must
/// stay `Send` and `Sync` when the value is.
#[test]
fn secrets_are_send_and_sync() {
    assert_send_sync::<Secret<String>>();
    assert_send_sync::<Secret<Vec<u8>>>();
    assert_send_sync::<Secret<[u8; 32]>>();
    assert_send_sync::<Secret<Box<str>>>();
    assert_send_sync::<Secret<Box<[u8]>>>();
}
