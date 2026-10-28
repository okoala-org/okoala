// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! `deserialize_with` functions for secret fields of a [`crate::Section`].
//!
//! ```
//! use okoala_secret::Secret;
//! use serde::Deserialize;
//!
//! #[derive(Debug, Deserialize)]
//! #[serde(deny_unknown_fields)]
//! struct Smtp {
//!     #[serde(deserialize_with = "okoala_config::secret::string")]
//!     relay_password: Secret<String>,
//!     #[serde(default, deserialize_with = "okoala_config::secret::optional_bytes")]
//!     dkim_seed: Option<Secret<Vec<u8>>>,
//! }
//! ```
//!
//! The field's value in the file is a reference, `{ file = "<path>" }` or `{ env = "<VARIABLE>" }`, and the
//! loader reads the secret it names (see the crate documentation for the rules). These functions only work
//! inside the [`crate::Loader`]: through another deserializer (`toml::from_str`, say) they fail, so a section
//! type cannot be loaded in a way that skips the rules. Text secrets lose one trailing newline; byte secrets
//! are taken as they are.

use std::fmt;

use okoala_secret::Secret;
use serde::Deserializer;
use serde::de::{Unexpected, Visitor};

pub(crate) const TEXT_MARKER: &str = "$okoala_config::secret::text";
pub(crate) const BYTES_MARKER: &str = "$okoala_config::secret::bytes";

#[derive(Clone, Copy)]
struct TextVisitor;

impl<'de> Visitor<'de> for TextVisitor {
    type Value = Secret<String>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a secret reference loaded by okoala-config")
    }

    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(Secret::new(value))
    }

    // Only the loader produces secrets, and always as an owned `String`; a borrowed string comes from some other
    // deserializer, so it is refused (without echoing it).
    fn visit_str<E: serde::de::Error>(self, _value: &str) -> Result<Self::Value, E> {
        Err(E::invalid_type(
            Unexpected::Other("an inline string"),
            &self,
        ))
    }
}

#[derive(Clone, Copy)]
struct BytesVisitor;

impl<'de> Visitor<'de> for BytesVisitor {
    type Value = Secret<Vec<u8>>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a secret reference loaded by okoala-config")
    }

    fn visit_byte_buf<E: serde::de::Error>(self, value: Vec<u8>) -> Result<Self::Value, E> {
        Ok(Secret::new(value))
    }
}

struct Optional<V>(V);

impl<'de, V: MarkedVisitor<'de>> Visitor<'de> for Optional<V> {
    type Value = Option<V::Value>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.expecting(f)
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer
            .deserialize_newtype_struct(V::MARKER, self.0)
            .map(Some)
    }
}

trait MarkedVisitor<'de>: Visitor<'de> {
    const MARKER: &'static str;
}

impl MarkedVisitor<'_> for TextVisitor {
    const MARKER: &'static str = TEXT_MARKER;
}

impl MarkedVisitor<'_> for BytesVisitor {
    const MARKER: &'static str = BYTES_MARKER;
}

/// A required text secret.
pub fn string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Secret<String>, D::Error> {
    deserializer.deserialize_newtype_struct(TEXT_MARKER, TextVisitor)
}

/// A required binary secret.
pub fn bytes<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Secret<Vec<u8>>, D::Error> {
    deserializer.deserialize_newtype_struct(BYTES_MARKER, BytesVisitor)
}

/// An optional text secret; use with `#[serde(default)]` so a missing key is `None`.
pub fn optional_string<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Secret<String>>, D::Error> {
    deserializer.deserialize_option(Optional(TextVisitor))
}

/// An optional binary secret; use with `#[serde(default)]` so a missing key is `None`.
pub fn optional_bytes<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Secret<Vec<u8>>>, D::Error> {
    deserializer.deserialize_option(Optional(BytesVisitor))
}
