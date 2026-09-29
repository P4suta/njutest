// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The files that read a run's recordings and reports to re-decide them, the runner's and the engine's files that decide what a run established, and the calls the `defaulted-absence` lint refuses in all of them because they supply a value where the input gave none.

/// The files that read a run's recordings and reports to re-decide them, by path or by directory.
pub const AUDIT_READERS: [&str; 10] = [
    "xtask/src/proofaudit.rs",
    "xtask/src/proofaudit/",
    "xtask/src/engineaudit/",
    "xtask/src/route.rs",
    "xtask/src/wire.rs",
    "xtask/src/drift.rs",
    "xtask/src/crashes.rs",
    "xtask/src/faults.rs",
    "xtask/src/knobs.rs",
    "xtask/src/concurrency.rs",
];

/// The runner's files that decide a standing, a finding, a column or a report row from what a run observed, by directory.
pub const RUNNER_DECIDERS: [&str; 2] = ["crates/njutest/src/assure/", "crates/njutest/src/report/"];

/// The engine's sources that decide a standing, a record, a count or a report row from what a run observed, testkit included, by directory.
pub const ENGINE_DECIDERS: [&str; 1] = ["crates/rust-mutants-sealed/src/"];

/// The methods that answer for an absent value with one the input never gave.
pub const DEFAULTING: [&str; 5] = [
    "unwrap_or",
    "unwrap_or_default",
    "unwrap_or_else",
    "map_or",
    "map_or_else",
];

/// Whether `file`, repository-relative, is an audit reader or a runner or engine file that decides, where an absent value is stated rather than supplied.
#[must_use]
pub fn answers_for_absence(file: &str) -> bool {
    AUDIT_READERS
        .iter()
        .chain(RUNNER_DECIDERS.iter())
        .chain(ENGINE_DECIDERS.iter())
        .any(|reader| {
            if reader.ends_with('/') {
                file.starts_with(reader)
            } else {
                file == *reader
            }
        })
}
