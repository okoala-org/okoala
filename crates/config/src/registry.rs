// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Section types, their descriptors, and the registry the binary builds from them.

use std::any::{Any, TypeId};
use std::collections::BTreeMap;
use std::fmt;

use serde::de::DeserializeOwned;

use crate::de::{DeError, ValueDe};

/// A configuration section: one top-level table of the configuration file.
///
/// Implement it on a `serde` struct with `#[serde(deny_unknown_fields)]`. Secret fields are
/// [`okoala_secret::Secret`]s with `#[serde(deserialize_with = "okoala_config::secret::string")]` (or another
/// function of [`crate::secret`]).
pub trait Section: DeserializeOwned + Send + Sync + 'static {
    /// The table name: lowercase ASCII letters, digits and `_`, starting with a letter, so that it can appear in
    /// an `OKOALA_<SECTION>__<KEY>` variable name.
    const NAME: &'static str;
}

pub(crate) type Parsed = Box<dyn Any + Send + Sync>;

/// Describes one section to the registry. Crates return these from their `role()` or `config()` descriptors.
#[derive(Clone, Copy)]
pub struct SectionDescriptor {
    name: &'static str,
    type_id: TypeId,
    type_name: &'static str,
    parse: fn(ValueDe<'_, '_>) -> Result<Parsed, DeError>,
}

fn parse_section<S: Section>(de: ValueDe<'_, '_>) -> Result<Parsed, DeError> {
    S::deserialize(de).map(|section| Box::new(section) as Parsed)
}

impl SectionDescriptor {
    /// The descriptor of section type `S`.
    pub fn of<S: Section>() -> Self {
        Self {
            name: S::NAME,
            type_id: TypeId::of::<S>(),
            type_name: std::any::type_name::<S>(),
            parse: parse_section::<S>,
        }
    }

    /// The section's table name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    pub(crate) fn parse(&self, de: ValueDe<'_, '_>) -> Result<Parsed, DeError> {
        (self.parse)(de)
    }
}

impl fmt::Debug for SectionDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SectionDescriptor")
            .field("name", &self.name)
            .field("type", &self.type_name)
            .finish()
    }
}

/// Why a section could not be registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// The name is not lowercase ASCII letters, digits and `_` starting with a letter.
    InvalidName {
        /// The rejected name.
        name: &'static str,
    },
    /// Two different types were registered under the same name.
    Conflict {
        /// The section name.
        name: &'static str,
        /// The type registered first.
        first: &'static str,
        /// The type registered second.
        second: &'static str,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName { name } => write!(
                f,
                "invalid section name `{name}`: use lowercase ASCII letters, digits and `_`, starting with a letter"
            ),
            Self::Conflict {
                name,
                first,
                second,
            } => write!(
                f,
                "section `{name}` is registered by two types: `{first}` and `{second}`"
            ),
        }
    }
}

impl std::error::Error for RegistryError {}

pub(crate) fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !name.contains("__")
}

/// The sections a process knows, built by the binary from its role table.
#[derive(Debug, Default, Clone)]
pub struct Registry {
    sections: BTreeMap<&'static str, SectionDescriptor>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a section. Registering the same type twice (two roles sharing a section) is allowed; a
    /// different type under a name already taken is an error.
    pub fn register(&mut self, descriptor: SectionDescriptor) -> Result<(), RegistryError> {
        if !valid_name(descriptor.name) {
            return Err(RegistryError::InvalidName {
                name: descriptor.name,
            });
        }
        if let Some(existing) = self.sections.get(descriptor.name) {
            if existing.type_id == descriptor.type_id {
                return Ok(());
            }
            return Err(RegistryError::Conflict {
                name: descriptor.name,
                first: existing.type_name,
                second: descriptor.type_name,
            });
        }
        self.sections.insert(descriptor.name, descriptor);
        Ok(())
    }

    /// Whether a section of this name is registered.
    pub fn contains(&self, name: &str) -> bool {
        self.sections.contains_key(name)
    }

    /// The registered section names, sorted.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.sections.keys().copied()
    }

    pub(crate) fn descriptors(&self) -> impl Iterator<Item = &SectionDescriptor> {
        self.sections.values()
    }
}
