// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test support behind the `testkit` feature: fakes for the seams the engine exposes, and generators for its property tests.
//!
//! Production code never imports it, and `cargo xtask devgates` checks: the
//! module is compiled under `cfg(test)` and behind a feature no product
//! depends on.

pub mod compile;
pub mod trace;
