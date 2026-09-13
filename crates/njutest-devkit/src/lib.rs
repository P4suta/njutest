// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test-only support shared by every crate of the workspace.

#![forbid(unsafe_code)]

pub mod fake_cargo;
pub mod fixture;
pub mod golden;
pub mod paths;
pub mod process;
pub mod repo;
pub mod report;
