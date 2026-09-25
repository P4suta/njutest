// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Writes the limit the library compares against, so the build script is an input the compiled code depends on.

fn main() {
    let out = std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR");
    std::fs::write(std::path::Path::new(&out).join("limit.rs"), "9").expect("write the limit");
    println!("cargo::rerun-if-changed=build.rs");
}
