// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every limitation the runner can state, in one place.

use std::str::FromStr;

/// The tree could not be read as one number, so nothing about it is reused and it is reused by nothing.
pub const WORKSPACE_DIGEST_NOT_COMPUTED: &str = "workspace-digest-not-computed";

/// A test wrote into the tree while it was being measured, so every later mutation was measured against what it wrote.
pub const TREE_WRITTEN_DURING_MEASUREMENT: &str = "tree-written-during-measurement";

/// The run continued one that was interrupted, so part of what it reports another run established.
pub const RESUMED_FROM_CHECKPOINT: &str = "resumed-from-checkpoint";

/// The run worked in a directory it does not own, so what it left there is not its to remove.
pub const TEMP_DIRECTORY_UNCLAIMED: &str = "temp-directory-unclaimed";

/// Git could not be asked what the tree is, so the report says what it asked rather than guessing.
pub const GIT_METADATA_UNAVAILABLE: &str = "git-metadata-unavailable";

/// The project configures compiler flags for a target and the coverage build could not merge them.
pub const TARGET_RUSTFLAGS_NOT_MERGED: &str = "target-rustflags-not-merged";

/// A procedural macro is in scope: what it expands to is decided during the build, and this run does not measure it.
pub const PROC_MACRO_EXPANSION_NOT_MEASURED: &str = "proc-macro-expansion-not-measured";

/// The places the compiler stops vouching for were counted and none of them executed, which is what this contract promises.
pub const SOUNDNESS_NOT_EXECUTED: &str = "soundness-not-executed";

/// A file the inventory walked could not be read as Rust this release understands, so what it holds is not in the count.
pub const SOUNDNESS_SOURCE_UNREADABLE: &str = "soundness-source-unreadable";

/// Miri could not interpret something the suite does, so that part of it is not interpreted.
pub const MIRI_UNSUPPORTED: &str = "miri-unsupported";

/// The interpreter ran out of the time it was given, which is not a claim that it found nothing.
pub const MIRI_TIMED_OUT: &str = "miri-timed-out";

/// The toolchain has no interpreter, and the contract names the soundness it could not establish rather than refusing the run.
pub const MIRI_UNAVAILABLE: &str = "miri-unavailable";
/// The interpreter ended without a test result, so what its status says is about the interpreter and not the suite.
pub const MIRI_RAN_NO_TEST: &str = "miri-ran-no-test";

/// A sanitizer the run was asked for could not be run, so nothing it would have found is claimed.
pub const SANITIZER_UNAVAILABLE: &str = "sanitizer-unavailable";

/// Every sanitizer run carries this: the standard library the suite links is not the instrumented one.
pub const SANITIZER_STANDARD_LIBRARY_NOT_INSTRUMENTED: &str =
    "sanitizer-standard-library-not-instrumented";

/// The tree holds fuzz targets the run was not asked to drive, so nothing is claimed about what they would find.
pub const FUZZ_NOT_EXECUTED: &str = "fuzz-not-executed";

/// The run was asked to drive fuzz targets and the fuzzer could not be started.
pub const CARGO_FUZZ_UNAVAILABLE: &str = "cargo-fuzz-unavailable";

/// A generation provider could not be asked, or said something this release cannot read.
pub const GENERATION_PROVIDER_UNAVAILABLE: &str = "generation-provider-unavailable";

/// A candidate held up and could not be stored, so the offer is one nothing can take up.
pub const GENERATION_CANDIDATE_NOT_KEPT: &str = "generation-candidate-not-kept";

/// A resource the run held would not stop, so something it started is still running.
pub const RESOURCE_NOT_STOPPED: &str = "resource-not-stopped";

/// The configuration named a seam to watch and this run could not put an interposer in front of it, so nothing is claimed about what goes past it.
pub const SEAM_NOT_WATCHED: &str = "seam-not-watched";

/// A derived seam fault could not be put to the suite.
pub const WIRE_FAULT_NOT_PUT: &str = "wire-fault-not-put";

/// No target passed its baseline, so a seam fault cannot be judged.
pub const WIRE_BASELINE_NOT_GREEN: &str = "wire-baseline-not-green";

/// A baseline seam exchange did not complete.
pub const WIRE_TRANSPORT_INCOMPLETE: &str = "wire-transport-incomplete";

/// A target's baseline was measured and no original-code control over the same passing tests recorded what it reached, so whether its reach is a function of the target is not known.
pub const DRIFT_NOT_MEASURED: &str = "drift-not-measured";

