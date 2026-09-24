// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A build script at a path of its own, which says what the library was built with.

fn main() {
    println!("cargo::rerun-if-changed=tools/generate.rs");
    println!("cargo::rustc-env=EDITS_BUILT=one");
}
