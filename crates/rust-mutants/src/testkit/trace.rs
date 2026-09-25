// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Recording a run the same way twice, so a golden can freeze what it says.

#![expect(
    clippy::panic,
    reason = "the frozen origin is a constant of this file: a moment it cannot parse is a \
              broken testkit, not a run-time condition a caller could handle"
)]

use jiff::Timestamp;

use crate::trace::{Clock, Event, MemorySink, Payload, Recorder, Sink, TraceContext};

/// The moment a frozen recording starts at: a round number far from any real one, so a golden that leaks a wall clock is obvious.
pub const ORIGIN: i64 = 1_800_000_000;

/// A clock that advances one second per reading from [`ORIGIN`], so a recording is the same bytes every time it is made.
///
/// # Panics
/// Never: [`ORIGIN`] is a moment.
#[must_use]
pub fn stepping_clock() -> Clock {
    let origin = Timestamp::from_second(ORIGIN)
        .unwrap_or_else(|error| panic!("the frozen origin is a moment: {error}"));
    Clock::stepping(origin, std::time::Duration::from_secs(1))
}

/// A recorder over a memory sink on the stepping clock, which is what a test reads back with [`Recorder::events`].
#[must_use]
pub fn memory_recorder() -> Recorder {
    Recorder::new(
        Sink::Memory(MemorySink::unbounded()),
        stepping_clock(),
        standalone_context(),
    )
}

/// A stable standalone trace binding for tests that exercise the engine below its CLI boundary.
///
/// # Panics
/// Panics only if the fixed test run identity ceases to be canonical, which is a defect in this testkit rather than a condition a test can recover from.
#[must_use]
pub fn standalone_context() -> TraceContext {
    let run_id = crate::id::RunId::try_from("test")
        .unwrap_or_else(|error| panic!("the fixed test run id is canonical: {error}"));
    let build = crate::cargo::BuildConfig::default().selection();
    TraceContext::Standalone {
        run_id,
        build_selection: build.digest().clone(),
    }
}

/// The type name of every event, in order: what a test asserts when it is about which decisions were recorded rather than about what each one said.
#[must_use]
pub fn type_names(events: &[Event]) -> Vec<&'static str> {
    events
        .iter()
        .map(|event| event.payload.type_name())
        .collect()
}

/// The wrapper whose object is the record documented for `payload`, or none where the record's fields sit directly beside the `type` tag.
///
/// An exhaustive match makes a new payload variant add its serialization shape here before a documentation ledger can compile.
#[must_use]
pub const fn record_key(payload: &Payload) -> Option<&'static str> {
    match payload {
        Payload::RunStart { .. } => None,
        Payload::PhaseStart { .. } | Payload::PhaseEnd { .. } => Some("phase"),
        Payload::Open { .. } => Some("open"),
        Payload::Snapshot { .. } => Some("snapshot"),
        Payload::Exec { .. } => Some("exec"),
        Payload::DiscoverFile { .. } => Some("discover"),
        Payload::Instrument { .. } => Some("instrument"),
        Payload::ValidateRound { .. } => Some("round"),
        Payload::Bisect { .. } => Some("bisect"),
        Payload::Build { .. } => Some("build"),
        Payload::Verify { .. } => Some("verify"),
        Payload::Touch { .. } => Some("touch"),
        Payload::PerturbedControl { .. } => Some("perturbed"),
        Payload::Witness { .. } => Some("witness"),
        Payload::SkipClaim { .. } => Some("claim"),
        Payload::Kept { .. } => Some("kept"),
        Payload::Route { .. } => Some("route"),
        Payload::Cache { .. } => Some("cache"),
        Payload::Select { .. } => Some("select"),
        Payload::Identical { .. } => Some("identical"),
        Payload::Evidence { .. } => Some("evidence"),
        Payload::MutantExec { .. } => Some("mutant"),
        Payload::Note { .. } => Some("note"),
        Payload::RunEnd { .. } => Some("run"),
    }
}

