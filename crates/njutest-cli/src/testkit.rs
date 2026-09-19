// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test support this crate's own suite and a sibling's may reach for.

#![expect(
    clippy::unreachable,
    reason = "three of these samples are made by asking the library to refuse something it always refuses; a sample that came back as a success would mean the library stopped refusing it, which is a failure to report loudly rather than to carry"
)]

use std::path::Path;

use crate::error::RunnerError;

/// Drives the watch loop with a caller-controlled wait.
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

/// A routing that decided nothing, for a sample of every payload to carry.
///
/// Written out rather than defaulted, because there is no granularity a route
/// is decided at when nobody decided it, and `RouteRecord` has no `Default`
/// for that reason.
const fn nothing_routed() -> crate::trace::RouteRecord {
    crate::trace::RouteRecord {
        mutant: String::new(),
        granularity: rust_mutants::session::Granularity::All,
        fallback: None,
        reaching: Vec::new(),
        tests: Vec::new(),
        discharged: Vec::new(),
        considered: Vec::new(),
        reused: None,
        refused: None,
    }
}

/// One event of every shape a recording can hold.
#[must_use]
pub fn every_payload() -> Vec<crate::trace::Payload> {
    use crate::trace::{
        ArtifactRecord, ExecRecord, MutantExecRecord, NoteRecord, Payload, PhaseRecord,
        ProbeExecRecord, ProgressRecord, RunRecord, StartRecord, WireExchangeRecord,
        WireExecRecord,
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
            exec: ExecRecord {
                argv: Vec::new(),
                dir: None,
                env_names: Vec::new(),
                timeout_ms: None,
                stopped: rust_mutants::execute::Stopped::Ran { code: 0 },
                duration_ms: 0,
                output_bytes: 0,
                output_sha256: None,
                output_truncated: false,
                output_path: None,
                error: None,
                output: Vec::new(),
            },
        },
        Payload::Progress {
            progress: ProgressRecord {
                subject: String::new(),
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
            route: nothing_routed(),
        },
        Payload::MutantExec {
            mutant: MutantExecRecord::default(),
        },
        Payload::ProbeExec {
            probe: ProbeExecRecord::default(),
        },
        Payload::WireExchange {
            exchange: WireExchangeRecord::default(),
        },
        Payload::WireExec {
            wire: WireExecRecord {
                fault: String::new(),
                capability: String::new(),
                seq: 0,
                rule: crate::wire::rule::Rule::DropConnection,
                decision: crate::report::SeamDecision::Unnoticed,
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
                verdict: "ASSURED".to_owned(),
                accounting: None,
                error: None,
                events_emitted: 10,
                events_dropped: 0,
            },
        },
    ];
    exhaustive(&payloads);
    payloads
}

/// Refuses to compile where a shape is added and this list is not, which is what makes it a ledger.
fn exhaustive(payloads: &[crate::trace::Payload]) {
    use crate::trace::Payload;
    for one in payloads {
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
            | Payload::WireExchange { .. }
            | Payload::WireExec { .. }
            | Payload::Note { .. }
            | Payload::RunEnd { .. } => {}
        }
    }
}
