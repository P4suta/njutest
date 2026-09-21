// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test support this crate's own suite and a sibling's may reach for.

#![expect(
    clippy::unreachable,
    reason = "three of these samples are made by asking the library to refuse something it always refuses; a sample that came back as a success would mean the library stopped refusing it, which is a failure to report loudly rather than to carry"
)]

#[cfg(feature = "testkit")]
use std::path::Path;

#[cfg(feature = "testkit")]
use crate::error::RunnerError;

/// Closed classification returned by the feature-gated Kani export parser
/// fuzz boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(feature = "testkit")]
pub enum ModelResultClass {
    /// The complete pinned document proves equality.
    Proved,
    /// The complete pinned document carries the one tagged counterexample.
    Noticed,
    /// Every malformed, incomplete, or non-affirmative document fails closed.
    Undecided,
}

/// Feeds arbitrary bytes through the production Kani 0.68 result parser.
///
/// It uses a fixed, closed expectation. Exposed only by the `testkit` feature so the
/// fuzz crate exercises the real strict parser without widening normal API.
#[must_use]
#[cfg(feature = "testkit")]
pub fn model_result(bytes: &[u8]) -> ModelResultClass {
    let harness = crate::assure::model::parser_fixture();
    let parsed = crate::assure::model::result::parse(
        bytes,
        crate::assure::model::result::Expectation {
            harness: &harness,
            target: "test-target",
            root: Path::new("/fixture"),
            target_dir: Path::new("/fixture/target/kani"),
            package: "fixture",
        },
    );
    match parsed.decision {
        crate::assure::model::result::Decision::Proved => ModelResultClass::Proved,
        crate::assure::model::result::Decision::Noticed => ModelResultClass::Noticed,
        crate::assure::model::result::Decision::Undecided(_) => ModelResultClass::Undecided,
    }
}

/// Drives the watch loop with a caller-controlled wait.
#[cfg(feature = "testkit")]
pub fn watch_until_with_wait<L, R, W>(
    cancel: &rust_mutants::runner::Cancel,
    look: L,
    round: R,
    waiting: (std::time::Duration, W),
) -> u8
where
    L: FnMut() -> Option<crate::app::watch::Seen>,
    R: FnMut() -> u8,
    W: FnMut(std::time::Duration),
{
    let mut look = look;
    let mut round = round;
    let watched = crate::app::watch::until_with_wait(
        cancel,
        || Ok::<_, std::convert::Infallible>(look()),
        || Ok::<_, std::convert::Infallible>(round()),
        waiting,
    );
    match watched {
        Ok(code) => code,
        Err(never) => match never {},
    }
}

