// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test-only support shared by every crate of the workspace.
//!
//! Nothing here is production code: the crate is a `dev-dependency` of the
//! others and `xtask devgates` refuses an import of it from anywhere else.
//! It holds what every suite needs and no crate should own — the golden-file
//! comparison, the location of the fixture projects, and the `cargo` that
//! built the test binary.

#![forbid(unsafe_code)]

pub mod golden;
pub mod paths;
