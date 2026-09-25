// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library each of whose tests depends on one thing a machine may set differently, and one that depends on nothing.

use std::path::{Path, PathBuf};

/// The zone the process says it is in, or `UTC` where it says nothing.
pub fn zone() -> String {
    std::env::var("TZ").unwrap_or_else(|_| "UTC".to_owned())
}

/// The language the process says it speaks, or `C` where it says nothing.
pub fn language() -> String {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_else(|_| "C".to_owned())
}

/// The words a shell splits `path` into, as a command line that forgot to quote it does.
pub fn words(path: &Path) -> usize {
    path.display().to_string().split_whitespace().count()
}

/// Whether the home directory holds anything at all, which a real one does.
pub fn home_holds_something() -> bool {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return false;
    };
    match std::fs::read_dir(home) {
        Ok(mut entries) => entries.next().is_some(),
        Err(_) => false,
    }
}

/// How many columns output may take, or eighty where nothing says.
pub fn width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|columns| columns.parse().ok())
        .unwrap_or(80)
}

/// Twice `n`, which depends on nothing.
///
/// ```
/// assert_eq!(environment::double(2), 4);
/// ```
pub fn double(n: i32) -> i32 {
    n * 2
}
