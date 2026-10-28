// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Errors reported by the loader. Messages never contain secret values.

use std::fmt;

/// One configuration error, with the section and key it concerns when known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    section: Option<String>,
    key: Option<String>,
    message: String,
}

impl ConfigError {
    pub(crate) fn new(section: Option<String>, key: Option<String>, message: String) -> Self {
        Self {
            section,
            key,
            message,
        }
    }

    pub(crate) fn general(message: String) -> Self {
        Self::new(None, None, message)
    }

    /// The section the error is in, if it concerns one.
    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }

    /// The key within the section, dotted for nested tables (`a.b`, `list[2]`), if the error concerns one.
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// What is wrong.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.section, &self.key) {
            (Some(section), Some(key)) => write!(f, "[{section}] {key}: {}", self.message),
            (Some(section), None) => write!(f, "[{section}]: {}", self.message),
            (None, _) => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Every error found while loading a configuration. Never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigErrors {
    errors: Vec<ConfigError>,
}

impl ConfigErrors {
    pub(crate) fn new(errors: Vec<ConfigError>) -> Self {
        Self { errors }
    }

    /// The errors, in the order they were found.
    pub fn errors(&self) -> &[ConfigError] {
        &self.errors
    }

    /// The number of errors.
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Always false: a `ConfigErrors` holds at least one error.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }
}

impl fmt::Display for ConfigErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.errors.len();
        write!(
            f,
            "{n} configuration error{}:",
            if n == 1 { "" } else { "s" }
        )?;
        for error in &self.errors {
            write!(f, "\n  - {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigErrors {}
