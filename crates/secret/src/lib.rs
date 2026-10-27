// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! # Okoala Secret
//!
//! `okoala-secret` provides the `Secret<T>` wrapper which helps prevent accidental exposure of sensitive values in debug logs and prints.
//! You can of course still print the secret value by explicity calling `reveal_value()` then printing the result. `Secret<T>` will help
//! reduce accidental exposures, but ultimate responsibility lies with the users of this crate.

use std::fmt;
use zeroize::{Zeroize, ZeroizeOnDrop};

// Secret<T> type
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct Secret<T: Zeroize> {
    value: T,
}

// Secret<T> impl
impl<T: Zeroize> Secret<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }

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
