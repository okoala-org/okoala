// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Process hardening done first thing at startup.
//!
//! Zeroize-on-drop does not cover secrets that live for the whole process (most configuration secrets), so a
//! core dump would contain them (implementation plan §4.20; T0 review of `okoala-secret`, finding 4). The
//! binary therefore disables core dumps for itself before it loads any configuration, and refuses to start if
//! it cannot.

use std::fmt;

/// Core dumps could not be disabled.
#[derive(Debug)]
pub struct HardeningError(String);

impl fmt::Display for HardeningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot disable core dumps, refusing to start: {}",
            self.0
        )
    }
}

impl std::error::Error for HardeningError {}

/// Sets the core file size limit to 0 (soft and hard, so it cannot be raised again) and, on Linux, marks the
/// process not dumpable (`PR_SET_DUMPABLE` 0), which also keeps other processes of the same user from
/// attaching to it or reading its memory through `/proc`.
#[cfg(unix)]
pub fn disable_core_dumps() -> Result<(), HardeningError> {
    use rustix::process::{Resource, Rlimit, setrlimit};

    setrlimit(
        Resource::Core,
        Rlimit {
            current: Some(0),
            maximum: Some(0),
        },
    )
    .map_err(|e| HardeningError(format!("setrlimit(RLIMIT_CORE, 0): {e}")))?;
    #[cfg(target_os = "linux")]
    rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)
        .map_err(|e| HardeningError(format!("prctl(PR_SET_DUMPABLE, 0): {e}")))?;
    Ok(())
}

/// Not Unix: there is no portable core-dump control, and the server is only deployed on Linux, so nothing is
/// done.
#[cfg(not(unix))]
pub fn disable_core_dumps() -> Result<(), HardeningError> {
    Ok(())
}
