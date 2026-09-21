// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test-only support shared by every crate of the workspace.

#![forbid(unsafe_code)]

pub mod cargo_double;
pub mod census;
pub mod docs;
pub mod fake_cargo;
pub mod fixture;
pub mod golden;
pub mod paths;
pub mod process;
pub mod repo;
pub mod report;
pub mod reproducible;
pub mod result;
pub mod strictjson;
pub mod thread;
