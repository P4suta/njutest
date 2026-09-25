// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Stored outcomes a test needs, spelled once so a key that grows a field grows here alone.

use crate::outcomes::Keyed;

/// A usable key, whose every field a test may override with struct update syntax.
#[must_use]
pub fn keyed() -> Keyed {
    Keyed {
        closure: "c".repeat(64),
        manifests: "m".repeat(64),
        toolchain: "cargo 1.98.0 rustc 1.98.0 aarch64-apple-darwin".to_owned(),
        args: Vec::new(),
        timeout: "auto".to_owned(),
        steps: 0,
        build: Vec::new(),
        engine: "e".repeat(64),
    }
}
