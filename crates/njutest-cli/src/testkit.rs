// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test support this crate's own suite and a sibling's may reach for.
//!
//! Nothing in production imports this: `cargo xtask devgates` refuses it.

#![expect(
    clippy::unreachable,
    reason = "three of these samples are made by asking the library to refuse something it always refuses; a sample that came back as a success would mean the library stopped refusing it, which is a failure to report loudly rather than to carry"
)]

use std::path::Path;

use crate::error::RunnerError;

/// Drives the watch loop with a caller-controlled wait.
///
/// Production always waits with [`std::thread::sleep`]. Tests supply an
/// observable wait instead, so they prove exactly where the loop waits without
/// depending on how promptly a hosted operating system schedules the thread.
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
    crate::app::watch::until_with_wait(cancel, look, round, waiting)
}

/// One failure of every shape this runner reports.
///
/// The list is held to the enum by the `match` below, which names every
/// variant and has no catch-all: a failure added without a sample here does
/// not compile. That is the point. What this is for — checking that every
/// code a run can print is one `docs/errors.md` explains — is only as good as
/// the list being every one, and a list somebody maintains by hand is a list
/// that is one behind.
#[must_use]
pub fn every_failure() -> Vec<RunnerError> {
    let nowhere = Path::new("nowhere");
    let failures = vec![
        RunnerError::Interrupted,
        RunnerError::Config(
            crate::config::Config::parse("version = 9\n", nowhere)
                .err()
                .unwrap_or_else(|| unreachable!("nine is not a version this release knows")),
        ),
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
            rust_mutants::coverage::parse_export(b"not an export")
                .err()
                .unwrap_or_else(|| unreachable!("that is not an export"))
                .into(),
        ),
        RunnerError::Provider(crate::provider::ProviderError::new(
            crate::provider::ProviderErrorKind::Unstartable,
            "no such command",
        )),
        RunnerError::MiriMissing {
            message: "the toolchain has no miri".to_owned(),
        },
        RunnerError::Resource(crate::resource::ResourceError::EnvironmentRefused {
            capability: "postgres".to_owned(),
            name: "RUSTFLAGS".to_owned(),
        }),
        RunnerError::Report(
            crate::report::json::parse("{}")
                .err()
                .unwrap_or_else(|| unreachable!("an empty object is not a report")),
        ),
        RunnerError::Scratch(crate::scratch::ScratchError::Unusable {
            path: nowhere.to_path_buf(),
            source: std::io::Error::other("no"),
        }),
        RunnerError::Build(crate::build::BuildError::NotRun {
            message: "cargo would not start".to_owned(),
        }),
        RunnerError::Engine(rust_mutants::EngineError::Interrupted),
    ];
    for one in &failures {
        match one {
            RunnerError::Interrupted
            | RunnerError::Config(_)
            | RunnerError::Target(_)
            | RunnerError::Evidence(_)
            | RunnerError::Cache(_)
            | RunnerError::Coverage(_)
            | RunnerError::Provider(_)
            | RunnerError::MiriMissing { .. }
            | RunnerError::Resource(_)
            | RunnerError::Report(_)
            | RunnerError::Scratch(_)
            | RunnerError::Build(_)
            | RunnerError::Engine(_) => {}
        }
    }
    failures
}

/// One refusal of every shape the evidence layer can record.
///
/// Held to the enum by the `match` below, which names every variant and has
/// no catch-all. What this is for — checking that every word a route can
/// carry is one [`docs/trace-v1.md`](../../../docs/trace-v1.md) lists — is
/// only as good as the list being every one.
#[must_use]
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

/// One event of every shape a recording can hold.
///
/// Held to the enum the same way. A recording is what an audit re-derives a
/// run's proofs from, and a shape the page does not list is one a reader
/// meets with nothing to look it up by.
#[must_use]
pub fn every_payload() -> Vec<crate::trace::Payload> {
    use crate::trace::{
        ArtifactRecord, ExecRecord, MutantExecRecord, NoteRecord, Payload, PhaseRecord,
        ProbeExecRecord, ProgressRecord, RouteRecord, RunRecord, StartRecord,
    };
    let phase = PhaseRecord {
        name: "baseline".to_owned(),
        duration_ms: Some(1),
    };
    let payloads = vec![
        Payload::RunStart {
            start: StartRecord::of(
                "20270115T080000Z-aaaaaa",
                crate::report::RunKind::Full,
                crate::config::Contract::StandardV1,
            ),
        },
        Payload::PhaseStart {
            phase: phase.clone(),
        },
        Payload::PhaseEnd { phase },
        Payload::Exec {
            exec: ExecRecord::default(),
        },
        Payload::Progress {
            progress: ProgressRecord {
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
            route: RouteRecord::default(),
        },
        Payload::MutantExec {
            mutant: MutantExecRecord::default(),
        },
        Payload::ProbeExec {
            probe: ProbeExecRecord::default(),
        },
        Payload::Note {
            note: NoteRecord {
                kind: "a".to_owned(),
                detail: "one".to_owned(),
            },
        },
        Payload::RunEnd {
            run: RunRecord {
                verdict: "ASSURED".to_owned(),
                accounting: None,
                error: None,
                events_emitted: 10,
                events_dropped: 0,
            },
        },
    ];
    for one in &payloads {
        match one {
            Payload::RunStart { .. }
            | Payload::PhaseStart { .. }
            | Payload::PhaseEnd { .. }
            | Payload::Exec { .. }
            | Payload::Progress { .. }
            | Payload::Artifact { .. }
            | Payload::Route { .. }
            | Payload::MutantExec { .. }
            | Payload::ProbeExec { .. }
            | Payload::Note { .. }
            | Payload::RunEnd { .. } => {}
        }
    }
    payloads
}
