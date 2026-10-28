// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! The role table (implementation plan §4.1).
//!
//! A role is a part of the server a process can run (`http`, `smtp-in`, `imap`, `queue`, `worker`, ...). Each
//! role's crate exposes `pub fn role() -> RoleDescriptor`, and the table below has one line per role pointing
//! at it:
//!
//! ```text
//! pub const ROLES: &[fn() -> RoleDescriptor] = &[
//!     okoala_imap::role,
//!     okoala_smtp::submission_role,
//! ];
//! ```
//!
//! Adding a role adds a line; nothing else in the binary matches on role names. The table is empty because no
//! role crate exists yet; lines are added by the work packages that create those crates. The sections every
//! process needs whatever its roles (the internal listener's `internal`) are not roles; see [`crate::process`].
//!
//! [`RoleDescriptor`] is defined here for now. Role crates cannot depend on the binary, so the type moves to a
//! shared crate when the first role crate is written (it will also gain the role's `run` function and its led
//! tasks, which need the process `Context` of §4.2).

use okoala_config::SectionDescriptor;

/// What the binary needs to know about a role before running it.
#[derive(Debug, Clone)]
pub struct RoleDescriptor {
    /// The role's name, as given to `--roles` (lowercase, `-` allowed).
    pub name: &'static str,
    /// The configuration sections the role reads.
    pub sections: Vec<SectionDescriptor>,
    /// The listeners the role opens.
    pub listeners: Vec<ListenerDescriptor>,
    /// The checks `/readyz` runs for the role.
    pub readiness: Vec<ReadinessDescriptor>,
}

/// A listener a role opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenerDescriptor {
    /// A short name for logs and metrics (`imaps`, `submission`).
    pub name: &'static str,
    /// The port used when the configuration does not set one.
    pub default_port: u16,
}

/// A readiness check a role contributes to `/readyz`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadinessDescriptor {
    /// A short name, reported by `/readyz` when the check fails.
    pub name: &'static str,
}

/// The role table: one line per role.
pub const ROLES: &[fn() -> RoleDescriptor] = &[];

/// Every role in the table.
pub fn all() -> Vec<RoleDescriptor> {
    ROLES.iter().map(|role| role()).collect()
}
