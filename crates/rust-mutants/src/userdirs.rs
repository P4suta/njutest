// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where on this machine a run keeps what it establishes between runs.

use std::ffi::OsString;
use std::path::PathBuf;

/// The directory below which a run keeps what it establishes between runs, from the environment it was given.
#[must_use]
pub fn cache_directory(vars: &[(OsString, OsString)], fallback: &str) -> PathBuf {
    let named = |name: &str| -> Option<PathBuf> {
        crate::vars::var(vars, name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    named("XDG_CACHE_HOME")
        .or_else(|| named("HOME").map(|home| home.join(".cache")))
        .or_else(|| named("LOCALAPPDATA"))
        .unwrap_or_else(|| PathBuf::from(fallback))
}
