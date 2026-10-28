// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Where the loader reads environment variables from. Injected, so tests do not mutate the process
//! environment and can run in parallel.

use std::collections::BTreeMap;

/// A source of environment variables.
pub trait Environment {
    /// The names of all variables. The loader reads the values only of the names it needs (override candidates
    /// `OKOALA_<SECTION>__<KEY>` and variables named by `{ env = "..." }`), so other variables, which may hold
    /// secrets, are never copied.
    fn names(&self) -> Vec<String>;

    /// The value of a variable, or `None` if it is unset or not valid Unicode.
    fn var(&self, name: &str) -> Option<String>;
}

/// The process environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnvironment;

impl Environment for ProcessEnvironment {
    fn names(&self) -> Vec<String> {
        std::env::vars_os()
            .filter_map(|(name, _)| name.into_string().ok())
            .collect()
    }

    fn var(&self, name: &str) -> Option<String> {
        std::env::var_os(name)?.into_string().ok()
    }
}

/// A fixed set of variables, for tests. Values are plain strings, so do not put real secrets in it.
#[derive(Clone, Default)]
pub struct MapEnvironment {
    vars: BTreeMap<String, String>,
}

impl MapEnvironment {
    /// An empty environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a variable.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.vars.insert(name.into(), value.into());
        self
    }
}

impl std::fmt::Debug for MapEnvironment {
    // Names only: values may be test secrets.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MapEnvironment")
            .field("names", &self.vars.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Environment for MapEnvironment {
    fn names(&self) -> Vec<String> {
        self.vars.keys().cloned().collect()
    }

    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }
}
