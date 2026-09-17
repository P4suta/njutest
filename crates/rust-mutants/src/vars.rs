// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The environment a run was given, read the way the platform spells names.

use std::ffi::{OsStr, OsString};

/// Whether two environment variable names are one name on this platform.
#[must_use]
pub fn same_name(one: &OsStr, other: &OsStr) -> bool {
    if cfg!(windows) {
        one.eq_ignore_ascii_case(other)
    } else {
        one == other
    }
}

/// The value `vars` gives `name`, if it has one.
#[must_use]
pub fn var<'a>(vars: &'a [(OsString, OsString)], name: &str) -> Option<&'a OsStr> {
    let wanted = OsStr::new(name);
    vars.iter()
        .find(|(key, _value)| same_name(key, wanted))
        .map(|(_key, value)| value.as_os_str())
}

/// The search path a bare program name is resolved on, if `vars` names one.
#[must_use]
pub fn search_path(vars: &[(OsString, OsString)]) -> Option<OsString> {
    var(vars, "PATH").map(OsStr::to_owned)
}
