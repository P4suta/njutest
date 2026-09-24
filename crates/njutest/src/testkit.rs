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

/// Closed classification returned by the feature-gated Kani export parser fuzz boundary.
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
/// It uses a fixed, closed expectation.
/// Exposed only by the `testkit` feature so the fuzz crate exercises the real strict parser without widening normal API.
#[must_use]
#[cfg(feature = "testkit")]
pub fn model_result(bytes: &[u8]) -> ModelResultClass {
    /// The fixture's workspace root, absolute the way this platform means it, because a leading slash is not absolute on Windows without a drive.
    const FIXTURE_ROOT: &str = if cfg!(windows) {
        "C:/fixture"
    } else {
        "/fixture"
    };

    let harness = crate::assure::model::parser_fixture();
    let parsed = crate::assure::model::result::parse(
        bytes,
        crate::assure::model::result::Expectation {
            harness: &harness,
            target: "test-target",
            root: Path::new(FIXTURE_ROOT),
            target_dir: &std::path::PathBuf::from(format!("{FIXTURE_ROOT}/target/kani")),
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
        RunnerError::Blind {
            layer: rust_mutants::sentinel::Planted::Reach,
            mutant: "src/lib.rs:one:return-default".to_owned(),
            expected: "unreached".to_owned(),
            routed: "test".to_owned(),
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
            | RunnerError::Blind { .. }
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

/// A routing with every optional field present, for the serialized-field ledger.
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
/// This match is deliberately exhaustive: a new payload shape has no field ledger until its record wrapper is named here, so it fails compilation instead of inheriting a row by default.
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
        Payload::FaultExec { .. } | Payload::Fault { .. } => "fault",
        Payload::ProbeExec { .. } => "probe",
        Payload::WireExchange { .. } => "exchange",
        Payload::WireExec { .. } => "wire",
        Payload::Sentinel { .. } => "sentinel",
        Payload::Model { .. } => "model",
        Payload::Drift { .. } => "drift",
        Payload::Note { .. } => "note",
        Payload::RunEnd { .. } => "run",
    }
}

/// Exhaustive, read-only projections of trace payloads for integration tests.
///
/// Each projection names every non-selected variant.
/// Adding a payload variant therefore breaks compilation instead of making a test oracle silently skip the new event.
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
        /// A fault execution.
        FaultExec(&'a crate::trace::FaultExecRecord),
        /// A fault site's decision.
        Fault(&'a crate::report::faults::FaultRecord),
        /// A probe execution.
        ProbeExec(&'a crate::trace::ProbeExecRecord),
        /// A wire exchange.
        WireExchange,
        /// A wire decision.
        WireExec,
        /// A routing layer's sentinel.
        Sentinel(&'a crate::trace::SentinelRecord),
        /// A model decision.
        Model,
        /// A control's drift observation.
        Drift(&'a crate::trace::DriftRecord),
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
            Payload::FaultExec { fault } => Ref::FaultExec(fault),
            Payload::Fault { fault } => Ref::Fault(fault),
            Payload::ProbeExec { probe } => Ref::ProbeExec(probe),
            Payload::WireExchange { .. } => Ref::WireExchange,
            Payload::WireExec { .. } => Ref::WireExec,
            Payload::Sentinel { sentinel } => Ref::Sentinel(sentinel),
            Payload::Model { .. } => Ref::Model,
            Payload::Drift { drift } => Ref::Drift(drift),
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

        /// The drift record, where this is one.
        #[must_use]
        pub const fn drift(self) -> Option<&'a crate::trace::DriftRecord> {
            let Self::Drift(drift) = self else {
                return None;
            };
            Some(drift)
        }

        /// The note record, where this is one.
        #[must_use]
        pub const fn note(self) -> Option<&'a crate::trace::NoteRecord> {
            let Self::Note(note) = self else {
                return None;
            };
            Some(note)
        }

        /// The sentinel record, where this is one.
        #[must_use]
        pub const fn sentinel(self) -> Option<&'a crate::trace::SentinelRecord> {
            let Self::Sentinel(sentinel) = self else {
                return None;
            };
            Some(sentinel)
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

/// Closed specimens whose union serializes every top-level field of every trace record.
///
/// Optional fields are present, and the flattened [`crate::trace::Read`] and [`crate::report::SeamDecision`] sets plus every [`rust_mutants::execute::Stopped`] variant are named in full.
/// Extending any of those enums therefore makes this testkit fail to compile until the new wire shape has a specimen.
///
/// # Panics
/// A specimen lacks its required record key, contradicting the closed inventory this function constructs.
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
        Payload::FaultExec {
            fault: crate::trace::FaultExecRecord {
                fault: "abcdef".to_owned(),
                target: "demo/test/calls".to_owned(),
                args: vec!["--exact".to_owned(), "tests::one".to_owned()],
                outcome: "killed".to_owned(),
                duration_ms: 5,
                alone: false,
            },
        },
        Payload::Fault {
            fault: crate::report::faults::FaultRecord {
                catalog_index: crate::report::CatalogIndex::new(0),
                id: "a".repeat(64),
                display_id: "a".repeat(20),
                path: "src/lib.rs".to_owned(),
                item: "load".to_owned(),
                position: Some(crate::report::Position {
                    line: 13,
                    column: 16,
                    character_column: 16,
                }),
                decision: crate::report::faults::FaultDecision::Noticed {
                    by: "demo/test/calls".to_owned(),
                },
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
        Payload::Drift {
            drift: crate::trace::DriftRecord {
                mutant: "abcdef".to_owned(),
                observed: crate::report::drift::Drift::Moved {
                    target: "demo/lib/demo".to_owned(),
                    reached: crate::report::drift::Moved {
                        gained: std::collections::BTreeSet::from([2]),
                        lost: std::collections::BTreeSet::from([1]),
                    },
                    bodies: crate::report::drift::Moved {
                        gained: std::collections::BTreeSet::new(),
                        lost: std::collections::BTreeSet::new(),
                    },
                    infected: crate::report::drift::Moved {
                        gained: std::collections::BTreeSet::new(),
                        lost: std::collections::BTreeSet::new(),
                    },
                },
            },
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
    payloads.extend(
        rust_mutants::sentinel::Planted::every()
            .into_iter()
            .flat_map(rust_mutants::sentinel::Planted::expectations)
            .map(|expectation| Payload::Sentinel {
                sentinel: crate::trace::SentinelRecord {
                    layer: expectation.planted,
                    mutant: expectation.mutant.to_string(),
                    expected: expectation.expected,
                    routed: "test".to_owned(),
                    sighted: false,
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

/// A configuration with one member in every collection it has, so a ledger that walks one sees every key a reader may write.
///
/// The key ledger read `Config::default()`, whose collections are empty: the eight keys of a `[resources.*]` table, the three of `[generation]`, and the ten of an `[[acceptance]]` entry were invisible to it, documented or not.
/// No `..Default::default()` appears below, so a field added to any of these is a field somebody has to give a value here before the tree compiles.
#[cfg(feature = "testkit")]
#[must_use]
pub fn documented_specimen() -> crate::config::Config {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use crate::config::{
        Acceptance, Cache, Config, Configuration, Contract, Execution, Faults, Fuzz, Generation,
        Mutation, Project, Reports, Resource, Soundness, Verification,
    };

    Config {
        version: 1,
        contract: Contract::default(),
        project: Project {
            packages: vec!["demo".to_owned()],
            include: vec!["src/**/*.rs".to_owned()],
            exclude: vec!["src/generated/**".to_owned()],
        },
        execution: Execution {
            features: vec!["slow".to_owned()],
            all_features: false,
            no_default_features: false,
            test_binary_args: vec!["--test-threads=1".to_owned()],
            environment: vec!["RUST_LOG=info".to_owned()],
            timeout: Duration::from_mins(10),
            steps: 50_000_000,
            build_timeout: Some(Duration::from_mins(15)),
            jobs: 1,
            skip_targets: vec!["demo/lib/demo".to_owned()],
            coverage: true,
        },
        cache: Cache {
            max_bytes: 5_368_709_120,
            ttl: Duration::from_hours(24 * 30),
        },
        mutation: Mutation { equivalence: true },
        faults: Faults { inject: true },
        verification: Verification {
            unwind: Some(8),
            timeout: Some(Duration::from_mins(2)),
        },
        reports: Reports {
            keep: 20,
            directory: match crate::config::ReportDirectory::try_from("reports") {
                Ok(directory) => directory,
                Err(refusal) => unreachable!("the default report directory: {refusal}"),
            },
        },
        soundness: Soundness {
            miri_flags: vec!["-Zmiri-strict-provenance".to_owned()],
            sanitizers: vec!["address".to_owned()],
        },
        fuzz: Fuzz {
            run: true,
            max_total_time: Duration::from_secs(60),
            targets: vec!["libtest_summary".to_owned()],
        },
        resources: BTreeMap::from([(
            "api".to_owned(),
            Resource {
                command: vec!["docker".to_owned(), "compose".to_owned(), "up".to_owned()],
                timeout: Duration::from_secs(60),
                shared: true,
                exclusive: false,
                environment: vec!["BASE_URL=http://127.0.0.1:8080".to_owned()],
                interpose: "BASE_URL".to_owned(),
                wire: crate::wire::Wire::Http,
                hold: Duration::from_secs(30),
            },
        )]),
        generation: Some(Generation {
            command: vec!["write-tests".to_owned()],
            allowed_paths: vec!["tests/**".to_owned()],
            environment: vec!["MODEL=none".to_owned()],
        }),
        acceptance: vec![Acceptance {
            id: "0123456789abcdef".to_owned(),
            path: Some("src/lib.rs".to_owned()),
            item: Some("demo::add".to_owned()),
            rule: Some("add-to-sub".to_owned()),
            original: Some("a + b".to_owned()),
            line: Some(12),
            reason: "the difference is unobservable through the public surface".to_owned(),
            expires: Some(jiff::Timestamp::UNIX_EPOCH),
            owner: Some("a reviewer".to_owned()),
            ticket: Some("NJU-1".to_owned()),
        }],
        configuration: vec![Configuration {
            name: "no-default".to_owned(),
            features: vec!["slow".to_owned()],
            all_features: false,
            no_default_features: true,
            profile: Some("release".to_owned()),
            target: Some("x86_64-unknown-linux-gnu".to_owned()),
        }],
    }
}

/// One value of every shape a proof's uncertainty takes.
///
/// The schema splits this one tagged set across nine branches, eight of them a single `kind`, and nothing on this side was held to the nine.
/// The match below decides nothing; it is what makes the compiler refuse this function the day somebody adds a variant.
#[cfg(feature = "testkit")]
#[must_use]
pub fn every_model_uncertainty() -> Vec<crate::report::ModelUncertainty> {
    use crate::report::{
        ModelAffirmative, ModelArtifactFailure, ModelConfiguration, ModelProcessFailure,
        ModelPropertyStatus, ModelProtocol, ModelToolFailure, ModelUncertainty,
    };

    let every = vec![
        ModelUncertainty::BoundExhausted,
        ModelUncertainty::Cutoff,
        ModelUncertainty::Cancelled,
        ModelUncertainty::Configuration(ModelConfiguration::Package),
        ModelUncertainty::Tool(ModelToolFailure::Unavailable),
        ModelUncertainty::Process(ModelProcessFailure::NotStarted),
        ModelUncertainty::Artifact(ModelArtifactFailure::Missing),
        ModelUncertainty::ExitMismatch {
            expected: ModelAffirmative::Proved,
            actual: 1,
        },
        ModelUncertainty::Protocol(ModelProtocol::Schema),
        ModelUncertainty::Property(ModelPropertyStatus::Unknown),
        ModelUncertainty::OtherFailure("another property failed".to_owned()),
    ];
    for one in &every {
        match one {
            ModelUncertainty::BoundExhausted
            | ModelUncertainty::Cutoff
            | ModelUncertainty::Cancelled
            | ModelUncertainty::Configuration(..)
            | ModelUncertainty::Tool(..)
            | ModelUncertainty::Process(..)
            | ModelUncertainty::Artifact(..)
            | ModelUncertainty::ExitMismatch { .. }
            | ModelUncertainty::Protocol(..)
            | ModelUncertainty::Property(..)
            | ModelUncertainty::OtherFailure(..) => {}
        }
    }
    every
}

/// Complete reports assembled from mutation rows, for suites that read a report rather than write one, counted by the model's own counter.
#[cfg(feature = "testkit")]
pub mod reports {
    use crate::report::{
        Answered, BuildReport, CatalogIndex, CountError, Decided, Discharged, Established, Finding,
        Limitation, MutantRecord, Outcome, Position, Report, Reuse, Routing, RunKind, TargetRecord,
        TargetStatus,
    };

    /// Why the rows a fixture gave did not make one complete report.
    #[derive(Debug, thiserror::Error)]
    #[non_exhaustive]
    pub enum UnmadeReport {
        /// The rows do not fit the report's counters.
        #[error("the rows do not fit the report's counters: {source}")]
        Counted {
            /// What did not fit.
            #[from]
            source: CountError,
        },
        /// The builds are not one checked measurement.
        #[error("the builds are not one checked measurement: {source}")]
        Measured {
            /// Why.
            #[from]
            source: crate::report::across::BuildMeasurementsError,
        },
        /// The builds are not builds of one catalog.
        #[error("the builds are not builds of one catalog: {source}")]
        Configured {
            /// Why.
            #[from]
            source: crate::report::across::ConfiguredError,
        },
        /// The run identity is not a canonical one.
        #[error("the run identity is not canonical: {source}")]
        Named {
            /// Why.
            #[from]
            source: rust_mutants::id::RunIdError,
        },
        /// The lattice made one part of a catalog rather than a whole.
        #[error("the rows made one part of a catalog rather than a whole")]
        Part,
        /// A row names a rule the engine does not have, which no run could have written.
        #[error(
            "no rule of the engine's table is named {name:?}, so no run could have written the row"
        )]
        Rule {
            /// The name the row gave.
            name: String,
        },
        /// The whole was refused as a completed report.
        #[error("the whole was refused as a completed report: {source}")]
        Completed {
            /// Why.
            #[from]
            source: crate::report::CompletionError,
        },
    }

    /// One mutation at `line` of `item` in `path`, where the rule named `rule` made `was` into `now`, decided as `outcome`, with no route, and established by this run.
    #[must_use]
    pub fn row(
        index: u32,
        (path, item, line): (&str, &str, u32),
        (rule, was, now): (&str, &str, &str),
        outcome: Decided,
    ) -> MutantRecord {
        let id = format!("{index:08x}{}", "a".repeat(56));
        MutantRecord {
            catalog_index: CatalogIndex::new(index),
            display_id: id.chars().take(20).collect(),
            id,
            path: path.to_owned(),
            position: Position {
                line,
                column: 5,
                character_column: 5,
            },
            rule: rule.to_owned(),
            item: item.to_owned(),
            original: was.to_owned(),
            replacement: now.to_owned(),
            outcome,
            accepted: false,
            blind_in: Vec::new(),
            routing: None,
            reuse: Reuse(Established::Here),
        }
    }

    /// A route this run decided and asked by: reaching `reaching`, removing `removed` by never-infected, and asking `answered` in order.
    #[must_use]
    pub fn routed(reaching: &[&str], removed: &[&str], answered: &[(&str, Outcome)]) -> Routing {
        Routing {
            granularity: if reaching.is_empty() {
                rust_mutants::session::Granularity::Discharged
            } else {
                rust_mutants::session::Granularity::Block
            },
            reaching: reaching.iter().map(|one| (*one).to_owned()).collect(),
            discharged: removed
                .iter()
                .map(|one| Discharged {
                    target: (*one).to_owned(),
                    proof: rust_mutants::session::NEVER_INFECTED,
                })
                .collect(),
            fallback: None,
            answered: answered
                .iter()
                .map(|(target, outcome)| Answered {
                    target: (*target).to_owned(),
                    outcome: *outcome,
                })
                .collect(),
        }
    }

    /// One build's report holding `rows`, with the counts the model counts from them and the findings and verdict they require.
    fn measured(
        run: &str,
        kind: RunKind,
        rows: Vec<MutantRecord>,
        builds: &[String],
    ) -> Result<BuildReport, UnmadeReport> {
        let mut report = BuildReport::new(run, kind, crate::config::Contract::StandardV1);
        "2026-09-24T00:00:00Z".clone_into(&mut report.timing.started);
        "2026-09-24T00:00:00Z".clone_into(&mut report.timing.finished);
        report.timing.duration_ms = 1;
        report.scope.configured_builds = builds.to_vec();
        report.limitations.push(Limitation::new(
            "git-metadata-unavailable",
            "a report assembled from rows has no repository process",
        ));
        report.targets.push(TargetRecord {
            id: "target".to_owned(),
            name: "pkg/lib/pkg".to_owned(),
            package: "pkg".to_owned(),
            status: TargetStatus::Passed,
            duration_ms: 1,
            message: None,
        });
        report.count_targets()?;
        report.accounting.mutants = crate::report::count_mutants(&rows)?;
        report.findings = rows
            .iter()
            .filter_map(|row| {
                row.outcome
                    .outcome()
                    .required_finding(row.accepted)
                    .map(|kind| Finding::new(kind, &row.display_id, "a finding its row requires"))
            })
            .collect();
        report.mutants = rows;
        super::read_every_named_file(&mut report);
        report.verdict = report.concluded();
        Ok(report)
    }

    /// The complete report of run `run`, of `kind`, that measured one build per entry of `builds`, each holding its rows.
    ///
    /// # Errors
    /// [`UnmadeReport`] when the rows do not make a report the model accepts, or name a rule the engine does not have, which is the fixture's mistake to fix.
    pub fn completed(
        run: &str,
        kind: RunKind,
        builds: Vec<(&str, Vec<MutantRecord>)>,
    ) -> Result<Report, UnmadeReport> {
        completed_with_drift(
            run,
            kind,
            builds
                .into_iter()
                .map(|(name, rows)| (name, rows, Vec::new()))
                .collect(),
        )
    }

    /// The report [`completed`] makes, with each build also recording what its controls established about each target's baseline reach.
    ///
    /// # Errors
    /// [`UnmadeReport`] as [`completed`] refuses.
    pub fn completed_with_drift(
        run: &str,
        kind: RunKind,
        builds: Vec<(&str, Vec<MutantRecord>, Vec<crate::report::drift::Drift>)>,
    ) -> Result<Report, UnmadeReport> {
        let rules = rust_mutants::rule::Registry::canonical();
        if let Some(unknown) = builds
            .iter()
            .flat_map(|(_, rows, _)| rows)
            .find(|row| rules.lookup(&row.rule).is_none())
        {
            return Err(UnmadeReport::Rule {
                name: unknown.rule.clone(),
            });
        }
        let order: Vec<String> = builds
            .iter()
            .map(|(name, _, _)| (*name).to_owned())
            .collect();
        let mut measured_builds = Vec::with_capacity(builds.len());
        for (at, (name, rows, drift)) in builds.into_iter().enumerate() {
            let mut report = measured(&format!("{run}-{at}"), kind, rows, &order)?;
            report.drift = drift;
            report.verdict = report.concluded();
            measured_builds.push((
                name.to_owned(),
                rust_mutants::cargo::BuildConfig::default().selection(),
                report,
            ));
        }
        let measurements = crate::report::across::BuildMeasurements::checked(measured_builds)?;
        let final_run = rust_mutants::id::RunId::try_from(run)?;
        match crate::report::across::configured(&final_run, &measurements)? {
            crate::report::LatticedDocument::Complete(whole) => {
                Ok(whole.complete_without_models()?)
            }
            crate::report::LatticedDocument::Shard(_) => Err(UnmadeReport::Part),
        }
    }
}

/// Records, for every file the rows and findings of `report` name, a digest as a run that read the file would have, which a report assembled by hand needs before the audit accepts it.
///
/// The digest is the SHA-256 of the path, not of any file's bytes, so a test that reads a real file records that file's own digest instead.
#[cfg(feature = "testkit")]
pub fn read_every_named_file(report: &mut crate::report::BuildReport) {
    let named: Vec<String> = report
        .mutants
        .iter()
        .map(|row| row.path.clone())
        .chain(
            report
                .findings
                .iter()
                .filter_map(|finding| finding.path.clone()),
        )
        .collect();
    for path in named {
        let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
        sha2::Digest::update(&mut hasher, path.as_bytes());
        report
            .sources
            .entry(path)
            .or_insert_with(|| rust_mutants::id::HexDigest::finish(hasher));
    }
}
