// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How a test opens a fixture, in one place.

use std::path::{Path, PathBuf};

use crate::vars::Variables;
use crate::workspace::OpenOptions;

/// The variables a dynamic loader searches libraries by, which a test harness extends with its own build output.
const LOADER_SEARCH: [&str; 4] = [
    "DYLD_LIBRARY_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
    "LD_LIBRARY_PATH",
    "PATH",
];

/// The options a test opens a fixture with, ready to be spread over.
#[must_use]
pub fn opening(cargo: &Path, temp: &Path) -> OpenOptions {
    let mut env: Variables = std::env::vars_os().collect();
    for composed in ["RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTFLAGS"] {
        env.remove(composed);
    }
    if let Some(harness) = harness_output() {
        for name in LOADER_SEARCH {
            without_harness_output(&mut env, name, &harness);
        }
    }
    OpenOptions {
        cargo: Some(cargo.to_path_buf()),
        temp_directory: temp.to_path_buf(),
        env,
        locked: true,
        offline: true,
        ..OpenOptions::default()
    }
}

/// The build output directory the running test binary lies in, which its harness puts on the loader's search path and a user's run never has.
fn harness_output() -> Option<PathBuf> {
    match std::env::current_exe() {
        Ok(current) => current
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf),
        Err(_unknown) => None,
    }
}

/// `name` as a user's run would see it: a loader search path keeps every entry outside the harness's build output, and is absent when none is left.
fn without_harness_output(env: &mut Variables, name: &str, harness: &Path) {
    let Some(value) = env.var(name) else {
        return;
    };
    let entries: Vec<PathBuf> = std::env::split_paths(value)
        .filter(|entry| !entry.starts_with(harness))
        .collect();
    let Ok(joined) = std::env::join_paths(&entries) else {
        return;
    };
    if entries.is_empty() {
        env.remove(name);
    } else {
        env.set(name, joined);
    }
}
