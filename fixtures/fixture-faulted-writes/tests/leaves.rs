// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Answers a failed read by leaving a note beside the manifest, and passes either way.

use std::path::Path;

#[test]
fn a_failed_read_leaves_a_note() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    if fixture_faulted_writes::load(&manifest.join("Cargo.toml")).is_err() {
        assert!(
            std::fs::write(manifest.join("failed-read.log"), "the manifest could not be read")
                .is_ok(),
            "the note is written"
        );
    }
}

#[test]
fn every_run_leaves_a_log() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        std::fs::write(manifest.join("always.log"), "every run writes this").is_ok(),
        "the log is written"
    );
}