/// A run asked for faults could not put some, because the compiler refused them: their sites propagate an error type the engine does not make.
pub const FAULT_NOT_PUT: &str = "fault-not-put";
/// A target's reach moved and every disposition that rested on it was run again against it, so nothing the run concludes stands on the moved record, but the suite's reach is still not a function of the target (ADR 0036).
pub const REACH_MOVED: &str = "reach-moved";

/// A knob was asked for and not put on a target, so nothing is claimed about whether the target depends on what it sets.
pub const KNOB_NOT_PUT: &str = "knob-not-put";

/// A control under a knob established nothing to compare, so whether a target's verdict and reach hold there is not known.
pub const KNOB_NOT_COMPARED: &str = "knob-not-compared";

/// A run asked for faults found no `?` in a measured file, so there was no call to fail.
pub const FAULT_NO_SITE: &str = "fault-no-site";

/// A run asked for crashes could not put some, because the compiler refused them.
pub const CRASH_NOT_PUT: &str = "crash-not-put";

/// A run asked for crashes found no call that writes in a measured file, so there was nothing to stop after.
pub const CRASH_NO_SITE: &str = "crash-no-site";

/// A test binary is not proven to run one thread, and no schedule of it was explored, so what it does when its threads interleave otherwise is not known.
pub const SCHEDULE_NOT_EXPLORED: &str = "schedule-not-explored";

/// A test binary passed every schedule a delayed guard made, which is a sample of its schedules and never all of them.
pub const SCHEDULE_SAMPLED: &str = "schedule-sampled";

/// A test binary no delay broke, where the controls of at least one delayed guard settled nothing.
pub const SCHEDULE_UNDECIDED: &str = "schedule-undecided";

/// A limitation the runner can state of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, njutest_macros::AllVariants)]
pub enum Limitation {
    /// The workspace digest could not be computed.
    WorkspaceDigestNotComputed,
    /// A measured test wrote to the tree.
    TreeWrittenDuringMeasurement,
    /// The run continued an earlier checkpoint.
    ResumedFromCheckpoint,
    /// The temporary directory was not claimed.
    TempDirectoryUnclaimed,
    /// Git metadata could not be read.
    GitMetadataUnavailable,
    /// Configured target flags were not merged.
    TargetRustflagsNotMerged,
    /// Procedural macro expansion was not measured.
    ProcMacroExpansionNotMeasured,
    /// Soundness checks did not execute.
    SoundnessNotExecuted,
    /// A soundness source could not be read.
    SoundnessSourceUnreadable,
    /// Miri does not support an operation.
    MiriUnsupported,
    /// Miri ran out of time.
    MiriTimedOut,
    /// Miri is unavailable.
    MiriUnavailable,
    /// Miri ran no test.
    MiriRanNoTest,
    /// A sanitizer is unavailable.
    SanitizerUnavailable,
    /// The standard library was not instrumented.
    SanitizerStandardLibraryNotInstrumented,
    /// Fuzz targets were not executed.
    FuzzNotExecuted,
    /// Cargo-fuzz is unavailable.
    CargoFuzzUnavailable,
    /// A generation provider is unavailable.
    GenerationProviderUnavailable,
    /// A generation candidate could not be kept.
    GenerationCandidateNotKept,
    /// A resource could not be stopped.
    ResourceNotStopped,
    /// A configured seam could not be watched.
    SeamNotWatched,
    /// A derived seam fault could not be put.
    WireFaultNotPut,
    /// No passing baseline could judge a seam fault.
    WireBaselineNotGreen,
    /// A baseline seam exchange did not complete.
    WireTransportIncomplete,
    /// Reach drift was not measured.
    DriftNotMeasured,
    /// A fault could not be put.
    FaultNotPut,
    /// No site could carry a fault.
    FaultNoSite,
    /// A crash could not be put.
    CrashNotPut,
    /// No site could carry a crash.
    CrashNoSite,
    /// Reach moved during measurement.
    ReachMoved,
    /// A knob could not be put.
    KnobNotPut,
    /// A knob had no usable control.
    KnobNotCompared,
    /// Schedules were not explored.
    ScheduleNotExplored,
    /// Only a sample of schedules was explored.
    ScheduleSampled,
    /// A schedule control settled nothing.
    ScheduleUndecided,
}

