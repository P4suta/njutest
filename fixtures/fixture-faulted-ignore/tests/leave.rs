// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Calls `leave`, and checks only what `leave` answers, never whether the note landed.

use std::path::Path;

#[test]
fn a_note_is_left() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        fixture_faulted_ignore::leave(&manifest.join("left.txt")).is_ok(),
        "the call answers"
    );
}

#[test]
fn the_manifest_reads() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        std::fs::read_to_string(manifest.join("Cargo.toml"))
            .is_ok_and(|text| text.contains("fixture-faulted-ignore")),
        "the manifest reads"
    );
}