/// One failure of every shape this runner reports.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "the function is the closed inventory of every RunnerError shape; splitting it would let the inventory and constructor drift apart"
)]
#[cfg(feature = "testkit")]
pub fn every_failure() -> Vec<RunnerError> {
    fn failure<T, E>(result: Result<T, E>, message: &'static str) -> E {
        match result {
            Ok(_) => unreachable!("{message}"),
            Err(error) => error,
        }
    }

    let nowhere = Path::new("nowhere");
    let invalid_utf8 = vec![0xff];
    let invalid_utf8 = failure(
        std::str::from_utf8(&invalid_utf8),
        "0xff is not a valid UTF-8 document",
    );
    let failures = vec![
        RunnerError::Interrupted,
        RunnerError::Config(failure(
            crate::config::Config::parse("version = 9\n", nowhere),
            "nine is not a version this release knows",
        )),
        RunnerError::Target(crate::targets::TargetError::new(
            crate::targets::TargetErrorKind::ListFailed,
            "pkg/lib/pkg",
            "the binary said nothing",
        )),
        RunnerError::Evidence(crate::evidence::tree::ScanError::Unreadable {
            path: nowhere.to_path_buf(),
            source: std::io::Error::other("no"),
        }),
        RunnerError::Cache(crate::cache::store::CacheError::Refused {
            message: "a report with no identity answers for no inputs".to_owned(),
        }),
        RunnerError::Coverage(
            failure(
                rust_mutants::coverage::parse_export(b"not an export"),
                "that is not an export",
            )
            .into(),
        ),
        RunnerError::Provider(crate::provider::ProviderError::new(
            crate::provider::ProviderErrorKind::Unstartable,
            "no such command",
        )),
        RunnerError::IdentityEnvironment {
            source: crate::assure::identity::EnvironmentError::Value {
                name: "RUSTFLAGS".to_owned(),
                source: invalid_utf8,
            },
        },
        RunnerError::MutationText {
            source: crate::assure::mutation::MutationTextError::Original {
                source: invalid_utf8,
            },
        },
        RunnerError::MiriMissing {
            message: "the toolchain has no miri".to_owned(),
        },
        RunnerError::PhaseOutput {
            phase: "fixture",
            source: invalid_utf8,
        },
        RunnerError::Resource(crate::resource::ResourceError::EnvironmentRefused {
            capability: "postgres".to_owned(),
            name: "RUSTFLAGS".to_owned(),
        }),
        RunnerError::Report(failure(
            crate::report::json::parse("{}"),
            "an empty object is not a report",
        )),
        RunnerError::Scratch(crate::scratch::ScratchError::Unusable {
            path: nowhere.to_path_buf(),
            source: std::io::Error::other("no"),
        }),
        RunnerError::Build(crate::build::BuildError::NotRun {
            message: "cargo would not start".to_owned(),
        }),
        RunnerError::Engine(rust_mutants::EngineError::Interrupted),
        RunnerError::Checkpoint(crate::checkpoint::CheckpointError::Unusable {
            path: nowhere.to_path_buf(),
            source: std::io::Error::other("no"),
        }),
        RunnerError::MutationEvidence(crate::evidence::store::StoreError::Unusable {
            path: nowhere.to_path_buf(),
            source: std::io::Error::other("no"),
        }),
        RunnerError::Model {
            message: "model artifact could not be retained".to_owned(),
        },
        RunnerError::Schedule(crate::assure::schedule::ScheduleError::WorkerPanicked),
        RunnerError::Equivalence {
            source: crate::assure::equivalence::EquivalenceError::DuplicateDecision {
                display_id: "abcdef".to_owned(),
            },
        },
        RunnerError::Output {
            source: std::io::Error::other("output closed"),
        },
    ];
    for one in &failures {
        match one {
            RunnerError::Interrupted
            | RunnerError::Output { .. }
            | RunnerError::RunInvariant { .. }
            | RunnerError::WireIdentity { .. }
            | RunnerError::Config(_)
            | RunnerError::Target(_)
            | RunnerError::Evidence(_)
            | RunnerError::Cache(_)
            | RunnerError::Checkpoint(_)
            | RunnerError::MutationEvidence(_)
            | RunnerError::Coverage(_)
            | RunnerError::Provider(_)
            | RunnerError::IdentityEnvironment { .. }
            | RunnerError::MutationText { .. }
            | RunnerError::MiriMissing { .. }
            | RunnerError::PhaseOutput { .. }
            | RunnerError::Model { .. }
            | RunnerError::Schedule(_)
            | RunnerError::Equivalence { .. }
            | RunnerError::Resource(_)
            | RunnerError::Report(_)
            | RunnerError::ReportCount { .. }
            | RunnerError::Scratch(_)
            | RunnerError::Build(_)
            | RunnerError::Engine(_) => {}
        }
    }
    failures
}

/// One refusal of every shape the evidence layer can record.
#[must_use]
#[cfg(feature = "testkit")]
pub fn every_refusal() -> Vec<crate::evidence::store::Refusal> {
    use crate::evidence::store::Refusal;
    let refusals = vec![
        Refusal::Nothing,
        Refusal::Unreadable {
            message: "the record does not parse".to_owned(),
        },
        Refusal::TargetUnknown {
            target: "core/lib/core".to_owned(),
        },
        Refusal::NotRouted {
            target: "core/lib/core".to_owned(),
        },
        Refusal::KeyChanged {
            target: "core/lib/core".to_owned(),
        },
        Refusal::NotPassing {
            target: "core/lib/core".to_owned(),
        },
        Refusal::TargetEntered {
            target: "core/lib/core".to_owned(),
        },
        Refusal::NothingRouted,
    ];
    for one in &refusals {
        match one {
            Refusal::Nothing
            | Refusal::Unreadable { .. }
            | Refusal::TargetUnknown { .. }
            | Refusal::NotRouted { .. }
            | Refusal::KeyChanged { .. }
            | Refusal::NotPassing { .. }
            | Refusal::TargetEntered { .. }
            | Refusal::NothingRouted => {}
        }
    }
    refusals
}

