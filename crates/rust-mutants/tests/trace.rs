// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The engine trace: diagnostic exhaust under the rules of ADR 0002. Never a claim, never a failure, honest about drops.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::as_conversions,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::fs;
use std::io;

use rust_mutants::testkit::trace::{memory_recorder, stepping_clock, type_names};
use rust_mutants::trace::summary::{diff, render, summarize};
use rust_mutants::trace::{
    DirSink, EVERY_TYPE, ExecRecord, FILE_NAME, MemorySink, OUTPUT_DIRECTORY_NAME,
    OUTPUT_FILE_LIMIT, OpenRecord, Payload, ProbeExecRecord, Problem, Recorder, RouteRecord,
    SCHEMA, Sink, SnapshotRecord, SweepRecord, TRUNCATION_MARKER, VerifyRecord, WitnessRecord,
    check, read_events,
};
use std::collections::BTreeSet;

use sha2::{Digest as _, Sha256};

/// A directory sink that has been closed, so everything written to it fails: the reachable form of "the disk is gone".
fn broken(dir: &std::path::Path) -> Sink {
    let sink = DirSink::create(&dir.join("recording")).expect("the sink");
    sink.close().expect("closed");
    Sink::Dir(sink)
}

fn exec(argv: &[&str]) -> ExecRecord {
    ExecRecord {
        argv: argv.iter().map(|s| (*s).to_owned()).collect(),
        ..ExecRecord::default()
    }
}

#[test]
fn a_disabled_recorder_records_nothing_and_every_call_is_a_no_op() {
    let recorder = Recorder::disabled();
    assert!(!recorder.is_enabled());
    let phase = recorder.phase("open");
    recorder.exec(exec(&["cargo", "metadata"]));
    recorder.note("progress", "nothing to see");
    phase.end();
    recorder.run_end("ok", None);
    assert!(!Recorder::clone(&recorder).is_enabled());
}

#[test]
fn the_schema_is_frozen() {
    assert_eq!(SCHEMA, "rust-mutants-trace-v1");
    assert_eq!(FILE_NAME, "trace.jsonl");
    assert_eq!(OUTPUT_DIRECTORY_NAME, "output");
    assert_eq!(OUTPUT_FILE_LIMIT, 1 << 20);
    assert_eq!(TRUNCATION_MARKER, "...");
}

