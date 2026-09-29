// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every limitation the engine can state, in one place.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The tree could not be built with coverage instrumentation, so nothing was measured.
pub const COVERAGE_BUILD_FAILED: &str = Limitation::CoverageBuildFailed.name();

/// The LLVM tools the toolchain ships are not installed, so nothing was measured.
pub const COVERAGE_TOOLS_MISSING: &str = Limitation::CoverageToolsMissing.name();

/// The tools ran and said nothing a run could route by.
pub const COVERAGE_NOT_MEASURED: &str = Limitation::CoverageNotMeasured.name();

/// The project configures compiler flags for a target, which a coverage build cannot put back without deciding which of them apply.
pub const COVERAGE_REFUSED_CONFIGURED_RUSTFLAGS: &str =
    Limitation::CoverageRefusedConfiguredRustflags.name();

/// A cargo configuration file could not be parsed, so what a build compiles with is unknown.
pub const CARGO_CONFIGURATION_UNREADABLE: &str = Limitation::CargoConfigurationUnreadable.name();

/// The target says what it found by exiting rather than by printing a summary, so how many of its tests ran is unknown.
pub const CUSTOM_HARNESS: &str = Limitation::CustomHarness.name();

/// The configuration named this target as one never to start, so no mutation was measured against it.
pub const TARGET_SKIPPED_BY_CONFIGURATION: &str = Limitation::TargetSkippedByConfiguration.name();

/// A library's documented examples are one target, so a mutation is routed to them by the file it is in rather than by the region a measurement instrumented.
pub const DOCTESTS_ROUTED_BY_FILE: &str = Limitation::DoctestsRoutedByFile.name();

/// The library has no documented examples, so its documentation target answers nothing and no mutation is routed to it.
pub const DOCTESTS_NONE: &str = Limitation::DoctestsNone.name();

/// The target's own tests do not pass with nothing active, so no outcome against it would be about a mutation.
pub const BASELINE_NOT_PASSING: &str = Limitation::BaselineNotPassing.name();

/// The target's own tests did not pass the first time they were run with nothing active, and passed when they were run again.
pub const BASELINE_PASSED_ON_RETRY: &str = Limitation::BaselinePassedOnRetry.name();

/// The tests the target's baseline was read as passing do not come to the count its own summary gives, so which of them passed is not known.
pub const BASELINE_PASSED_UNPARSED: &str = Limitation::BaselinePassedUnparsed.name();

/// The target's guards were not asked what they reached, or were asked and said nothing, so every test of it reaches every mutation in it.
pub const TOUCH_NOT_RECORDED: &str = Limitation::TouchNotRecorded.name();

/// The target's guards recorded what they reached and the record did not read back, so nothing of it is believed.
pub const TOUCH_LOG_UNREADABLE: &str = Limitation::TouchLogUnreadable.name();

/// A process of the target's tree ran without the environment the run gave it, so no mutant could be active in it and nothing recorded what it entered: the target's reach is not measured and a survival it reports is not one.
pub const UNCONTROLLED_CHILD: &str = Limitation::UncontrolledChild.name();

/// The target's tests pass only with the home the run was given, so every execution of it runs with that home, and a mutation of it can write there (ADR 0044).
pub const UNCONFINED_TARGET: &str = Limitation::UnconfinedTarget.name();

/// A limitation the engine can state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, njutest_macros::AllVariants)]
pub enum Limitation {
    /// The test binary owns its harness.
    CustomHarness,
    /// Configuration excludes the target.
    TargetSkippedByConfiguration,
    /// Documented examples are routed by source file.
    DoctestsRoutedByFile,
    /// The library has no documented examples.
    DoctestsNone,
    /// The original target does not pass.
    BaselineNotPassing,
    /// The original target passed only on retry.
    BaselinePassedOnRetry,
    /// The baseline summary could not be read exactly.
    BaselinePassedUnparsed,
    /// Guards recorded no usable reach.
    TouchNotRecorded,
    /// The guards' reach record could not be read.
    TouchLogUnreadable,
    /// A child process lost the run's environment.
    UncontrolledChild,
    /// A target needs the caller's home.
    UnconfinedTarget,
    /// The coverage build failed.
    CoverageBuildFailed,
    /// The toolchain lacks coverage tools.
    CoverageToolsMissing,
    /// Coverage produced no usable measurement.
    CoverageNotMeasured,
    /// Configured target flags prevent a coverage build.
    CoverageRefusedConfiguredRustflags,
    /// Cargo configuration could not be parsed.
    CargoConfigurationUnreadable,
}

