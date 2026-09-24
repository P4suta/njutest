// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every limitation the runner can state, in one place.

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

/// A target's baseline was measured and no original-code control over the same passing tests recorded what it reached, so whether its reach is a function of the target is not known.
pub const DRIFT_NOT_MEASURED: &str = "drift-not-measured";

/// A run asked for faults could not put some, because the compiler refused them: their sites propagate an error type the engine does not make.
pub const FAULT_NOT_PUT: &str = "fault-not-put";

/// A knob was asked for and not put on a target, so nothing is claimed about whether the target depends on what it sets.
pub const KNOB_NOT_PUT: &str = "knob-not-put";

/// A control under a knob established nothing to compare, so whether a target's verdict and reach hold there is not known.
pub const KNOB_NOT_COMPARED: &str = "knob-not-compared";

/// A run asked for faults found no `?` in a measured file, so there was no call to fail.
pub const FAULT_NO_SITE: &str = "fault-no-site";

/// Every limitation this runner states of its own, in the order a reader meets them in a run.
#[cfg(feature = "testkit")]
pub const ALL: [&str; 24] = [
    WORKSPACE_DIGEST_NOT_COMPUTED,
    TREE_WRITTEN_DURING_MEASUREMENT,
    RESUMED_FROM_CHECKPOINT,
    TEMP_DIRECTORY_UNCLAIMED,
    GIT_METADATA_UNAVAILABLE,
    TARGET_RUSTFLAGS_NOT_MERGED,
    PROC_MACRO_EXPANSION_NOT_MEASURED,
    SOUNDNESS_NOT_EXECUTED,
    SOUNDNESS_SOURCE_UNREADABLE,
    MIRI_UNSUPPORTED,
    MIRI_TIMED_OUT,
    SANITIZER_UNAVAILABLE,
    SANITIZER_STANDARD_LIBRARY_NOT_INSTRUMENTED,
    FUZZ_NOT_EXECUTED,
    CARGO_FUZZ_UNAVAILABLE,
    GENERATION_PROVIDER_UNAVAILABLE,
    GENERATION_CANDIDATE_NOT_KEPT,
    RESOURCE_NOT_STOPPED,
    SEAM_NOT_WATCHED,
    DRIFT_NOT_MEASURED,
    FAULT_NOT_PUT,
    FAULT_NO_SITE,
    KNOB_NOT_PUT,
    KNOB_NOT_COMPARED,
];