/// A routing with every optional field present, for the serialized-field
/// ledger.
#[cfg(feature = "testkit")]
fn complete_route() -> crate::trace::RouteRecord {
    crate::trace::RouteRecord {
        mutant: "abcdef".to_owned(),
        granularity: rust_mutants::session::Granularity::Block,
        fallback: Some(rust_mutants::session::Fallback::TouchIncomplete),
        reaching: vec!["demo/lib/demo".to_owned()],
        tests: vec![crate::trace::AskedRecord {
            target: "demo/lib/demo".to_owned(),
            tests: vec!["tests::one".to_owned()],
        }],
        discharged: vec![crate::trace::DischargeRecord {
            target: "demo/test/proved".to_owned(),
            proof: "never-infected".to_owned(),
        }],
        considered: vec!["demo/test/other".to_owned()],
        reused: Some("earlier-run".to_owned()),
        refused: Some("key-changed".to_owned()),
    }
}

/// The wrapper whose object is the record documented for `payload`.
///
/// This match is deliberately exhaustive: a new payload shape has no field
/// ledger until its record wrapper is named here, so it fails compilation
/// instead of inheriting a row by default.
#[must_use]
#[cfg(feature = "testkit")]
pub const fn payload_record_key(payload: &crate::trace::Payload) -> &'static str {
    use crate::trace::Payload;

    match payload {
        Payload::RunStart { .. } => "start",
        Payload::PhaseStart { .. } | Payload::PhaseEnd { .. } => "phase",
        Payload::Exec { .. } => "exec",
        Payload::Progress { .. } => "progress",
        Payload::Artifact { .. } => "artifact",
        Payload::Route { .. } => "route",
        Payload::MutantExec { .. } => "mutant",
        Payload::ProbeExec { .. } => "probe",
        Payload::WireExchange { .. } => "exchange",
        Payload::WireExec { .. } => "wire",
        Payload::Model { .. } => "model",
        Payload::Note { .. } => "note",
        Payload::RunEnd { .. } => "run",
    }
}

/// Exhaustive, read-only projections of trace payloads for integration tests.
///
/// Each projection names every non-selected variant. Adding a payload variant
/// therefore breaks compilation instead of making a test oracle silently skip
/// the new event.
pub mod payload {
    #[cfg(feature = "testkit")]
    use crate::trace::Payload;

