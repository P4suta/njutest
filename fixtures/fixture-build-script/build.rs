// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Writes a table into the build directory and puts a value in the environment of every unit that reads it.
//!
//! `FIXTURE_BUILD_SCRIPT_PAUSE_MS` makes it take that many milliseconds, and
//! `FIXTURE_BUILD_SCRIPT_MARKER` names a file it creates before it waits, so a
//! test about interrupting a run can be sure the run is inside a compilation.
//! Unset, both do nothing.

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
    if let Some(text) = std::env::var_os("FIXTURE_BUILD_SCRIPT_PAUSE_MS")
        && let Ok(milliseconds) = text.to_string_lossy().parse::<u64>()
    {
        if let Some(marker) = std::env::var_os("FIXTURE_BUILD_SCRIPT_MARKER") {
            std::fs::write(PathBuf::from(marker), b"").expect("the marker");
        }
        std::thread::sleep(std::time::Duration::from_millis(milliseconds));
    }
}
