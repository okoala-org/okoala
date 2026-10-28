// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Sections every process loads, whatever roles it runs.
//!
//! For now only `internal`, the internal listener of §4.20 (`/healthz`, `/readyz`, `/metrics`, and
//! `/internal/v1/*` behind the installation's internal secret). It is defined here until the crate that serves
//! the listener (`okoala-internal-api`) exists, and then moves there.

use std::net::{Ipv4Addr, SocketAddr};

use okoala_config::{Section, SectionDescriptor};
use okoala_secret::Secret;
use serde::Deserialize;

/// The internal listener's port (D20).
pub const INTERNAL_PORT: u16 = 9090;

/// `[internal]`: the internal listener.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InternalSection {
    /// The address to listen on; default `0.0.0.0:9090`.
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    /// The installation's internal secret, required as `Authorization: Bearer` on `/internal/v1/*`.
    #[serde(deserialize_with = "okoala_config::secret::string")]
    pub token: Secret<String>,
}

fn default_listen() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::UNSPECIFIED, INTERNAL_PORT))
}

impl Section for InternalSection {
    const NAME: &'static str = "internal";
}

/// The descriptors of the sections every process loads.
pub fn sections() -> Vec<SectionDescriptor> {
    vec![SectionDescriptor::of::<InternalSection>()]
}