/// Closed specimens whose union serializes every top-level field of every trace record.
///
/// Optional collections and values are non-empty.
/// `exec` has one specimen per [`crate::execute::Stopped`] variant, so a new way a process can stop makes this testkit fail to compile until its wire shape is represented.
///
/// # Panics
/// Panics only if a hard-coded specimen violates the testkit's own nonempty record-name invariant.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "one visible constructor per closed payload is the compile-time ledger"
)]
pub fn every_payload() -> Vec<Payload> {
    use crate::trace::{
        AttributionRecord, BisectRecord, BuildRecord, CacheRecord, DischargeRecord,
        DiscoverFileRecord, EvidenceRecord, ExecRecord, IdenticalRecord, InstrumentRecord,
        KeptRecord, MutantExecRecord, NoteRecord, OpenRecord, PhaseRecord, RouteRecord, RunRecord,
        SelectRecord, SiteRecord, SkipClaimRecord, SkipCount, SnapshotRecord, SweepRecord,
        TargetRecord, TouchRecord, ValidateRoundRecord, VerifyRecord, WitnessRecord,
    };

    let phase = || PhaseRecord {
        name: "prepare".to_owned(),
        duration_ms: Some(7),
    };
    let mut payloads = vec![
        Payload::RunStart {
            schema: crate::trace::SCHEMA.to_owned(),
            engine: crate::VERSION.to_owned(),
            context: standalone_context(),
        },
        Payload::PhaseStart { phase: phase() },
        Payload::PhaseEnd { phase: phase() },
        Payload::Open {
            open: OpenRecord {
                root: "/workspace".to_owned(),
                snapshot_dir: "/tmp/snapshot".to_owned(),
                stable_dir: true,
                sweep: Some(SweepRecord {
                    parent: "/tmp".to_owned(),
                    removed: 1,
                    removed_bytes: 2,
                    live: 3,
                    kept: 4,
                    failures: 5,
                }),
            },
        },
        Payload::Snapshot {
            snapshot: SnapshotRecord {
                source_root: "/workspace".to_owned(),
                dir: "/tmp/snapshot".to_owned(),
                files: 2,
                bytes: 3,
                workspace_digest: Some("digest".to_owned()),
                duration_ms: 4,
                error: Some("refusal".to_owned()),
            },
        },
        Payload::DiscoverFile {
            discover: DiscoverFileRecord {
                path: "src/lib.rs".to_owned(),
                candidates: 2,
                sites: vec![SiteRecord {
                    line: 3,
                    column: 4,
                    rule: "gt-to-ge".to_owned(),
                    form: Some("E".to_owned()),
                    skip: Some("test-code".to_owned()),
                    note: Some("declined".to_owned()),
                }],
                skips: vec![SkipCount {
                    reason: "test-code".to_owned(),
                    count: 1,
                }],
            },
        },
        Payload::Instrument {
            instrument: InstrumentRecord {
                path: "src/lib.rs".to_owned(),
                guards: 2,
                module: "__rust_mutants_runtime".to_owned(),
                lines_before: 10,
                lines_after: 10,
            },
        },
        Payload::ValidateRound {
            round: ValidateRoundRecord {
                round: 1,
                condemned: 2,
                success: false,
                written: 3,
                attributed: vec![AttributionRecord {
                    index: 4,
                    code: Some("E0308".to_owned()),
                    said: "mismatched types".to_owned(),
                }],
                unattributed: vec!["build failed".to_owned()],
            },
        },
        Payload::Bisect {
            bisect: BisectRecord {
                suspects: 8,
                offenders: vec![3],
                attempts: 4,
                diagnosed: 1,
            },
        },
        Payload::Build {
            build: BuildRecord {
                targets: vec!["demo/lib/demo".to_owned()],
                details: vec![TargetRecord {
                    id: "demo/lib/demo".to_owned(),
                    kind: "lib".to_owned(),
                    harness: true,
                    limitations: vec!["custom-harness".to_owned()],
                }],
            },
        },
        Payload::Verify {
            verify: VerifyRecord {
                target: "demo/lib/demo".to_owned(),
                outcome: "passed".to_owned(),
                tests_run: Some(5),
                duration_ms: 6,
                remembered: true,
                retried: true,
            },
        },
        Payload::PerturbedControl {
            perturbed: crate::trace::PerturbedRecord {
                target: "demo/lib/demo".to_owned(),
                perturbation: crate::trace::PerturbationRecord {
                    environment: vec![crate::trace::SetRecord {
                        name: "TMPDIR".to_owned(),
                        value: None,
                    }],
                    launcher: Some("umask 077".to_owned()),
                    arguments: vec!["--test-threads=1".to_owned()],
                },
                outcome: "survived".to_owned(),
                failed_tests: Vec::new(),
                duration_ms: 3,
                reach: crate::trace::ReachRecord::Recorded {
                    touch: TouchRecord {
                        target: "demo/lib/demo".to_owned(),
                        measured: crate::trace::Measurement::Control,
                        mutant: None,
                        passed: vec!["tests::adds".to_owned()],
                        summary: crate::trace::SummaryRecord::Libtest { tests_run: Some(1) },
                        reached_sites: vec![0],
                        entered_bodies: Vec::new(),
                        infected_sites: Vec::new(),
                        tests: 1,
                        sites: 1,
                        loose: 0,
                        infected: 0,
                        entered: 0,
                        entered_items: Vec::new(),
                    },
                },
            },
        },
        Payload::Touch {
            touch: TouchRecord {
                target: "demo/lib/demo".to_owned(),
                measured: crate::trace::Measurement::Control,
                mutant: None,
                passed: vec!["tests::adds".to_owned(), "tests::subtracts".to_owned()],
                summary: crate::trace::SummaryRecord::Libtest { tests_run: Some(2) },
                reached_sites: vec![0, 1, 2],
                entered_bodies: vec![3],
                infected_sites: vec![1],
                tests: 2,
                sites: 3,
                loose: 1,
                infected: 1,
                entered: 2,
                entered_items: vec![0, 1],
            },
        },
        Payload::Witness {
            witness: WitnessRecord {
                index: 1,
                witnesses: vec!["PartialEq".to_owned()],
                checked: false,
                diagnostic: Some("trait bound refused".to_owned()),
            },
        },
        Payload::SkipClaim {
            claim: SkipClaimRecord {
                path: "src/lib.rs".to_owned(),
                line: 9,
                reason: "generated".to_owned(),
                matched: true,
            },
        },
        Payload::Kept {
            kept: KeptRecord {
                path: "/tmp/snapshot".to_owned(),
                run_id: "run".to_owned(),
            },
        },
        Payload::Route {
            route: RouteRecord {
                mutant: "abcdef".to_owned(),
                index: 1,
                granularity: crate::session::Granularity::Block,
                fallback: Some(crate::session::Fallback::TouchIncomplete),
                reaching: vec!["demo/lib/demo".to_owned()],
                considered: vec!["demo/test/other".to_owned()],
                discharged: vec![DischargeRecord {
                    target: "demo/test/proved".to_owned(),
                    proof: "never-infected".to_owned(),
                }],
                executed: vec!["demo/lib/demo".to_owned()],
                reused: Some("earlier-run".to_owned()),
            },
        },
        Payload::Cache {
            cache: CacheRecord {
                mutant: "abcdef".to_owned(),
                key: "key".to_owned(),
                hit: true,
                source_run_id: Some("earlier-run".to_owned()),
            },
        },
        Payload::Select {
            select: SelectRecord {
                mutant: "abcdef".to_owned(),
                reason: "unreached".to_owned(),
            },
        },
        Payload::Identical {
            identical: IdenticalRecord {
                index: 1,
                identity: "not-established".to_owned(),
                detail: Some("compiler failed".to_owned()),
            },
        },
        Payload::Evidence {
            evidence: EvidenceRecord {
                file: "touched-v1.json".to_owned(),
                bytes: 12,
                digest: "digest".to_owned(),
            },
        },
        Payload::MutantExec {
            mutant: MutantExecRecord {
                entered_records: None,
                id: "abcdef".to_owned(),
                index: 1,
                target: "demo/lib/demo".to_owned(),
                outcome: "killed".to_owned(),
                step_notice: Some(crate::execute::StepLimitNotice::specimen()),
                exit_code: 101,
                duration_ms: 5,
                tests_run: Some(3),
                signal: Some(9),
                failed_tests: vec!["tests::caught".to_owned()],
                timeout_ms: 30_000,
                timeout_source: "configured".to_owned(),
                alone: true,
                lingered: true,
            },
        },
        Payload::Note {
            note: NoteRecord {
                kind: "progress".to_owned(),
                detail: "one thing happened".to_owned(),
            },
        },
        Payload::RunEnd {
            run: RunRecord {
                outcome: "failed".to_owned(),
                error: Some("one failure".to_owned()),
                events_emitted: 22,
                events_dropped: 1,
            },
        },
    ];
    payloads.extend(every_stopped().into_iter().map(|stopped| Payload::Exec {
        exec: ExecRecord {
            argv: vec!["cargo".to_owned(), "test".to_owned()],
            dir: Some("/workspace".to_owned()),
            env_names: vec!["RUSTFLAGS".to_owned()],
            timeout_ms: Some(300_000),
            quiet_ms: Some(30_000),
            stopped,
            duration_ms: 5,
            output_bytes: 6,
            output_sha256: Some("digest".to_owned()),
            output_truncated: true,
            output_path: Some("output/1.txt".to_owned()),
            error: Some("one error".to_owned()),
            output: b"capture".to_vec(),
        },
    }));
    for payload in &payloads {
        if let Some(record) = record_key(payload) {
            assert!(!record.is_empty(), "record wrapper names are nonempty");
        }
    }
    payloads
}

