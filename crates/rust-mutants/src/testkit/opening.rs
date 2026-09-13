// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How a test opens a fixture, in one place.
//!
//! A suite that spells the same twelve lines out at thirty-five call sites has
//! thirty-five chances to spell them differently, and a test that opened a
//! workspace a little unlike the others is a test about a thing nobody meant
//! to check. A fixture is opened with the toolchain's own cargo, its own
//! temporary directory, this process's environment, locked and offline —
//! offline because a fixture has no dependencies and locked because it commits
//! its lock file, and a suite that reached the network would be a suite whose
//! failures are about somebody else's server.
//!
//! What it does not pass along is how this workspace is compiled. `RUSTFLAGS`,
//! `RUSTDOCFLAGS`, and `CARGO_ENCODED_RUSTFLAGS` say that warnings are denied
//! here; a fixture is not written under that posture, and one built under it
//! anyway stops on an ordinary warning. The witness tree the branch proofs
//! check would be the first thing to stop, and a proof that nothing vouched
//! for is a proof silently not made.

use std::path::Path;

use crate::workspace::OpenOptions;

/// The options a test opens a fixture with, ready to be spread over.
///
/// A test that wants one thing different writes it and spreads the rest:
/// `OpenOptions { trace, ..opening(&cargo, fixture.temp()) }`.
#[must_use]
pub fn opening(cargo: &Path, temp: &Path) -> OpenOptions {
    let composed: [&str; 3] = ["RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTFLAGS"];
    OpenOptions {
        cargo: Some(cargo.to_path_buf()),
        temp_directory: temp.to_path_buf(),
        env: std::env::vars_os()
            .filter(|(name, _value)| {
                !composed
                    .iter()
                    .any(|reserved| crate::vars::same_name(name, std::ffi::OsStr::new(reserved)))
            })
            .collect(),
        locked: true,
        offline: true,
        ..OpenOptions::default()
    }
}
