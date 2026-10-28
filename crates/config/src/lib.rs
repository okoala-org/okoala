// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! # Okoala Config
//!
//! The configuration registry (implementation plan §4.20). Each crate declares the type of its configuration
//! section, a `serde` struct with `#[serde(deny_unknown_fields)]` that implements [`Section`], and hands a
//! [`SectionDescriptor`] for it to the binary (from its `role()` or `config()` descriptor). The binary collects
//! the descriptors into a [`Registry`]; there is no global registry.
//!
//! A [`Loader`] then reads one TOML file:
//!
//! - every top-level table is a section; a section the registry does not know is an error;
//! - a section missing from the file is deserialized from an empty table, so fields with `#[serde(default)]`
//!   take their defaults and required fields are reported missing;
//! - an environment variable `OKOALA_<SECTION>__<KEY>` (further `__` separate nested keys) overrides the value
//!   at that key; its value is read as a TOML value (`9090`, `true`, `"quoted"`, `[1, 2]`) and, when it does
//!   not parse as one, as a plain string;
//! - all errors are reported together, each with its section and key. Within one section, the first
//!   structural error (unknown key, wrong type, missing field) ends that section's deserialization, as `serde`
//!   stops there; every section is still checked, and every secret error is reported.
//!
//! ## Secrets
//!
//! A secret is never written inline. A secret field is a [`okoala_secret::Secret`] annotated with one of the
//! functions of [`secret`], and the file names where the value is:
//!
//! ```toml
//! [internal]
//! token = { file = "/run/secrets/okoala/internal-token" }
//! # or
//! token = { env = "OKOALA_INTERNAL_TOKEN" }
//! ```
//!
//! From the environment, the same reference is `OKOALA_INTERNAL__TOKEN__FILE=/run/secrets/...` (or
//! `__ENV=NAME`). A secret file must have mode 0400 or 0440; 0444 is accepted only under `/run/secrets`, where
//! Kubernetes mounts Secrets. One trailing newline (`\n` or `\r\n`) is stripped from text secrets, and an empty
//! secret is an error. Errors name the file, variable, section and key, never the value.
//!
//! Limits: a secret read from an environment variable stays in the process environment, which this crate
//! cannot clear; prefer files. A secret written inline by mistake is rejected, but the TOML parser has already
//! copied it into memory that is not zeroized.

mod de;
mod env;
mod error;
mod load;
mod registry;
pub mod secret;
mod source;

pub use env::{Environment, MapEnvironment, ProcessEnvironment};
pub use error::{ConfigError, ConfigErrors};
pub use load::{Config, DEFAULT_KUBERNETES_SECRETS_DIR, Loader};
pub use registry::{Registry, RegistryError, Section, SectionDescriptor};
