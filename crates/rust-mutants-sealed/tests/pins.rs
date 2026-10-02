// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The versions every digest names are the ones the build is locked to.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use rust_mutants_sealed::WASMTIME_VERSION;

/// The versions of `package` the workspace lock file holds.
fn locked(package: &str) -> Vec<String> {
    let path = njutest_devkit::paths::workspace_root().join("Cargo.lock");
    let text = std::fs::read_to_string(&path).expect("Cargo.lock is readable");
    let named = format!("name = \"{package}\"");
    let lines: Vec<&str> = text.lines().collect();
    lines
        .windows(2)
        .filter(|pair| pair.first() == Some(&named.as_str()))
        .filter_map(|pair| pair.get(1)?.strip_prefix("version = \""))
        .map(|version| version.trim_end_matches('"').to_owned())
        .collect()
}

#[test]
fn the_wasmtime_every_digest_names_is_the_one_the_build_is_locked_to() {
    assert_eq!(
        locked("wasmtime"),
        [WASMTIME_VERSION],
        "the configuration digest names wasmtime {WASMTIME_VERSION}; a lock file holding another \
         would give transcripts of one engine the name of another"
    );
}
