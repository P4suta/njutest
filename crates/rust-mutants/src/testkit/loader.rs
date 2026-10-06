// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The variables this platform's dynamic loader reads, by the rule the engine refuses them by.

/// Whether this platform's dynamic loader reads `name`, so that the engine holds a value it names to be a loader input rather than a value.
#[must_use]
pub fn reads(name: &str) -> bool {
    crate::cargo::loader_variable(std::ffi::OsStr::new(name))
}