/// Every way a process can stop, named so extending the enum extends the specimen ledger at compile time.
#[must_use]
pub fn every_stopped() -> [crate::execute::Stopped; 11] {
    use crate::execute::Stopped;

    let exits = every_process_exit();
    let protocol = every_step_protocol_failure();
    let stopped = [
        Stopped::NotStarted,
        Stopped::Exited { exit: exits[0] },
        Stopped::Exited { exit: exits[1] },
        Stopped::Exited { exit: exits[2] },
        Stopped::TimedOut { raised: Some(3) },
        Stopped::Stalled { raised: Some(2) },
        Stopped::Cancelled { started: true },
        Stopped::WaitFailed,
        Stopped::StepLimitReached {
            notice: crate::execute::StepLimitNotice::specimen(),
        },
        Stopped::StepProtocolFailed {
            reason: protocol[0].clone(),
        },
        Stopped::Answered,
    ];
    for one in &stopped {
        match one {
            Stopped::NotStarted
            | Stopped::Exited { .. }
            | Stopped::TimedOut { .. }
            | Stopped::Stalled { .. }
            | Stopped::Cancelled { .. }
            | Stopped::WaitFailed
            | Stopped::StepLimitReached { .. }
            | Stopped::StepProtocolFailed { .. }
            | Stopped::Answered => {}
        }
    }
    stopped
}