#[test]
fn a_recording_starts_with_run_start_and_ends_with_run_end_carrying_the_accounting() {
    let recorder = memory_recorder();
    assert!(recorder.is_enabled());
    recorder.note("progress", "one");
    recorder.note("progress", "two");
    recorder.run_end("prepared", None);

    let events = recorder.events();
    assert_eq!(
        type_names(&events),
        ["run-start", "note", "note", "run-end"]
    );
    let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, [1, 2, 3, 4]);
    assert_eq!(events[0].timestamp, "2027-01-15T08:00:00Z");
    assert_eq!(events[3].timestamp, "2027-01-15T08:00:03Z");
    let elapsed: Vec<u64> = events.iter().map(|e| e.elapsed_ms).collect();
    assert_eq!(elapsed, [0, 1000, 2000, 3000]);
    match &events[0].payload {
        Payload::RunStart { schema, engine } => {
            assert_eq!(schema, SCHEMA);
            assert_eq!(engine, rust_mutants::VERSION);
        }
        other => panic!("{other:?}"),
    }
    match &events[3].payload {
        Payload::RunEnd { run } => {
            assert_eq!(run.outcome, "prepared");
            assert_eq!(run.error, None);
            assert_eq!(run.events_emitted, 3, "run-start and two notes");
            assert_eq!(run.events_dropped, 0);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn run_end_happens_once_and_nothing_is_recorded_afterwards() {
    let recorder = memory_recorder();
    recorder.run_end("errored", Some("boom".to_owned()));
    recorder.note("progress", "too late");
    recorder.run_end("ok", None);
    let events = recorder.events();
    assert_eq!(type_names(&events), ["run-start", "run-end"]);
    match &events[1].payload {
        Payload::RunEnd { run } => assert_eq!(run.error.as_deref(), Some("boom")),
        other => panic!("{other:?}"),
    }
    assert!(
        recorder.is_closed(),
        "run-end closes the sink so the stream is complete on disk"
    );
}

#[test]
fn a_phase_guard_ends_its_phase_once_with_its_duration_and_phases_nest() {
    let recorder = memory_recorder();
    let outer = recorder.phase("prepare");
    let inner = recorder.phase("discover");
    inner.end();
    {
        let _dropped = recorder.phase("validate");
    }
    drop(outer);
    recorder.run_end("ok", None);
    let events = recorder.events();
    assert_eq!(
        type_names(&events),
        [
            "run-start",
            "phase-start",
            "phase-start",
            "phase-end",
            "phase-start",
            "phase-end",
            "phase-end",
            "run-end",
        ]
    );
    let phases: Vec<(String, Option<u64>)> = events
        .iter()
        .filter_map(|e| match &e.payload {
            Payload::PhaseStart { phase } | Payload::PhaseEnd { phase } => {
                Some((phase.name.clone(), phase.duration_ms))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        phases,
        [
            ("prepare".to_owned(), None),
            ("discover".to_owned(), None),
            ("discover".to_owned(), Some(1000)),
            ("validate".to_owned(), None),
            ("validate".to_owned(), Some(1000)),
            ("prepare".to_owned(), Some(5000)),
        ]
    );
}

#[test]
fn events_are_sequenced_in_delivery_order_across_threads() {
    let recorder = Recorder::wall(Sink::Memory(MemorySink::unbounded()));
    std::thread::scope(|scope| {
        for thread in 0..8 {
            let recorder = recorder.clone();
            scope.spawn(move || {
                for i in 0..50 {
                    recorder.note("thread", &format!("{thread}/{i}"));
                }
            });
        }
    });
    recorder.run_end("ok", None);
    let events = recorder.events();
    assert_eq!(events.len(), 1 + 400 + 1);
    for (index, event) in events.iter().enumerate() {
        assert_eq!(
            event.seq,
            index as u64 + 1,
            "delivery order is sequence order"
        );
    }
}

#[test]
fn exec_keeps_environment_names_only_sorted_and_deduplicated_and_digests_the_output() {
    let recorder = memory_recorder();
    let record = ExecRecord {
        env_names: vec![
            "RUST_MUTANTS_ACTIVE=deadbeef".to_owned(),
            "TOKEN=hunter2".to_owned(),
            "A".to_owned(),
            "A=1".to_owned(),
        ],
        output: b"the captured output".to_vec(),
        dir: Some("/snap/tree".to_owned()),
        timeout_ms: Some(30_000),
        exit_code: 101,
        duration_ms: 12,
        ..exec(&["cargo", "test", "--", "--exact", "t::x"])
    };
    recorder.exec(record);
    recorder.run_end("ok", None);
    let events = recorder.events();
    let Payload::Exec { exec } = &events[1].payload else {
        panic!("{:?}", events[1]);
    };
    assert_eq!(exec.env_names, ["A", "RUST_MUTANTS_ACTIVE", "TOKEN"]);
    assert_eq!(exec.output_bytes, 19);
    assert_eq!(
        exec.output_sha256.as_deref(),
        Some(hex::encode(Sha256::digest(b"the captured output")).as_str())
    );
    let line = serde_json::to_string(&events[1]).expect("json");
    assert!(!line.contains("hunter2"), "{line}");
    assert!(!line.contains("deadbeef"), "{line}");
    assert!(!line.contains("captured output"), "{line}");
    assert!(line.contains("\"exit_code\":101"), "{line}");
}

#[test]
fn a_sink_that_cannot_write_is_counted_never_returned() {
    let temp = tempfile::tempdir().expect("tempdir");
    let recorder = Recorder::new(broken(temp.path()), stepping_clock());
    for i in 0..5 {
        recorder.note("n", &i.to_string());
    }
    recorder.run_end("ok", None);
    assert!(recorder.events().is_empty());
}

#[test]
fn one_sink_failing_costs_that_sink_the_event_and_not_the_others() {
    let temp = tempfile::tempdir().expect("tempdir");
    let recorder = Recorder::new(
        Sink::Tee(vec![
            broken(temp.path()),
            Sink::Memory(MemorySink::unbounded()),
        ]),
        stepping_clock(),
    );
    recorder.note("a", "1");
    recorder.run_end("ok", None);

    let events = recorder.events();
    assert_eq!(type_names(&events), ["run-start", "note", "run-end"]);
    match &events[2].payload {
        Payload::RunEnd { run } => assert_eq!(
            run.events_dropped, 0,
            "the recording lost nothing: one sink kept every event"
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_sink_that_counts_its_own_drops_is_the_authority() {
    let recorder = Recorder::new(Sink::Memory(MemorySink::bounded(4)), stepping_clock());
    for i in 0..5 {
        recorder.note("n", &i.to_string());
    }
    recorder.run_end("ok", None);
    let events = recorder.events();
    assert_eq!(events.len(), 4, "the ring keeps the newest four");
    assert_eq!(type_names(&events), ["note", "note", "note", "run-end"]);
    match &events[3].payload {
        Payload::RunEnd { run } => {
            assert_eq!(run.events_dropped, 2);
            assert_eq!(run.events_emitted, 4);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_recording_writes_one_json_line_per_event_and_the_reader_round_trips() {
    let temp = tempfile::tempdir().expect("tempdir");
    let stream = temp.path().join("recording");
    let recorder = Recorder::new(
        Sink::Dir(DirSink::create(&stream).expect("the sink")),
        stepping_clock(),
    );
    let phase = recorder.phase("open");
    recorder.open(OpenRecord {
        root: "/home/alice/project".to_owned(),
        snapshot_dir: "/tmp/rust-mutants-snap-9c2098df26004b24".to_owned(),
        stable_dir: true,
        sweep: Some(SweepRecord {
            parent: "/tmp".to_owned(),
            removed: 2,
            removed_bytes: 4096,
            live: 1,
            kept: 0,
            failures: 0,
        }),
    });
    recorder.snapshot(SnapshotRecord {
        source_root: "/home/alice/project".to_owned(),
        dir: "/tmp/rust-mutants-snap-9c2098df26004b24".to_owned(),
        files: 42,
        bytes: 123_456,
        workspace_digest: Some("ab".repeat(32)),
        duration_ms: 17,
        error: None,
    });
    phase.end();
    recorder.exec(ExecRecord {
        output: b"ok\n".to_vec(),
        ..exec(&["cargo", "metadata"])
    });
    recorder.run_end("ok", None);

    let text = fs::read_to_string(stream.join(FILE_NAME)).expect("the stream");
    assert_eq!(text.lines().count(), 7);
    assert!(text.ends_with('\n'));
    for line in text.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("each line is one object");
        assert!(
            value.get("seq").is_some() && value.get("type").is_some(),
            "{line}"
        );
    }
    let events = read_events(text.as_bytes()).expect("read");
    assert_eq!(
        type_names(&events),
        [
            "run-start",
            "phase-start",
            "open",
            "snapshot",
            "phase-end",
            "exec",
            "run-end"
        ]
    );
    assert!(check(&events).is_empty(), "{:?}", check(&events));
    let golden = mjutest_devkit::paths::workspace_root()
        .join("crates/rust-mutants/tests/testdata/trace/basic.golden");
    let stable = text.replace(rust_mutants::VERSION, "<version>");
    mjutest_devkit::golden::golden(&golden, stable.as_bytes()).expect("golden");
}

#[test]
fn dir_sink_claims_its_directory_exclusively_and_preserves_output_beside_the_stream() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().join("run-1");
    let sink = DirSink::create(&dir).expect("create");
    assert_eq!(sink.directory(), dir);
    let refused = DirSink::create(&dir).unwrap_err();
    assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);

    let recorder = Recorder::new(Sink::Dir(sink), stepping_clock());
    let big = vec![b'x'; OUTPUT_FILE_LIMIT + 10];
    recorder.exec(ExecRecord {
        output: big.clone(),
        ..exec(&["big"])
    });
    recorder.exec(ExecRecord {
        output: b"small\n".to_vec(),
        ..exec(&["small"])
    });
    recorder.exec(exec(&["silent"]));
    recorder.run_end("ok", None);

    let stream = fs::read_to_string(dir.join(FILE_NAME)).expect("stream");
    let events = read_events(stream.as_bytes()).expect("read");
    assert_eq!(
        type_names(&events),
        ["run-start", "exec", "exec", "exec", "run-end"]
    );
    let execs: Vec<&ExecRecord> = events
        .iter()
        .filter_map(|e| match &e.payload {
            Payload::Exec { exec } => Some(exec),
            _ => None,
        })
        .collect();
    assert_eq!(execs[0].output_path.as_deref(), Some("output/2.txt"));
    assert!(execs[0].output_truncated);
    assert_eq!(execs[0].output_bytes, big.len() as u64);
    assert_eq!(
        execs[0].output_sha256.as_deref(),
        Some(hex::encode(Sha256::digest(&big)).as_str()),
        "the digest is of the whole capture"
    );
    assert_eq!(execs[1].output_path.as_deref(), Some("output/3.txt"));
    assert!(!execs[1].output_truncated);
    assert_eq!(execs[2].output_path, None);
    assert!(
        execs.iter().all(|e| e.output.is_empty()),
        "raw output never rides in the stream"
    );

    let preserved = fs::read(dir.join(OUTPUT_DIRECTORY_NAME).join("2.txt")).expect("preserved");
    assert_eq!(preserved.len(), OUTPUT_FILE_LIMIT + TRUNCATION_MARKER.len());
    assert!(preserved.ends_with(TRUNCATION_MARKER.as_bytes()));
    assert_eq!(
        fs::read(dir.join(OUTPUT_DIRECTORY_NAME).join("3.txt")).expect("preserved"),
        b"small\n"
    );
}

#[test]
fn the_reader_refuses_a_malformed_line_and_names_it() {
    let text = "{\"seq\":1,\"type\":\"run-start\",\"schema\":\"rust-mutants-trace-v1\",\"engine\":\"0\",\"timestamp\":\"2027-01-15T08:00:00Z\",\"elapsed_ms\":0}\nnot json\n";
    let error = read_events(text.as_bytes()).unwrap_err();
    assert_eq!(error.line(), 2);
    assert!(error.to_string().contains("line 2"), "{error}");
}

#[test]
fn check_reports_sequence_gaps_a_missing_run_end_and_drops() {
    let recorder = memory_recorder();
    recorder.note("a", "1");
    recorder.note("b", "2");
    let mut events = recorder.events();
    assert_eq!(
        check(&events),
        [Problem::MissingRunEnd],
        "a killed run has no run-end"
    );
    events.remove(1);
    assert_eq!(
        check(&events),
        [
            Problem::SequenceGap {
                expected: 2,
                found: 3
            },
            Problem::MissingRunEnd
        ]
    );

    let recorder = Recorder::new(Sink::Memory(MemorySink::bounded(2)), stepping_clock());
    recorder.note("a", "1");
    recorder.note("b", "2");
    recorder.run_end("ok", None);
    let problems = check(&recorder.events());
    assert!(problems.contains(&Problem::MissingRunStart), "{problems:?}");
    assert!(problems.contains(&Problem::Dropped(1)), "{problems:?}");
}

/// The first half of the vocabulary: what a run says while it prepares.
fn one_of_each_preparation(recorder: &Recorder) {
    recorder.open(OpenRecord {
        root: "/w".to_owned(),
        snapshot_dir: "/tmp/snap".to_owned(),
        stable_dir: true,
        sweep: Some(SweepRecord {
            parent: "/tmp".to_owned(),
            removed: 1,
            removed_bytes: 2,
            live: 0,
            kept: 0,
            failures: 0,
        }),
    });
    recorder.snapshot(SnapshotRecord {
        source_root: "/w".to_owned(),
        dir: "/tmp/snap".to_owned(),
        files: 3,
        bytes: 40,
        workspace_digest: Some("a".repeat(64)),
        duration_ms: 5,
        error: None,
    });
    recorder.exec(exec(&["cargo", "check"]));
    recorder.discover_file(rust_mutants::trace::DiscoverFileRecord {
        path: "src/lib.rs".to_owned(),
        candidates: 2,
        sites: vec![rust_mutants::trace::SiteRecord {
            line: 3,
            column: 9,
            rule: "le-to-lt".to_owned(),
            form: Some("C".to_owned()),
            skip: None,
            note: None,
        }],
        skips: vec![rust_mutants::trace::SkipCount {
            reason: "macro-invocation".to_owned(),
            count: 1,
        }],
    });
    recorder.instrument(rust_mutants::trace::InstrumentRecord {
        path: "src/lib.rs".to_owned(),
        guards: 2,
        module: "__rm_0000".to_owned(),
        lines_before: 10,
        lines_after: 10,
    });
    recorder.validate_round(rust_mutants::trace::ValidateRoundRecord {
        round: 1,
        written: 2,
        condemned: 0,
        success: false,
        attributed: vec![rust_mutants::trace::AttributionRecord {
            index: 1,
            code: Some("E0308".to_owned()),
            said: "mismatched types".to_owned(),
        }],
        unattributed: vec!["error: something else".to_owned()],
    });
    recorder.bisect(rust_mutants::trace::BisectRecord {
        suspects: 4,
        offenders: vec![3],
        attempts: 5,
        diagnosed: 1,
    });
    recorder.build(rust_mutants::trace::BuildRecord {
        targets: vec!["demo/lib/demo".to_owned()],
        details: vec![rust_mutants::trace::TargetRecord {
            id: "demo/lib/demo".to_owned(),
            kind: "lib".to_owned(),
            harness: true,
            limitations: Vec::new(),
        }],
    });
}

/// The second half: what a run says while it measures.
fn one_of_each_measurement(recorder: &Recorder) {
    recorder.verify(VerifyRecord {
        target: "demo/lib/demo".to_owned(),
        outcome: "survived".to_owned(),
        tests_run: Some(3),
        duration_ms: 12,
    });
    recorder.probe_exec(ProbeExecRecord {
        target: "demo/lib/demo".to_owned(),
        outcome: "measured".to_owned(),
        infected: Some(2),
        duration_ms: 8,
    });
    recorder.witness(WitnessRecord {
        index: 1,
        witnesses: vec!["ord".to_owned()],
        checked: true,
        diagnostic: None,
    });
    recorder.skip_claim(rust_mutants::trace::SkipClaimRecord {
        path: "src/lib.rs".to_owned(),
        line: 12,
        reason: "the bound is the caller's".to_owned(),
        matched: true,
    });
    recorder.mutant_exec(rust_mutants::trace::MutantExecRecord {
        id: "b".repeat(64),
        index: 1,
        target: "demo/lib/demo".to_owned(),
        outcome: "killed".to_owned(),
        exit_code: 101,
        duration_ms: 40,
        tests_run: Some(3),
        signal: Some(6),
        failed_tests: vec!["demo::tests::le_bound".to_owned()],
        timeout_ms: 90_000,
        timeout_source: "derived".to_owned(),
        alone: true,
    });
    recorder.cache(rust_mutants::trace::CacheRecord {
        mutant: "b".repeat(20),
        key: "d".repeat(64),
        hit: true,
        source_run_id: Some("20260907T000000000Z".to_owned()),
    });
    recorder.select(rust_mutants::trace::SelectRecord {
        mutant: "b".repeat(20),
        reason: "unreached".to_owned(),
    });
    recorder.identical(rust_mutants::trace::IdenticalRecord {
        index: 1,
        identity: "identical".to_owned(),
        detail: None,
    });
    recorder.evidence(rust_mutants::trace::EvidenceRecord {
        file: "reached-v1.json".to_owned(),
        bytes: 4096,
        digest: "c".repeat(64),
    });
    recorder.route(RouteRecord {
        mutant: "b".repeat(20),
        index: 1,
        granularity: "block".to_owned(),
        fallback: None,
        reaching: vec!["demo/lib/demo".to_owned()],
        discharged: Vec::new(),
        executed: vec!["demo/lib/demo".to_owned()],
        reused: None,
    });
    recorder.note("coverage", "the tools are not installed");
}

#[test]
fn every_event_type_has_one_golden_line_and_validates_against_the_schema() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().join("recording");
    let sink = DirSink::create(&dir).expect("the sink");
    let recorder = Recorder::new(Sink::Dir(sink), stepping_clock());
    let phase = recorder.phase("prepare");
    one_of_each_preparation(&recorder);
    one_of_each_measurement(&recorder);
    phase.end();
    recorder.run_end("detected", None);

    let text = fs::read_to_string(dir.join(FILE_NAME)).expect("the stream");
    let events = read_events(text.as_bytes()).expect("read");
    let seen: BTreeSet<&str> = type_names(&events).into_iter().collect();
    let known: BTreeSet<&str> = EVERY_TYPE.iter().copied().collect();
    assert_eq!(
        seen, known,
        "the golden holds one line of every type the vocabulary knows, and no other"
    );

    let schema: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            mjutest_devkit::paths::workspace_root().join("schema/rust-mutants-trace-v1.json"),
        )
        .expect("the schema file"),
    )
    .expect("the schema parses");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    for line in text.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("a line is one object");
        if let Err(error) = validator.validate(&value) {
            panic!("{line}\n{error}");
        }
    }

    let golden = mjutest_devkit::paths::workspace_root()
        .join("crates/rust-mutants/tests/testdata/trace/events.golden");
    let stable = text.replace(rust_mutants::VERSION, "<version>");
    mjutest_devkit::golden::golden(&golden, stable.as_bytes()).expect("golden");
}

#[test]
fn the_schema_and_the_vocabulary_name_the_same_types() {
    let schema: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            mjutest_devkit::paths::workspace_root().join("schema/rust-mutants-trace-v1.json"),
        )
        .expect("the schema file"),
    )
    .expect("the schema parses");
    let named: BTreeSet<String> = schema["oneOf"]
        .as_array()
        .expect("one branch per type")
        .iter()
        .filter_map(|branch| branch["properties"]["type"]["const"].as_str())
        .map(str::to_owned)
        .collect();
    let known: BTreeSet<String> = EVERY_TYPE.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(named, known);
}

#[test]
fn a_phase_that_never_ended_is_a_problem_a_reader_is_told_about() {
    let events = vec![
        event(
            1,
            Payload::RunStart {
                schema: SCHEMA.to_owned(),
                engine: "0.1.0".to_owned(),
            },
        ),
        event(
            2,
            Payload::PhaseStart {
                phase: rust_mutants::trace::PhaseRecord {
                    name: "prepare".to_owned(),
                    duration_ms: None,
                },
            },
        ),
        event(
            3,
            Payload::RunEnd {
                run: rust_mutants::trace::RunRecord {
                    outcome: "failed".to_owned(),
                    error: Some("killed".to_owned()),
                    events_emitted: 3,
                    events_dropped: 0,
                },
            },
        ),
    ];
    let problems = check(&events);
    assert!(
        problems.iter().any(|problem| matches!(
            problem,
            Problem::UnbalancedPhase { name } if name == "prepare"
        )),
        "a run killed inside a phase leaves it open, and a reader is told: {problems:?}"
    );
}

#[test]
fn a_phase_that_began_and_ended_is_no_problem() {
    let phase = |name: &str, seq, end: bool| {
        let record = rust_mutants::trace::PhaseRecord {
            name: name.to_owned(),
            duration_ms: end.then_some(4),
        };
        event(
            seq,
            if end {
                Payload::PhaseEnd { phase: record }
            } else {
                Payload::PhaseStart { phase: record }
            },
        )
    };
    let events = vec![
        event(
            1,
            Payload::RunStart {
                schema: SCHEMA.to_owned(),
                engine: "0.1.0".to_owned(),
            },
        ),
        phase("prepare", 2, false),
        phase("validate", 3, false),
        phase("validate", 4, true),
        phase("prepare", 5, true),
        event(
            6,
            Payload::RunEnd {
                run: rust_mutants::trace::RunRecord {
                    outcome: "detected".to_owned(),
                    error: None,
                    events_emitted: 6,
                    events_dropped: 0,
                },
            },
        ),
    ];
    assert!(check(&events).is_empty(), "{:?}", check(&events));
}

/// One event of a hand-built recording, so a reader can be tested on a stream no run would produce.
fn event(seq: u64, payload: Payload) -> rust_mutants::trace::Event {
    rust_mutants::trace::Event {
        seq,
        timestamp: "2027-01-15T08:00:00Z".to_owned(),
        elapsed_ms: seq.saturating_mul(1000),
        payload,
    }
}

#[test]
fn a_summary_counts_the_types_times_every_phase_and_names_the_slowest_commands() {
    let recorder = memory_recorder();
    let outer = recorder.phase("prepare");
    let inner = recorder.phase("validate");
    recorder.exec(exec(&["cargo", "check", "--workspace"]));
    one_of_each_preparation(&recorder);
    inner.end();
    recorder.exec(exec(&["cargo", "test", "--no-run"]));
    one_of_each_measurement(&recorder);
    outer.end();
    recorder.run_end("detected", None);

    let summary = summarize(&recorder.events(), 2);
    assert_eq!(summary.events, recorder.events().len() as u64);
    assert_eq!(summary.dropped, 0);
    assert_eq!(
        summary.counts.get("exec").copied(),
        Some(3),
        "the types are counted: {:?}",
        summary.counts
    );

    let phases: Vec<&str> = summary
        .phases
        .iter()
        .map(|phase| phase.path.as_str())
        .collect();
    assert_eq!(
        phases,
        ["prepare", "prepare/validate"],
        "a nested phase is named by the path a reader would follow"
    );
    assert!(
        summary.phases.iter().all(|phase| phase.duration_ms > 0),
        "every phase says how long it took: {:?}",
        summary.phases
    );

    assert_eq!(summary.slowest.len(), 2, "only what was asked for");
    assert!(
        summary.slowest[0].duration_ms >= summary.slowest[1].duration_ms,
        "the slowest first: {:?}",
        summary.slowest
    );
    assert_eq!(
        summary.executions.get("killed").copied(),
        Some(1),
        "an execution is counted by what it established"
    );
    assert_eq!(
        summary.routes.get("block").copied(),
        Some(1),
        "and a route by what decided it"
    );
    assert_eq!(summary.rounds, 1, "one validation round");
    assert_eq!(summary.bisections, 5, "and what isolation cost");
}

#[test]
fn a_summary_renders_as_lines_a_person_reads_and_a_diff_says_what_moved() {
    let recorder = memory_recorder();
    let phase = recorder.phase("prepare");
    recorder.exec(exec(&["cargo", "check"]));
    phase.end();
    recorder.run_end("detected", None);
    let before = summarize(&recorder.events(), 3);

    let rendered = render(&before);
    assert!(rendered.contains("EVENTS\t"), "{rendered}");
    assert!(rendered.contains("PHASE\tprepare"), "{rendered}");
    assert!(rendered.contains("SLOWEST\t"), "{rendered}");

    let second = memory_recorder();
    let phase = second.phase("prepare");
    second.exec(exec(&["cargo", "check"]));
    second.exec(exec(&["cargo", "test"]));
    phase.end();
    second.run_end("detected", None);
    let after = summarize(&second.events(), 3);

    let moved = diff(&before, &after);
    assert!(
        moved
            .iter()
            .any(|change| change.what == "exec" && change.from == 1 && change.to == 2),
        "{moved:?}"
    );
    assert!(
        !moved.iter().any(|change| change.from == change.to),
        "a diff says what moved, not what stayed: {moved:?}"
    );
}
