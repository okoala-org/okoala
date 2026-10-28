// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! # Okoala Secret
//!
//! `okoala-secret` provides the `Secret<T>` wrapper which helps prevent accidental exposure of sensitive
//! values in debug logs and prints. You can of course still print the secret value by explicitly calling
//! `reveal_value()` then printing the result. `Secret<T>` will help reduce accidental exposures, but ultimate
//! responsibility lies with the users of this crate.

use std::fmt;
use std::hash::{Hash, Hasher};
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

/// Holds a secret value and helps prevent it from being accidentaly exposed
pub struct Secret<T: Zeroize> {
    value: T,
}

impl<T: Zeroize> Secret<T> {
    /// Creates a new Secret
    pub fn new(value: T) -> Self {
        Self { value }
    }

    /// Reveals the secret value. Use responsibly.
    pub fn reveal_value(&self) -> &T {
        &self.value
    }
}

// Debug impl that redacts secret value
impl<T: Zeroize> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secret")
            .field("value", &"[REDACTED]")
            .finish()
    }
}

// Zero the value memory on drop
impl<T: Zeroize> Drop for Secret<T> {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

// Constant time compare for Secret<T>
impl<T: Zeroize + AsRef<[u8]>> PartialEq for Secret<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value.as_ref().ct_eq(&other.value.as_ref()).into()
    }
}

impl<T: Zeroize + AsRef<[u8]>> Eq for Secret<T> {}

impl<T: Zeroize + Hash> Hash for Secret<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}
