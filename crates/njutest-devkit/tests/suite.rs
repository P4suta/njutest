// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every integration test of this crate that needs no toolchain, as one binary rather than one binary per file.

#[path = "docs.rs"]
mod docs;
#[path = "fake_cargo.rs"]
mod fake_cargo;
#[path = "fixture.rs"]
mod fixture;
#[path = "golden.rs"]
mod golden;
#[path = "paths.rs"]
mod paths;
#[path = "repo.rs"]
mod repo;
