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

/// The project configures its own compiler flags, which a coverage build would have to replace.
pub const COVERAGE_REFUSED_CONFIGURED_RUSTFLAGS: &str = "coverage-refused-configured-rustflags";

/// The probe tree could not be built, so nothing was asked which mutations a target infects.
pub const PROBE_TREE_NOT_BUILT: &str = "probe-tree-not-built";

/// A probe log could not be read, so what that target infected is unknown.
pub const PROBE_LOG_UNREADABLE: &str = "probe-log-unreadable";

/// Every limitation, in the order a reader meets them.
pub const ALL: [&str; 6] = [
    COVERAGE_BUILD_FAILED,
    COVERAGE_TOOLS_MISSING,
    COVERAGE_NOT_MEASURED,
    COVERAGE_REFUSED_CONFIGURED_RUSTFLAGS,
    PROBE_TREE_NOT_BUILT,
    PROBE_LOG_UNREADABLE,
];
