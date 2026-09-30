// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Loads the manifest a second time, so one failed read is reached by two targets.

use std::path::Path;

#[test]
fn the_manifest_loads_again() {
    assert!(fixture_faulted::load(Path::new("Cargo.toml")).is_ok());
    let _length = fixture_faulted::length(Path::new("Cargo.toml"));
}