impl Limitation {
    /// The stable name carried by a report.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::WorkspaceDigestNotComputed => WORKSPACE_DIGEST_NOT_COMPUTED,
            Self::TreeWrittenDuringMeasurement => TREE_WRITTEN_DURING_MEASUREMENT,
            Self::ResumedFromCheckpoint => RESUMED_FROM_CHECKPOINT,
            Self::TempDirectoryUnclaimed => TEMP_DIRECTORY_UNCLAIMED,
            Self::GitMetadataUnavailable => GIT_METADATA_UNAVAILABLE,
            Self::TargetRustflagsNotMerged => TARGET_RUSTFLAGS_NOT_MERGED,
            Self::ProcMacroExpansionNotMeasured => PROC_MACRO_EXPANSION_NOT_MEASURED,
            Self::SoundnessNotExecuted => SOUNDNESS_NOT_EXECUTED,
            Self::SoundnessSourceUnreadable => SOUNDNESS_SOURCE_UNREADABLE,
            Self::MiriUnsupported => MIRI_UNSUPPORTED,
            Self::MiriTimedOut => MIRI_TIMED_OUT,
            Self::MiriUnavailable => MIRI_UNAVAILABLE,
            Self::MiriRanNoTest => MIRI_RAN_NO_TEST,
            Self::SanitizerUnavailable => SANITIZER_UNAVAILABLE,
            Self::SanitizerStandardLibraryNotInstrumented => {
                SANITIZER_STANDARD_LIBRARY_NOT_INSTRUMENTED
            }
            Self::FuzzNotExecuted => FUZZ_NOT_EXECUTED,
            Self::CargoFuzzUnavailable => CARGO_FUZZ_UNAVAILABLE,
            Self::GenerationProviderUnavailable => GENERATION_PROVIDER_UNAVAILABLE,
            Self::GenerationCandidateNotKept => GENERATION_CANDIDATE_NOT_KEPT,
            Self::ResourceNotStopped => RESOURCE_NOT_STOPPED,
            Self::SeamNotWatched => SEAM_NOT_WATCHED,
            Self::WireFaultNotPut => WIRE_FAULT_NOT_PUT,
            Self::WireBaselineNotGreen => WIRE_BASELINE_NOT_GREEN,
            Self::WireTransportIncomplete => WIRE_TRANSPORT_INCOMPLETE,
            Self::DriftNotMeasured => DRIFT_NOT_MEASURED,
            Self::FaultNotPut => FAULT_NOT_PUT,
            Self::FaultNoSite => FAULT_NO_SITE,
            Self::CrashNotPut => CRASH_NOT_PUT,
            Self::CrashNoSite => CRASH_NO_SITE,
            Self::ReachMoved => REACH_MOVED,
            Self::KnobNotPut => KNOB_NOT_PUT,
            Self::KnobNotCompared => KNOB_NOT_COMPARED,
            Self::ScheduleNotExplored => SCHEDULE_NOT_EXPLORED,
            Self::ScheduleSampled => SCHEDULE_SAMPLED,
            Self::ScheduleUndecided => SCHEDULE_UNDECIDED,
        }
    }
}

/// Every runner limitation name, derived from the closed enum in reader order.
#[cfg(feature = "testkit")]
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

/// A report limitation named by the runner, engine, or a skipped syntax site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Name {
    /// A runner limitation.
    Runner(Limitation),
    /// An engine limitation.
    Engine(rust_mutants::limitation::Limitation),
    /// A skipped syntax site.
    Skipped(rust_mutants::syntax::SkipReason),
}

impl Name {
    /// The stable name carried by the report.
    #[must_use]
    pub fn name(self) -> String {
        match self {
            Self::Runner(limitation) => limitation.name().to_owned(),
            Self::Engine(limitation) => limitation.name().to_owned(),
            Self::Skipped(reason) => format!("skipped-{}", reason.name()),
        }
    }
}

impl From<Limitation> for Name {
    fn from(limitation: Limitation) -> Self {
        Self::Runner(limitation)
    }
}

impl From<rust_mutants::limitation::Limitation> for Name {
    fn from(limitation: rust_mutants::limitation::Limitation) -> Self {
        Self::Engine(limitation)
    }
}

impl From<rust_mutants::syntax::SkipReason> for Name {
    fn from(reason: rust_mutants::syntax::SkipReason) -> Self {
        Self::Skipped(reason)
    }
}

/// A name no layer of this release can state.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown report limitation {0:?}")]
pub struct NameError(pub String);

impl FromStr for Name {
    type Err = NameError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        if let Some(limitation) = Limitation::ALL
            .into_iter()
            .find(|limitation| limitation.name() == name)
        {
            return Ok(Self::Runner(limitation));
        }
        if let Some(limitation) = rust_mutants::limitation::Limitation::ALL
            .into_iter()
            .find(|limitation| limitation.name() == name)
        {
            return Ok(Self::Engine(limitation));
        }
        if let Some(reason) = name.strip_prefix("skipped-").and_then(|name| {
            rust_mutants::syntax::SkipReason::ALL
                .into_iter()
                .find(|reason| reason.name() == name)
        }) {
            return Ok(Self::Skipped(reason));
        }
        Err(NameError(name.to_owned()))
    }
}
