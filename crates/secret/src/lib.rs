// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! # Okoala Secret
//!
//! `okoala-secret` provides the `Secret<T>` wrapper which helps prevent accidental exposure of sensitive
//! values in debug logs and prints. You can of course still print the secret value by explicitly calling
//! `reveal_value()` then printing the result. `Secret<T>` will help reduce accidental exposures, but ultimate
//! responsibility lies with the users of this crate.

use std::fmt;
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

mod __internal {
    pub trait Sealed {}
}

pub trait ValidSecretT: __internal::Sealed + Zeroize {
    type RevealedT: ?Sized;

    fn bytes(&self) -> &[u8];

    fn reveal(&self) -> &Self::RevealedT;
}

impl __internal::Sealed for String {}
impl ValidSecretT for String {
    type RevealedT = str;

    fn bytes(&self) -> &[u8] {
        self.as_bytes()
    }

    fn reveal(&self) -> &Self::RevealedT {
        self.as_str()
    }
}

impl __internal::Sealed for Vec<u8> {}
impl ValidSecretT for Vec<u8> {
    type RevealedT = [u8];

    fn bytes(&self) -> &[u8] {
        self.as_slice()
    }

    fn reveal(&self) -> &Self::RevealedT {
        self.as_slice()
    }
}

impl __internal::Sealed for Box<[u8]> {}
impl ValidSecretT for Box<[u8]> {
    type RevealedT = [u8];

    fn bytes(&self) -> &[u8] {
        self    
    }

    fn reveal(&self) -> &Self::RevealedT {
        self
    }
}

impl __internal::Sealed for Box<str> {}
impl ValidSecretT for Box<str> {
    type RevealedT = str;

    fn bytes(&self) -> &[u8] {
        self.as_bytes()
    }

    fn reveal(&self) -> &Self::RevealedT {
        self
    }
}

impl<const N: usize> __internal::Sealed for [u8; N] {}
impl<const N: usize> ValidSecretT for [u8; N] {
    type RevealedT = [u8];

    fn bytes(&self) -> &[u8] {
        &self[..]
    }

    fn reveal(&self) -> &Self::RevealedT {
        &self[..]
    }
}

/// Holds a secret value and helps prevent it from being accidentally exposed
pub struct Secret<T: ValidSecretT> {
    value: T,
}

impl<T: ValidSecretT> Secret<T> {
    /// Creates a new Secret
    pub fn new(value: T) -> Self {
        Self { value }
    }

    /// Reveals the secret value. Use responsibly.
    pub fn reveal_value(&self) -> &T::RevealedT {
        self.value.reveal()
    }
}

impl<T: ValidSecretT> Secret<T> {
    /// Constant time compare between `Secret<T>`s
    /// [IMPORTANT]: when lengths are different, this returns immediately (not constant time)
    #[must_use]
    pub fn compare(&self, other: &Secret<T>) -> bool {
        if self.value.bytes().is_empty() {
            return false;
        }

        self.value.bytes().ct_eq(other.value.bytes()).into()
    }

    /// Constant time compare between `Secret<T>` and `T`
    /// [IMPORTANT]: when lengths are different, this returns immediately (not constant time)
    #[must_use]
    pub fn compare_value(&self, other: &T) -> bool {
        if self.value.bytes().is_empty() {
            return false;
        }

        self.value.bytes().ct_eq(other.bytes()).into()
    }

    /// Constant time compare between `Secret<T>` and bytes
    /// [IMPORTANT]: when lengths are different, this returns immediately (not constant time)
    #[must_use]
    pub fn compare_bytes(&self, other: &[u8]) -> bool {
        if self.value.bytes().is_empty() {
            return false;
        }

        self.value.bytes().ct_eq(other).into()
    }
}

// Debug impl that redacts secret value
impl<T: ValidSecretT> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secret")
            .field("value", &"[REDACTED]")
            .finish()
    }
}

// Zero the value memory on drop
impl<T: ValidSecretT> Drop for Secret<T> {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}
