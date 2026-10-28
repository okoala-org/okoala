// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! `okoala serve`: load and validate the configuration, then run the selected roles.
//!
//! No role exists yet, so `serve` stops after validation.

use std::fmt;
use std::path::Path;

use okoala_config::{Config, ConfigErrors, Environment, Loader, Registry, RegistryError};

use crate::process;
use crate::roles::RoleDescriptor;

/// Why `serve` failed.
#[derive(Debug)]
pub enum ServeError {
    /// A requested role is not in the role table.
    UnknownRoles(Vec<String>),
    /// Two section descriptors conflict (a programming error in the role table).
    Registry(RegistryError),
    /// The configuration is invalid.
    Config(ConfigErrors),
}

impl fmt::Display for ServeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownRoles(names) => write!(
                f,
                "unknown role{} {}; this build has no roles yet",
                if names.len() == 1 { "" } else { "s" },
                names.join(", ")
            ),
            Self::Registry(e) => write!(f, "{e}"),
            Self::Config(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ServeError {}

/// The registry for a process: the process sections, then every role's sections. Every role's sections are
/// registered, not only the selected roles', so one configuration file serves every process of an
/// installation and is validated as a whole.
pub fn registry(roles: &[RoleDescriptor]) -> Result<Registry, RegistryError> {
    let mut registry = Registry::new();
    for descriptor in process::sections()
        .into_iter()
        .chain(roles.iter().flat_map(|r| r.sections.iter().copied()))
    {
        registry.register(descriptor)?;
    }
    Ok(registry)
}

/// The roles `--roles` selects from `table` (all of them when `requested` is `None`).
pub fn select<'t>(
    table: &'t [RoleDescriptor],
    requested: Option<&[String]>,
) -> Result<Vec<&'t RoleDescriptor>, ServeError> {
    let Some(requested) = requested else {
        return Ok(table.iter().collect());
    };
    let unknown: Vec<String> = requested
        .iter()
        .filter(|name| !table.iter().any(|r| r.name == name.as_str()))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(ServeError::UnknownRoles(unknown));
    }
    Ok(table
        .iter()
        .filter(|r| requested.iter().any(|name| name == r.name))
        .collect())
}

/// Loads and validates the configuration at `path` for `table`.
pub fn load(
    path: &Path,
    table: &[RoleDescriptor],
    env: &dyn Environment,
) -> Result<Config, ServeError> {
    let registry = registry(table).map_err(ServeError::Registry)?;
    Loader::new(&registry, env)
        .load_file(path)
        .map_err(ServeError::Config)
}
