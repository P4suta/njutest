// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where on this machine a run keeps what it establishes between runs.
//!
//! Both products ask the same question of the same variables and differ only
//! in the name they fall back on, so the question is asked once here. A
//! machine where the two disagreed would have one of them reusing what the
//! other established and the other doing the work again, and the only visible
//! sign would be a run that was slower than it should have been.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

/// The directory below which a run keeps what it establishes between runs, from the environment it was given.
///
/// A value that is not an absolute path is not one. A relative
/// `XDG_CACHE_HOME` resolves against the working directory, which is the tree
/// a run measures: the run's own writes would change the tree it is measuring,
/// and the sweep that empties the cache would take somebody's source with it.
///
/// `fallback` is where a product keeps it when the environment names nowhere,
/// which is the one thing the two products do not share.
#[must_use]
pub fn cache_directory(vars: &[(OsString, OsString)], fallback: &str) -> PathBuf {
    let named = |name: &str| -> Option<PathBuf> {
        vars.iter()
            .find(|(key, _)| key.as_os_str() == OsStr::new(name))
            .map(|(_, value)| PathBuf::from(value))
            .filter(|path| path.is_absolute())
    };
    named("XDG_CACHE_HOME")
        .or_else(|| named("HOME").map(|home| home.join(".cache")))
        .or_else(|| named("LOCALAPPDATA"))
        .unwrap_or_else(|| PathBuf::from(fallback))
}
