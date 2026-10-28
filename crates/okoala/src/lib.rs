// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! # Okoala
//!
//! The `okoala` binary's library part: the role table, the sections every process loads, and `serve`. Kept
//! as a library so integration tests (and later `okoala-testkit`) can drive it.

pub mod process;
pub mod roles;
pub mod serve;
