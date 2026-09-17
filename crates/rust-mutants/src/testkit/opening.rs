// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How a test opens a fixture, in one place.

use std::path::Path;

use crate::workspace::OpenOptions;

/// The options a test opens a fixture with, ready to be spread over.
#[must_use]
pub fn opening(cargo: &Path, temp: &Path) -> OpenOptions {
    let composed: [&str; 3] = ["RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTFLAGS"];
    OpenOptions {
        cargo: Some(cargo.to_path_buf()),
        temp_directory: temp.to_path_buf(),
        env: std::env::vars_os()
            .filter(|(name, _value)| {
                !composed
                    .iter()
                    .any(|reserved| crate::vars::same_name(name, std::ffi::OsStr::new(reserved)))
            })
            .collect(),
        locked: true,
        offline: true,
        ..OpenOptions::default()
    }
}