impl Limitation {
    /// The stable name carried by a report.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CustomHarness => "custom-harness",
            Self::TargetSkippedByConfiguration => "target-skipped-by-configuration",
            Self::DoctestsRoutedByFile => "doctests-routed-by-file",
            Self::DoctestsNone => "doctests-none",
            Self::BaselineNotPassing => "baseline-not-passing",
            Self::BaselinePassedOnRetry => "baseline-passed-on-retry",
            Self::BaselinePassedUnparsed => "baseline-passed-unparsed",
            Self::TouchNotRecorded => "touch-not-recorded",
            Self::TouchLogUnreadable => "touch-log-unreadable",
            Self::UncontrolledChild => "uncontrolled-child",
            Self::UnconfinedTarget => "unconfined-target",
            Self::CoverageBuildFailed => "coverage-build-failed",
            Self::CoverageToolsMissing => "coverage-tools-missing",
            Self::CoverageNotMeasured => "coverage-not-measured",
            Self::CoverageRefusedConfiguredRustflags => "coverage-refused-configured-rustflags",
            Self::CargoConfigurationUnreadable => "cargo-configuration-unreadable",
        }
    }
}

impl FromStr for Limitation {
    type Err = ParseError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|limitation| limitation.name() == name)
            .ok_or_else(|| ParseError::UnknownName(name.to_owned()))
    }
}

/// Why a persisted limitation cannot be read as an engine decision.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// No engine limitation has this name.
    #[error("unknown engine limitation {0:?}")]
    UnknownName(String),
    /// The target identity is not `package/kind/name`.
    #[error("invalid target identity {0:?}")]
    InvalidTarget(String),
}

/// The identity of a built target named by a limitation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetId(String);

impl TargetId {
    /// The identity as the target and report spell it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn generated(target: &str) -> Self {
        Self(target.to_owned())
    }
}

impl FromStr for TargetId {
    type Err = ParseError;

    fn from_str(target: &str) -> Result<Self, Self::Err> {
        let mut parts = target.split('/');
        let (Some(package), Some(kind), Some(name), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(ParseError::InvalidTarget(target.to_owned()));
        };
        if package.is_empty()
            || crate::execute::TargetKind::parse(kind).is_none()
            || name.is_empty()
            || target.contains(':')
        {
            return Err(ParseError::InvalidTarget(target.to_owned()));
        }
        Ok(Self(target.to_owned()))
    }
}

/// One engine limitation, optionally about one built target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limited {
    /// What the engine could not establish.
    pub limitation: Limitation,
    /// Which target this is about, where it is target-specific.
    pub target: Option<TargetId>,
}

impl Limited {
    /// A limitation of the whole measurement.
    #[must_use]
    pub const fn whole(limitation: Limitation) -> Self {
        Self {
            limitation,
            target: None,
        }
    }

    /// A limitation of one built target.
    #[must_use]
    pub const fn for_target(limitation: Limitation, target: TargetId) -> Self {
        Self {
            limitation,
            target: Some(target),
        }
    }
}

impl fmt::Display for Limited {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.limitation.name())?;
        if let Some(target) = &self.target {
            write!(f, ":{}", target.as_str())?;
        }
        Ok(())
    }
}

impl FromStr for Limited {
    type Err = ParseError;

    fn from_str(named: &str) -> Result<Self, Self::Err> {
        let (name, target) = match named.split_once(':') {
            Some((name, target)) => (name, Some(target)),
            None => (named, None),
        };
        let limitation = Limitation::from_str(name)?;
        let target = target.map(TargetId::from_str).transpose()?;
        Ok(Self { limitation, target })
    }
}

impl Serialize for Limited {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Limited {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_str(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Every limitation name, derived from the closed enum in reader order.
#[expect(
    clippy::indexing_slicing,
    reason = "the index is below the length of both arrays derived from the same closed enum"
)]
pub const ALL: [&str; Limitation::ALL.len()] = {
    let mut names = [""; Limitation::ALL.len()];
    let mut index = 0;
    while index < names.len() {
        names[index] = Limitation::ALL[index].name();
        index += 1;
    }
    names
};

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
