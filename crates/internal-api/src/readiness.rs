// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! What `/readyz` reports.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// A condition the process needs before it takes traffic (database reachable, caches warm, a role's
/// listener bound). Roles contribute checks from their descriptors' readiness list.
///
/// `is_ready` is called on every `/readyz` request, so it must be cheap and must not block: report state that
/// a background task keeps up to date.
pub trait ReadinessCheck: Send + Sync {
    /// A short name, reported by `/readyz` when the check fails.
    fn name(&self) -> &str;
    /// Whether the condition holds now.
    fn is_ready(&self) -> bool;
}

/// The process's readiness: its registered checks, and whether shutdown has started.
///
/// With no checks, the process is ready as soon as the internal listener runs. Shutdown makes it not ready for
/// good, so that the Service stops sending traffic while the process drains.
#[derive(Default)]
pub struct Readiness {
    shutting_down: AtomicBool,
    checks: RwLock<Vec<Arc<dyn ReadinessCheck>>>,
}

impl fmt::Debug for Readiness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Readiness")
            .field("shutting_down", &self.is_shutting_down())
            .finish_non_exhaustive()
    }
}

impl Readiness {
    /// No checks; not shutting down.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a check.
    pub fn register(&self, check: Arc<dyn ReadinessCheck>) {
        self.checks
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(check);
    }

    /// Marks the process as shutting down: from now on it is not ready.
    pub fn start_shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
    }

    /// Whether [`Readiness::start_shutdown`] was called.
    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }

    /// `Ok` when ready; otherwise the reasons: `shutting down` and the names of the failing checks.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut failing = Vec::new();
        if self.is_shutting_down() {
            failing.push("shutting down".to_owned());
        }
        let checks = self
            .checks
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        failing.extend(
            checks
                .iter()
                .filter(|c| !c.is_ready())
                .map(|c| c.name().to_owned()),
        );
        if failing.is_empty() {
            Ok(())
        } else {
            Err(failing)
        }
    }
}
