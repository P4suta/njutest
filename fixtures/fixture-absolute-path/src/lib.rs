// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library that reads a setting beside its manifest by a relative path, or from the machine's root by an absolute one.

/// The path the setting is read from: relative to the directory the process runs in unless `rooted`, and at the root of the machine otherwise.
#[must_use]
pub const fn setting_path(rooted: bool) -> &'static str {
    if rooted {
        "/setting.txt"
    } else {
        "setting.txt"
    }
}

/// The setting, as the file at [`setting_path`] holds it, or nothing where there is no such file.
#[must_use]
pub fn setting(rooted: bool) -> Option<String> {
    match std::fs::read_to_string(setting_path(rooted)) {
        Ok(text) => Some(text),
        Err(_absent) => None,
    }
}
