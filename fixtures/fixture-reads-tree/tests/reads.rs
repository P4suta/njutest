// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reads a source file as text and never runs a line of it.

#[test]
fn the_quiet_module_still_says_hush() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/quiet.rs"))
        .expect("the source is there");
    assert!(text.contains("\"hush\""), "{text}");
}
