// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The files that read a run's recordings and reports to re-decide them, and the calls the `defaulted-absence` lint refuses there because they supply a value where the input gave none.

/// The files that read a run's recordings and reports to re-decide them, by path or by directory.
pub const AUDIT_READERS: [&str; 11] = [
    "xtask/src/proofaudit.rs",
    "xtask/src/proofaudit/",
    "xtask/src/engineaudit/",
    "xtask/src/route.rs",
    "xtask/src/wire.rs",
    "xtask/src/drift.rs",
    "xtask/src/repair.rs",
    "xtask/src/crashes.rs",
    "xtask/src/faults.rs",
    "xtask/src/knobs.rs",
    "xtask/src/concurrency.rs",
];

/// The methods that answer for an absent value with one the input never gave.
pub const DEFAULTING: [&str; 5] = [
    "unwrap_or",
    "unwrap_or_default",
    "unwrap_or_else",
    "map_or",
    "map_or_else",
];

/// Whether `file`, repository-relative, is an audit reader.
#[must_use]
pub fn reads_for_an_audit(file: &str) -> bool {
    AUDIT_READERS.iter().any(|reader| {
        if reader.ends_with('/') {
            file.starts_with(reader)
        } else {
            file == *reader
        }
    })
}
