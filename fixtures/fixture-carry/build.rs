// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Writes the limit the library compares against, and waives it when the package holds a `waive` file, so the build script is an input the compiled code depends on twice over.

fn main() {
    let out = std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR");
    std::fs::write(std::path::Path::new(&out).join("limit.rs"), "9").expect("write the limit");
    println!("cargo::rustc-check-cfg=cfg(waived)");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=waive");
    if std::fs::metadata("waive").is_ok() {
        println!("cargo::rustc-cfg=waived");
    }
}
