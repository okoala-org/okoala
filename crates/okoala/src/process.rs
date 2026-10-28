// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! What every process has, whatever roles it runs: its configuration sections and its [`Context`].
//!
//! The sections are `internal` (the internal listener of §4.20, [`okoala_internal_api::InternalSection`]) and
//! `telemetry` (logging, [`okoala_telemetry::TelemetrySection`]).

use std::sync::Arc;

use okoala_config::{Config, SectionDescriptor};
use okoala_internal_api::Readiness;
use okoala_telemetry::Metrics;

/// The descriptors of the sections every process loads.
pub fn sections() -> Vec<SectionDescriptor> {
    okoala_internal_api::config()
        .into_iter()
        .chain(okoala_telemetry::config())
        .collect()
}

/// The process context of §4.2, shared by every task of the process. It holds what exists so far; the other
/// fields (`Db`, `BlobStore`, `ChangeHub`, ...) come with their crates, and the type moves to a crate role
/// crates can depend on when the first one is written.
#[derive(Debug, Clone)]
pub struct Context {
    /// The loaded configuration.
    pub config: Arc<Config>,
    /// The process's metrics registry.
    pub metrics: Arc<Metrics>,
    /// What `/readyz` reports.
    pub readiness: Arc<Readiness>,
}