    /// One exhaustively classified payload borrowed from a trace event.
    #[derive(Debug, Clone, Copy)]
    #[cfg(feature = "testkit")]
    pub enum Ref<'a> {
        /// A run-start record.
        RunStart,
        /// The opening half of a phase.
        PhaseStart(&'a crate::trace::PhaseRecord),
        /// The closing half of a phase.
        PhaseEnd(&'a crate::trace::PhaseRecord),
        /// A process execution.
        Exec(&'a crate::trace::ExecRecord),
        /// A progress observation.
        Progress(&'a crate::trace::ProgressRecord),
        /// A retained artifact.
        Artifact,
        /// A mutation route.
        Route(&'a crate::trace::RouteRecord),
        /// A mutation execution.
        MutantExec(&'a crate::trace::MutantExecRecord),
        /// A probe execution.
        ProbeExec(&'a crate::trace::ProbeExecRecord),
        /// A wire exchange.
        WireExchange,
        /// A wire decision.
        WireExec,
        /// A model decision.
        Model,
        /// A note.
        Note(&'a crate::trace::NoteRecord),
        /// A run-end record.
        RunEnd,
    }

    /// Classifies every payload variant without a catch-all arm.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn of(payload: &Payload) -> Ref<'_> {
        match payload {
            Payload::RunStart { .. } => Ref::RunStart,
            Payload::PhaseStart { phase } => Ref::PhaseStart(phase),
            Payload::PhaseEnd { phase } => Ref::PhaseEnd(phase),
            Payload::Exec { exec } => Ref::Exec(exec),
            Payload::Progress { progress } => Ref::Progress(progress),
            Payload::Artifact { .. } => Ref::Artifact,
            Payload::Route { route } => Ref::Route(route),
            Payload::MutantExec { mutant } => Ref::MutantExec(mutant),
            Payload::ProbeExec { probe } => Ref::ProbeExec(probe),
            Payload::WireExchange { .. } => Ref::WireExchange,
            Payload::WireExec { .. } => Ref::WireExec,
            Payload::Model { .. } => Ref::Model,
            Payload::Note { note } => Ref::Note(note),
            Payload::RunEnd { .. } => Ref::RunEnd,
        }
    }

    #[cfg(feature = "testkit")]
    impl<'a> Ref<'a> {
        /// The execution record, where this is one.
        #[must_use]
        #[cfg(feature = "testkit")]
        pub const fn exec(self) -> Option<&'a crate::trace::ExecRecord> {
            let Self::Exec(exec) = self else {
                return None;
            };
            Some(exec)
        }

        /// The note record, where this is one.
        #[must_use]
        pub const fn note(self) -> Option<&'a crate::trace::NoteRecord> {
            let Self::Note(note) = self else {
                return None;
            };
            Some(note)
        }

        /// The route record, where this is one.
        #[must_use]
        pub const fn route(self) -> Option<&'a crate::trace::RouteRecord> {
            let Self::Route(route) = self else {
                return None;
            };
            Some(route)
        }

        /// The mutation execution record, where this is one.
        #[must_use]
        pub const fn mutant_exec(self) -> Option<&'a crate::trace::MutantExecRecord> {
            let Self::MutantExec(mutant) = self else {
                return None;
            };
            Some(mutant)
        }

        /// The probe execution record, where this is one.
        #[must_use]
        pub const fn probe_exec(self) -> Option<&'a crate::trace::ProbeExecRecord> {
            let Self::ProbeExec(probe) = self else {
                return None;
            };
            Some(probe)
        }

        /// The progress record, where this is one.
        #[must_use]
        pub const fn progress(self) -> Option<&'a crate::trace::ProgressRecord> {
            let Self::Progress(progress) = self else {
                return None;
            };
            Some(progress)
        }

        /// The opening phase record, where this is one.
        #[must_use]
        pub const fn phase_start(self) -> Option<&'a crate::trace::PhaseRecord> {
            let Self::PhaseStart(phase) = self else {
                return None;
            };
            Some(phase)
        }

        /// The closing phase record, where this is one.
        #[must_use]
        pub const fn phase_end(self) -> Option<&'a crate::trace::PhaseRecord> {
            let Self::PhaseEnd(phase) = self else {
                return None;
            };
            Some(phase)
        }

        /// Either half of a phase pair.
        #[must_use]
        pub const fn phase(self) -> Option<&'a crate::trace::PhaseRecord> {
            if let Self::PhaseStart(phase) = self {
                return Some(phase);
            }
            let Self::PhaseEnd(phase) = self else {
                return None;
            };
            Some(phase)
        }
    }
}

