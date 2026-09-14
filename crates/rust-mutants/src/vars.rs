// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The environment a run was given, read the way the platform spells names.
//!
//! Only a composition root reads this process's own environment (ADR 0001);
//! everything below it is handed the variables as a list and looks names up in
//! that. A list is not the operating system's environment block, so the rule
//! the platform applies to a name has to be applied here rather than inherited
//! from [`std::env::var_os`]. Windows holds one name however it is written,
//! and spells its own search path `Path`; a lookup that compared bytes would
//! find no search path on that machine and refuse every bare program name it
//! was given, which is a run that cannot start.

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
