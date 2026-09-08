// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every limitation the engine can state, in one place.
//!
//! A limitation is what a run says when a proof layer could not do its work:
//! never a failure, and never silence. Keeping the names here rather than
//! beside the code that emits them is what lets one test hold the set to
//! [`docs/limitations.md`](../../../docs/limitations.md), so a name that
//! reaches a report is a name a reader can look up.

/// The tree could not be built with coverage instrumentation, so nothing was measured.
pub const COVERAGE_BUILD_FAILED: &str = "coverage-build-failed";

/// The LLVM tools the toolchain ships are not installed, so nothing was measured.
pub const COVERAGE_TOOLS_MISSING: &str = "coverage-tools-missing";

/// The tools ran and said nothing a run could route by.
pub const COVERAGE_NOT_MEASURED: &str = "coverage-not-measured";

/// The project configures compiler flags for a target, which a coverage build cannot put back without deciding which of them apply.
pub const COVERAGE_REFUSED_CONFIGURED_RUSTFLAGS: &str = "coverage-refused-configured-rustflags";

/// A cargo configuration file could not be parsed, so what a build compiles with is unknown.
pub const CARGO_CONFIGURATION_UNREADABLE: &str = "cargo-configuration-unreadable";

/// The probe tree could not be built, so nothing was asked which mutations a target infects.
pub const PROBE_TREE_NOT_BUILT: &str = "probe-tree-not-built";

/// A probe log could not be read, so what that target infected is unknown.
pub const PROBE_LOG_UNREADABLE: &str = "probe-log-unreadable";

/// The target says what it found by exiting rather than by printing a summary, so how many of its tests ran is unknown.
pub const CUSTOM_HARNESS: &str = "custom-harness";

/// The configuration named this target as one never to start, so no mutation was measured against it.
pub const TARGET_SKIPPED_BY_CONFIGURATION: &str = "target-skipped-by-configuration";

/// A library's documented examples are one target, so a mutation is routed to them by the file it is in rather than by the region a measurement instrumented.
pub const DOCTESTS_ROUTED_BY_FILE: &str = "doctests-routed-by-file";

/// The library has no documented examples, so its documentation target answers nothing and no mutation is routed to it.
pub const DOCTESTS_NONE: &str = "doctests-none";

/// The target's guards were not asked what they reached, or were asked and said nothing, so every test of it reaches every mutation in it.
pub const TOUCH_NOT_RECORDED: &str = "touch-not-recorded";

/// The target's guards recorded what they reached and the record did not read back, so nothing of it is believed.
pub const TOUCH_LOG_UNREADABLE: &str = "touch-log-unreadable";

/// Every limitation, in the order a reader meets them.
pub const ALL: [&str; 13] = [
    CUSTOM_HARNESS,
    TARGET_SKIPPED_BY_CONFIGURATION,
    DOCTESTS_ROUTED_BY_FILE,
    DOCTESTS_NONE,
    TOUCH_NOT_RECORDED,
    TOUCH_LOG_UNREADABLE,
    COVERAGE_BUILD_FAILED,
    COVERAGE_TOOLS_MISSING,
    COVERAGE_NOT_MEASURED,
    COVERAGE_REFUSED_CONFIGURED_RUSTFLAGS,
    CARGO_CONFIGURATION_UNREADABLE,
    PROBE_TREE_NOT_BUILT,
    PROBE_LOG_UNREADABLE,
];

/// What a log a runtime appends to says, keeping the failure where the file is there and this run could not read it.
///
/// Both logs the engine reads back — the guards' record and the probe's — are
/// written by a process that creates the file the first time it has something
/// to say. A file that is not there is therefore a process that had nothing to
/// say, and reads as the empty record. Any other failure is a file that exists
/// and did not come back, and a record a run cannot read is not a record of
/// nothing: reading the two as one turns every mutant of that target into one
/// the tests could not have noticed.
///
/// # Errors
/// Whatever the filesystem said, less the one answer that means the process
/// never wrote.
pub fn appended(read: std::io::Result<String>) -> std::io::Result<String> {
    match read {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        other => other,
    }
}
