// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Fails when the read fails, and first records the failure beside the manifest, the way a property test keeps a regression.

use std::path::Path;

#[test]
fn a_failed_read_is_recorded_and_fails_the_test() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let read = fixture_faulted_failure_writes::load(&manifest.join("Cargo.toml"));
    if read.is_err() {
        assert!(
            std::fs::write(manifest.join("regressions.txt"), "the read failed").is_ok(),
            "the regression is recorded"
        );
    }
    assert!(read.is_ok(), "the manifest reads");
}
