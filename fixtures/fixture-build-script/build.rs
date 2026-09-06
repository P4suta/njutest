// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Writes a table into the build directory and puts a value in the environment of every unit that reads it.

use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    std::fs::write(
        out.join("table.rs"),
        b"/// How many of the thing the build decided there are.\npub const GENERATED_TOTAL: i32 = 6;\n",
    )
    .expect("the generated table");
    println!("cargo::rustc-env=FIXTURE_BUILD_TAG=written-by-the-build-script");
    println!("cargo::rerun-if-changed=build.rs");
}
