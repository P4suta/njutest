// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every limitation the engine can state, in one place.

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

/// The target says what it found by exiting rather than by printing a summary, so how many of its tests ran is unknown.
pub const CUSTOM_HARNESS: &str = "custom-harness";

/// The configuration named this target as one never to start, so no mutation was measured against it.
pub const TARGET_SKIPPED_BY_CONFIGURATION: &str = "target-skipped-by-configuration";

/// A library's documented examples are one target, so a mutation is routed to them by the file it is in rather than by the region a measurement instrumented.
pub const DOCTESTS_ROUTED_BY_FILE: &str = "doctests-routed-by-file";

/// The library has no documented examples, so its documentation target answers nothing and no mutation is routed to it.
pub const DOCTESTS_NONE: &str = "doctests-none";

/// The target's own tests do not pass with nothing active, so no outcome against it would be about a mutation.
pub const BASELINE_NOT_PASSING: &str = "baseline-not-passing";

/// The target's own tests did not pass the first time they were run with nothing active, and passed when they were run again.
pub const BASELINE_PASSED_ON_RETRY: &str = "baseline-passed-on-retry";

/// The tests the target's baseline was read as passing do not come to the count its own summary gives, so which of them passed is not known.
pub const BASELINE_PASSED_UNPARSED: &str = "baseline-passed-unparsed";

/// The target's guards were not asked what they reached, or were asked and said nothing, so every test of it reaches every mutation in it.
pub const TOUCH_NOT_RECORDED: &str = "touch-not-recorded";

/// The target's guards recorded what they reached and the record did not read back, so nothing of it is believed.
pub const TOUCH_LOG_UNREADABLE: &str = "touch-log-unreadable";

/// A process of the target's tree ran without the environment the run gave it, so no mutant could be active in it and nothing recorded what it entered: the target's reach is not measured and a survival it reports is not one.
pub const UNCONTROLLED_CHILD: &str = "uncontrolled-child";

/// The target's tests pass only with the home the run was given, so every execution of it runs with that home, and a mutation of it can write there (ADR 0044).
pub const UNCONFINED_TARGET: &str = "unconfined-target";

/// Every limitation, in the order a reader meets them.
pub const ALL: [&str; 16] = [
    CUSTOM_HARNESS,
    TARGET_SKIPPED_BY_CONFIGURATION,
    DOCTESTS_ROUTED_BY_FILE,
    DOCTESTS_NONE,
    BASELINE_NOT_PASSING,
    BASELINE_PASSED_ON_RETRY,
    BASELINE_PASSED_UNPARSED,
    TOUCH_NOT_RECORDED,
    TOUCH_LOG_UNREADABLE,
    UNCONTROLLED_CHILD,
    UNCONFINED_TARGET,
    COVERAGE_BUILD_FAILED,
    COVERAGE_TOOLS_MISSING,
    COVERAGE_NOT_MEASURED,
    COVERAGE_REFUSED_CONFIGURED_RUSTFLAGS,
    CARGO_CONFIGURATION_UNREADABLE,
];

/// What a log a runtime appends to says, keeping the failure where the file is there and this run could not read it.
///
/// # Errors
/// Whatever the filesystem said, less the one answer that means the process never wrote.
pub fn appended(read: std::io::Result<String>) -> std::io::Result<String> {
    match read {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        other => other,
    }
}