/// Every closed step-protocol failure shape.
#[must_use]
pub fn every_step_protocol_failure() -> [crate::execute::StepProtocolFailure; 18] {
    use crate::execute::StepProtocolFailure;

    let failures = [
        StepProtocolFailure::MonitorInvalid {
            path: "notice".to_owned(),
        },
        StepProtocolFailure::MonitorInspect {
            path: "notice".to_owned(),
            detail: "refused".to_owned(),
        },
        StepProtocolFailure::Publication {},
        StepProtocolFailure::NoticeMissing {},
        StepProtocolFailure::NoticeNotRegular {
            path: "notice".to_owned(),
        },
        StepProtocolFailure::NoticeMetadata {
            path: "notice".to_owned(),
            detail: "refused".to_owned(),
        },
        StepProtocolFailure::NoticeOpen {
            path: "notice".to_owned(),
            detail: "refused".to_owned(),
        },
        StepProtocolFailure::NoticeRead {
            path: "notice".to_owned(),
            detail: "refused".to_owned(),
        },
        StepProtocolFailure::NoticeTooLarge {
            path: "notice".to_owned(),
            limit: 16 * 1024,
        },
        StepProtocolFailure::NoticeNotUtf8 {
            path: "notice".to_owned(),
        },
        StepProtocolFailure::NoticeEmpty {},
        StepProtocolFailure::NoticeNonCanonical {},
        StepProtocolFailure::NoticeExtraRecord {},
        StepProtocolFailure::ExecutionMismatch {},
        StepProtocolFailure::InvalidLimit {},
        StepProtocolFailure::InvalidObserved {},
        StepProtocolFailure::BoundaryMismatch {},
        StepProtocolFailure::Cleanup {
            path: "notice".to_owned(),
            detail: "refused".to_owned(),
        },
    ];
    for failure in &failures {
        match failure {
            StepProtocolFailure::MonitorInvalid { .. }
            | StepProtocolFailure::MonitorInspect { .. }
            | StepProtocolFailure::Publication {}
            | StepProtocolFailure::NoticeMissing {}
            | StepProtocolFailure::NoticeNotRegular { .. }
            | StepProtocolFailure::NoticeMetadata { .. }
            | StepProtocolFailure::NoticeOpen { .. }
            | StepProtocolFailure::NoticeRead { .. }
            | StepProtocolFailure::NoticeTooLarge { .. }
            | StepProtocolFailure::NoticeNotUtf8 { .. }
            | StepProtocolFailure::NoticeEmpty {}
            | StepProtocolFailure::NoticeNonCanonical {}
            | StepProtocolFailure::NoticeExtraRecord {}
            | StepProtocolFailure::ExecutionMismatch {}
            | StepProtocolFailure::InvalidLimit {}
            | StepProtocolFailure::InvalidObserved {}
            | StepProtocolFailure::BoundaryMismatch {}
            | StepProtocolFailure::Cleanup { .. } => {}
        }
    }
    failures
}

/// Every process-owned exit shape, named so extending the nested enum extends the specimen ledger at compile time as well.
fn every_process_exit() -> [crate::runner::ProcessExit; 3] {
    use crate::runner::ProcessExit;

    let exits = [
        ProcessExit::Code(0),
        ProcessExit::Signal(9),
        ProcessExit::Unknown,
    ];
    for exit in exits {
        match exit {
            ProcessExit::Code(_) | ProcessExit::Signal(_) | ProcessExit::Unknown => {}
        }
    }
    exits
}
