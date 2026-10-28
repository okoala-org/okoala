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
//! The field's value in the file is a reference, `{ secret_file = "<path>" }` or
//! `{ secret_env = "<VARIABLE>" }`, and the loader reads the secret it names (see the crate documentation for
//! the rules). The loader hands the value over in a private form (a newtype variant with a reserved name) that
//! these functions require, so a plain string from another deserializer is refused: `toml::from_str` and
//! `serde`'s own value deserializers cannot load a secret field and skip the rules. Only a deserializer built
//! to imitate that form could, which is a deliberate act, not a mistake.
//!
//! Text secrets lose one trailing newline and must then be one non-blank line; byte secrets are taken as they
//! are.

use std::fmt;
use std::marker::PhantomData;

use okoala_secret::Secret;
use serde::Deserializer;
use serde::de::{self, DeserializeSeed, EnumAccess, Unexpected, VariantAccess, Visitor};

pub(crate) const TEXT_MARKER: &str = "$okoala_config::secret::text";
pub(crate) const BYTES_MARKER: &str = "$okoala_config::secret::bytes";
/// The variant name the loader hands secrets over with.
pub(crate) const LOADED_VARIANT: &str = "$okoala_config::secret::loaded";

const EXPECTING: &str = "a secret reference loaded by okoala-config";

/// The two secret types a field can have.
trait SecretValue: Sized {
    const MARKER: &'static str;

    fn from_string<E: de::Error>(value: String) -> Result<Self, E>;

    fn from_bytes<E: de::Error>(value: Vec<u8>) -> Result<Self, E>;
}

impl SecretValue for Secret<String> {
    const MARKER: &'static str = TEXT_MARKER;

    fn from_string<E: de::Error>(value: String) -> Result<Self, E> {
        Ok(Secret::new(value))
    }

    fn from_bytes<E: de::Error>(value: Vec<u8>) -> Result<Self, E> {
        drop(Secret::new(value));
        Err(E::invalid_type(Unexpected::Other("bytes"), &EXPECTING))
    }
}

impl SecretValue for Secret<Vec<u8>> {
    const MARKER: &'static str = BYTES_MARKER;

    fn from_string<E: de::Error>(value: String) -> Result<Self, E> {
        drop(Secret::new(value));
        Err(E::invalid_type(Unexpected::Other("a string"), &EXPECTING))
    }

    fn from_bytes<E: de::Error>(value: Vec<u8>) -> Result<Self, E> {
        Ok(Secret::new(value))
    }
}

/// Accepts only the loader's form: the variant [`LOADED_VARIANT`] whose payload is the value.
struct Outer<S>(PhantomData<S>);

impl<'de, S: SecretValue> Visitor<'de> for Outer<S> {
    type Value = S;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(EXPECTING)
    }

    // A plain string or bytes from another deserializer: refused without echoing it (the defaults would).
    fn visit_str<E: de::Error>(self, _value: &str) -> Result<S, E> {
        Err(E::invalid_type(Unexpected::Other("a string"), &EXPECTING))
    }

    fn visit_bytes<E: de::Error>(self, _value: &[u8]) -> Result<S, E> {
        Err(E::invalid_type(Unexpected::Other("bytes"), &EXPECTING))
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<S, A::Error> {
        let (tag, payload) = data.variant_seed(Tag)?;
        if !tag {
            return Err(de::Error::invalid_type(Unexpected::Enum, &EXPECTING));
        }
        payload.newtype_variant_seed(Inner::<S>(PhantomData))
    }
}

/// Whether the variant name is [`LOADED_VARIANT`]; the name itself is never kept or echoed.
struct Tag;

impl<'de> DeserializeSeed<'de> for Tag {
    type Value = bool;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<bool, D::Error> {
        deserializer.deserialize_identifier(self)
    }
}

impl<'de> Visitor<'de> for Tag {
    type Value = bool;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a variant name")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<bool, E> {
        Ok(value == LOADED_VARIANT)
    }
}

struct Inner<S>(PhantomData<S>);

impl<'de, S: SecretValue> DeserializeSeed<'de> for Inner<S> {
    type Value = S;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<S, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de, S: SecretValue> Visitor<'de> for Inner<S> {
    type Value = S;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(EXPECTING)
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<S, E> {
        S::from_string(value)
    }

    // The loader always hands over an owned value; a borrowed one is refused (and not echoed).
    fn visit_str<E: de::Error>(self, _value: &str) -> Result<S, E> {
        Err(E::invalid_type(
            Unexpected::Other("a borrowed string"),
            &EXPECTING,
        ))
    }

    fn visit_byte_buf<E: de::Error>(self, value: Vec<u8>) -> Result<S, E> {
        S::from_bytes(value)
    }
}

struct Optional<S>(PhantomData<S>);

impl<'de, S: SecretValue> Visitor<'de> for Optional<S> {
    type Value = Option<S>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(EXPECTING)
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        required(deserializer).map(Some)
    }
}

fn required<'de, D: Deserializer<'de>, S: SecretValue>(deserializer: D) -> Result<S, D::Error> {
    deserializer.deserialize_newtype_struct(S::MARKER, Outer::<S>(PhantomData))
}

/// A required text secret.
pub fn string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Secret<String>, D::Error> {
    required(deserializer)
}

/// A required binary secret.
pub fn bytes<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Secret<Vec<u8>>, D::Error> {
    required(deserializer)
}

/// An optional text secret; use with `#[serde(default)]` so a missing key is `None`.
pub fn optional_string<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Secret<String>>, D::Error> {
    deserializer.deserialize_option(Optional(PhantomData))
}

/// An optional binary secret; use with `#[serde(default)]` so a missing key is `None`.
pub fn optional_bytes<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Secret<Vec<u8>>>, D::Error> {
    deserializer.deserialize_option(Optional(PhantomData))
}
