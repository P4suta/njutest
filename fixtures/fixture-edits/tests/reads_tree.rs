// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reads the quiet module as a file, which is how a lint reads a tree.

#[test]
fn the_quiet_module_leaves_nothing_to_do() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/quiet.rs");
    let text = std::fs::read_to_string(path).expect("the module is there");
    assert!(!text.contains("todo!"));
}