/// Closed specimens whose union serializes every top-level field of every
/// trace record.
///
/// Optional fields are present, and the flattened [`crate::trace::Read`] and
/// [`crate::report::SeamDecision`] sets plus every
/// [`rust_mutants::execute::Stopped`] variant are named in full. Extending any
/// of those enums therefore makes this testkit fail to compile until the new
/// wire shape has a specimen.
///
/// # Panics
/// A specimen lacks its required record key, contradicting the closed
/// inventory this function constructs.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "its length is the size of a closed set named in full, so splitting it would \
              hide the only property it has: that every shape is here. A shorter version \
              is one where a missing variant is harder to see."
)]
#[cfg(feature = "testkit")]
pub fn every_payload() -> Vec<crate::trace::Payload> {
    use crate::trace::{
        ArtifactRecord, ExecRecord, MutantExecRecord, NoteRecord, Payload, PhaseRecord,
        ProbeExecRecord, ProgressRecord, RunRecord, StartRecord, WireExchangeRecord,
        WireExecRecord,
    };
    let phase = || PhaseRecord {
        name: "baseline".to_owned(),
        duration_ms: Some(7),
    };
    let mut payloads = vec![
        Payload::RunStart {
            start: StartRecord::of(
                "20270115T080000Z-aaaaaa",
                crate::report::RunKind::Full,
                crate::config::Contract::StandardV1,
            ),
        },
        Payload::PhaseStart { phase: phase() },
        Payload::PhaseEnd { phase: phase() },
        Payload::Progress {
            progress: ProgressRecord {
                subject: "abcdef".to_owned(),
                message: "measuring".to_owned(),
                done: Some(1),
                total: Some(2),
            },
        },
        Payload::Artifact {
            artifact: ArtifactRecord {
                kind: "snapshot".to_owned(),
                path: "nowhere".to_owned(),
                bytes: Some(1),
            },
        },
        Payload::Route {
            route: complete_route(),
        },
        Payload::MutantExec {
            mutant: MutantExecRecord {
                mutant: "abcdef".to_owned(),
                target: "demo/lib/demo".to_owned(),
                args: vec!["--exact".to_owned(), "tests::one".to_owned()],
                outcome: "step_limit_reached".to_owned(),
                step_boundary: crate::report::StepBoundary::new(10, 11),
                duration_ms: 5,
                alone: true,
            },
        },
        Payload::ProbeExec {
            probe: ProbeExecRecord {
                target: "demo/lib/demo".to_owned(),
                outcome: "measured".to_owned(),
                infected: Some(3),
            },
        },
        Payload::Model {
            model: Box::new(crate::report::ModelRecord::specimen_ineligible(
                crate::report::ModelIneligibility::Effect,
            )),
        },
        Payload::Note {
            note: NoteRecord {
                kind: "a".to_owned(),
                detail: "one".to_owned(),
            },
        },
        Payload::RunEnd {
            run: RunRecord {
                verdict: "ERROR".to_owned(),
                accounting: None,
                error: Some("one failure".to_owned()),
                events_emitted: 10,
                events_dropped: 1,
            },
        },
    ];
    payloads.extend(
        rust_mutants::testkit::trace::every_stopped()
            .into_iter()
            .map(|stopped| Payload::Exec {
                exec: ExecRecord {
                    argv: vec!["cargo".to_owned(), "test".to_owned()],
                    dir: Some("/workspace".to_owned()),
                    env_names: vec!["RUSTFLAGS".to_owned()],
                    timeout_ms: Some(30_000),
                    stopped,
                    duration_ms: 5,
                    output_bytes: 6,
                    output_sha256: Some("d".repeat(64)),
                    output_truncated: true,
                    output_path: Some("output/1.txt".to_owned()),
                    error: Some("one error".to_owned()),
                    output: b"capture".to_vec(),
                },
            }),
    );
    payloads.extend(every_read().into_iter().map(|read| Payload::WireExchange {
        exchange: WireExchangeRecord {
            capability: "http".to_owned(),
            seq: 1,
            during: Some("demo/lib/demo".to_owned()),
            duration_ms: 2,
            read,
            request_bytes: 3,
            response_bytes: 4,
        },
    }));
    payloads.extend(
        every_seam_decision()
            .into_iter()
            .map(|decision| Payload::WireExec {
                wire: WireExecRecord {
                    fault: "fault".to_owned(),
                    capability: "http".to_owned(),
                    seq: 1,
                    rule: crate::wire::rule::Rule::DropConnection,
                    decision,
                },
            }),
    );
    for payload in &payloads {
        assert!(
            !payload_record_key(payload).is_empty(),
            "every payload must name its containing record"
        );
    }
    payloads
}

/// Every flattened read shape.
#[cfg(feature = "testkit")]
fn every_read() -> [crate::trace::Read; 2] {
    use crate::trace::Read;

    let reads = [
        Read::Raw,
        Read::Http {
            method: "GET".to_owned(),
            path: "/orders".to_owned(),
            status: 200,
        },
    ];
    for read in &reads {
        match read {
            Read::Raw | Read::Http { .. } => {}
        }
    }
    reads
}

/// Every flattened seam-decision shape.
#[cfg(feature = "testkit")]
fn every_seam_decision() -> [crate::report::SeamDecision; 4] {
    use crate::report::SeamDecision;

    let decisions = [
        SeamDecision::Tests {
            noticed_by: "demo/lib/demo".to_owned(),
        },
        SeamDecision::Proved {
            proof: "no-body-to-cut".to_owned(),
        },
        SeamDecision::Unnoticed,
        SeamDecision::Unreached,
    ];
    for decision in &decisions {
        match decision {
            SeamDecision::Tests { .. }
            | SeamDecision::Proved { .. }
            | SeamDecision::Unnoticed
            | SeamDecision::Unreached => {}
        }
    }
    decisions
}
