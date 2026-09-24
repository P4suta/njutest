// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The engine trace: diagnostic exhaust under the rules of ADR 0002.
//! Never a claim, never a failure, honest about drops.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::as_conversions,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::fs;
use std::io;

use njutest_devkit::result::{
    ResultState::{Refused, Returned},
    result_state,
};
use rust_mutants::testkit::trace::{
    memory_recorder, standalone_context, stepping_clock, type_names,
};
use rust_mutants::trace::summary::{diff, render, summarize};
use rust_mutants::trace::{
    DirSink, EVERY_TYPE, ExecRecord, FILE_NAME, MemorySink, NjutestBuild, OUTPUT_DIRECTORY_NAME,
    OUTPUT_FILE_LIMIT, OpenRecord, Payload, Problem, Recorder, RouteRecord, SCHEMA, Sink,
    SnapshotRecord, SweepRecord, TRUNCATION_MARKER, TraceContext, VerifyRecord, WitnessRecord,
    check, read_events,
};
use std::collections::BTreeSet;

use sha2::{Digest as _, Sha256};

enum RelevantPayload<'a> {
    PhaseStart(&'a rust_mutants::trace::PhaseRecord),
    PhaseEnd(&'a rust_mutants::trace::PhaseRecord),
    Exec(&'a ExecRecord),
    Other,
}

const fn relevant_payload(payload: &Payload) -> RelevantPayload<'_> {
    match payload {
        Payload::PhaseStart { phase } => RelevantPayload::PhaseStart(phase),
        Payload::PhaseEnd { phase } => RelevantPayload::PhaseEnd(phase),
        Payload::Exec { exec } => RelevantPayload::Exec(exec),
        Payload::RunStart { .. }
        | Payload::Open { .. }
        | Payload::Snapshot { .. }
        | Payload::DiscoverFile { .. }
        | Payload::Instrument { .. }
        | Payload::ValidateRound { .. }
        | Payload::Bisect { .. }
        | Payload::Build { .. }
        | Payload::Verify { .. }
        | Payload::Touch { .. }
        | Payload::Witness { .. }
        | Payload::SkipClaim { .. }
        | Payload::Kept { .. }
        | Payload::Route { .. }
        | Payload::Cache { .. }
        | Payload::Select { .. }
        | Payload::Identical { .. }
        | Payload::Evidence { .. }
        | Payload::MutantExec { .. }
        | Payload::Note { .. }
        | Payload::RunEnd { .. } => RelevantPayload::Other,
    }
}

/// A directory sink that has been closed, so everything written to it fails: the reachable form of "the disk is gone".
fn broken(dir: &std::path::Path) -> Sink {
    let sink = DirSink::create(&dir.join("recording")).expect("the sink");
    sink.close().expect("closed");
    Sink::required(sink)
}

/// One recorded execution of a process that ran and exited zero.
///
/// Written out rather than filled from a `Default`.
/// A process ends exactly one way, so the record has no value meaning nobody said which — and a builder that took one would put "could not be started" on a process that ran (ADR 0023).
fn exec(argv: &[&str]) -> ExecRecord {
    ExecRecord {
        argv: argv.iter().map(|s| (*s).to_owned()).collect(),
        dir: None,
        env_names: Vec::new(),
        timeout_ms: None,
        quiet_ms: None,
        stopped: rust_mutants::execute::Stopped::Exited {
            exit: rust_mutants::runner::ProcessExit::Code(0),
        },
        duration_ms: 0,
        output_bytes: 0,
        output_sha256: None,
        output_truncated: false,
        output_path: None,
        error: None,
        output: Vec::new(),
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
    recorder.run_end("ok", None).expect("trace closes");
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
fn nested_trace_context_is_a_closed_self_consistent_build_binding() {
    let final_run_id =
        rust_mutants::id::RunId::try_from("run").expect("the fixed final run id is canonical");
    let build = rust_mutants::cargo::BuildConfig::default().selection();
    let context = TraceContext::Njutest {
        build: NjutestBuild::new(final_run_id, 2, "release".to_owned(), &build)
            .expect("the nested binding derives its internal run id from its ordinal"),
    };
    let value = serde_json::to_value(&context).expect("the context serializes");
    assert_eq!(
        value,
        serde_json::json!({
            "kind": "njutest",
            "build": {
                "final_run_id": "run",
                "internal_run_id": "run-b0000000002",
                "ordinal": 2,
                "name": "release",
                "build_selection": build.digest(),
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<TraceContext>(value).expect("the exact binding reads back"),
        context
    );
}

#[test]
fn every_nested_build_has_a_namespace_distinct_from_the_final_run() {
    let final_run_id =
        rust_mutants::id::RunId::try_from("run").expect("the fixed final run id is canonical");
    let selection = rust_mutants::cargo::BuildConfig::default().selection();

    for (ordinal, expected) in [(0, "run-b0000000000"), (1, "run-b0000000001")] {
        let build = NjutestBuild::new(
            final_run_id.clone(),
            ordinal,
            format!("build-{ordinal}"),
            &selection,
        )
        .expect("the short final id can own either nested namespace");
        assert_eq!(build.internal_run_id().as_str(), expected);
        assert_ne!(build.internal_run_id(), build.final_run_id());
    }
}

#[test]
fn a_final_run_id_that_cannot_own_the_derived_build_namespace_is_refused() {
    let final_run_id = rust_mutants::id::RunId::try_from("a".repeat(64))
        .expect("the maximum-length final run id is canonical by itself");
    let selection = rust_mutants::cargo::BuildConfig::default().selection();
    let error = NjutestBuild::new(final_run_id, 0, "default".to_owned(), &selection)
        .expect_err("a nested namespace must not be silently truncated or aliased");
    assert!(matches!(
        error,
        rust_mutants::trace::NjutestBuildError::RunId { .. }
    ));
}

#[test]
fn nested_trace_context_rejects_partial_extra_and_cross_ordinal_bindings() {
    let exact = serde_json::json!({
        "kind": "njutest",
        "build": {
            "final_run_id": "run",
            "internal_run_id": "run-b0000000001",
            "ordinal": 1,
            "name": "release",
            "build_selection": "8ab0bfdf63e67552f235202347a8bd67247a83027fa8a321b61b759d3f35ab85",
        }
    });
    let parsed = serde_json::from_value::<TraceContext>(exact.clone());
    assert_eq!(result_state(&parsed), Returned, "{parsed:?}");

    let mut missing = exact.clone();
    missing["build"]
        .as_object_mut()
        .expect("the fixture build is an object")
        .remove("build_selection");
    let parsed = serde_json::from_value::<TraceContext>(missing);
    assert_eq!(result_state(&parsed), Refused, "{parsed:?}");

    let mut extra = exact.clone();
    extra["build"]["unbound"] = serde_json::json!(true);
    let parsed = serde_json::from_value::<TraceContext>(extra);
    assert_eq!(result_state(&parsed), Refused, "{parsed:?}");

    let mut swapped = exact;
    swapped["build"]["internal_run_id"] = serde_json::json!("run-b0000000002");
    let parsed = serde_json::from_value::<TraceContext>(swapped);
    assert_eq!(result_state(&parsed), Refused, "{parsed:?}");
}

#[test]
fn run_start_schema_requires_the_closed_trace_context() {
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &fs::read_to_string(
            njutest_devkit::paths::workspace_root().join("schema/rust-mutants-trace-v1.json"),
        )
        .expect("the schema file"),
    )
    .expect("the schema parses");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    let event = serde_json::json!({
        "seq": 1,
        "timestamp": "2027-01-15T08:00:00Z",
        "elapsed_ms": 0,
        "payload": {
            "type": "run-start",
            "schema": "rust-mutants-trace-v1",
            "engine": "0.1.0",
            "context": {
                "kind": "standalone",
                "run_id": "test",
                "build_selection": "8ab0bfdf63e67552f235202347a8bd67247a83027fa8a321b61b759d3f35ab85",
            }
        }
    });
    assert!(validator.is_valid(&event));

    let mut missing = event.clone();
    missing["payload"]
        .as_object_mut()
        .expect("the fixture payload is an object")
        .remove("context");
    assert!(!validator.is_valid(&missing));

    let mut malformed = event;
    malformed["payload"]["context"]["build_selection"] = serde_json::json!("not-a-digest");
    assert!(!validator.is_valid(&malformed));
}

#[test]
fn a_recording_starts_with_run_start_and_ends_with_run_end_carrying_the_accounting() {
    let recorder = memory_recorder();
    assert!(recorder.is_enabled());
    recorder.note("progress", "one");
    recorder.note("progress", "two");
    recorder.run_end("prepared", None).expect("trace closes");

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
    assert!(matches!(&events[0].payload, Payload::RunStart { .. }));
    let Payload::RunStart {
        schema,
        engine,
        context,
    } = &events[0].payload
    else {
        return;
    };
    assert_eq!(schema, SCHEMA);
    assert_eq!(engine, rust_mutants::VERSION);
    assert_eq!(context, &standalone_context());
    assert!(matches!(&events[3].payload, Payload::RunEnd { .. }));
    let Payload::RunEnd { run } = &events[3].payload else {
        return;
    };
    assert_eq!(run.outcome, "prepared");
    assert_eq!(run.error, None);
    assert_eq!(run.events_emitted, 3, "run-start and two notes");
    assert_eq!(run.events_dropped, 0);
}

#[test]
fn run_end_happens_once_and_nothing_is_recorded_afterwards() {
    let recorder = memory_recorder();
    recorder
        .run_end("errored", Some("boom".to_owned()))
        .expect("trace closes");
    recorder.note("progress", "too late");
    recorder.run_end("ok", None).expect("trace closes");
    let events = recorder.events();
    assert_eq!(type_names(&events), ["run-start", "run-end"]);
    assert!(matches!(&events[1].payload, Payload::RunEnd { .. }));
    let Payload::RunEnd { run } = &events[1].payload else {
        return;
    };
    assert_eq!(run.error.as_deref(), Some("boom"));
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
        let dropped_phase = recorder.phase("validate");
        drop(dropped_phase);
    }
    drop(outer);
    recorder.run_end("ok", None).expect("trace closes");
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
        .filter_map(|event| match relevant_payload(&event.payload) {
            RelevantPayload::PhaseStart(phase) | RelevantPayload::PhaseEnd(phase) => {
                Some((phase.name.clone(), phase.duration_ms))
            }
            RelevantPayload::Exec(_) | RelevantPayload::Other => None,
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
    let recorder = Recorder::wall(Sink::Memory(MemorySink::unbounded()), standalone_context());
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for thread in 0..8 {
            let recorder = recorder.clone();
            let worker = njutest_devkit::thread::ScopedThread::launch(scope, move || {
                for i in 0..50 {
                    recorder.note("thread", &format!("{thread}/{i}"));
                }
            });
            workers.push(worker);
        }
        for worker in workers {
            worker.join().expect("trace fixture worker joins");
        }
    });
    recorder.run_end("ok", None).expect("trace closes");
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
        stopped: rust_mutants::execute::Stopped::Exited {
            exit: rust_mutants::runner::ProcessExit::Code(101),
        },
        duration_ms: 12,
        ..exec(&["cargo", "test", "--", "--exact", "t::x"])
    };
    recorder.exec(record);
    recorder.run_end("ok", None).expect("trace closes");
    let events = recorder.events();
    assert!(
        matches!(&events[1].payload, Payload::Exec { .. }),
        "{:?}",
        events[1]
    );
    let Payload::Exec { exec } = &events[1].payload else {
        return;
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
    assert!(
        line.contains(
            "\"stopped\":{\"kind\":\"exited\",\"exit\":{\"kind\":\"code\",\"value\":101}}"
        ),
        "how a process ended is one field on the wire as in the type, so a reader cannot \
         be handed a status beside a flag that disagrees with it: {line}"
    );
}

#[test]
fn a_directory_sink_that_lost_events_fails_finalization() {
    let temp = tempfile::tempdir().expect("tempdir");
    let recorder = Recorder::new(broken(temp.path()), stepping_clock(), standalone_context());
    for i in 0..5 {
        recorder.note("n", &i.to_string());
    }
    let error = recorder
        .run_end("ok", None)
        .expect_err("a durable trace cannot hide lost events");
    assert!(error.to_string().contains("lost 7 event"), "{error}");
    assert!(recorder.events().is_empty());
}

#[test]
fn a_required_directory_failure_after_start_fails_finalization() {
    let temp = tempfile::tempdir().expect("tempdir");
    let directory = temp.path().join("required");
    let sink = DirSink::create(&directory).expect("the durable sink");
    let recorder = Recorder::new(Sink::required(sink), stepping_clock(), standalone_context());
    recorder.fail_durable_writes_for_test();
    recorder.note("after-start", "must be durable");
    let error = recorder
        .run_end("ok", None)
        .expect_err("an observer cannot turn a lost durable event into success");
    assert!(
        error
            .to_string()
            .contains("injected durable trace write failure"),
        "{error}"
    );
}

#[test]
fn a_required_directory_remains_authoritative_when_progress_disconnects() {
    let temp = tempfile::tempdir().expect("tempdir");
    let directory = temp.path().join("required");
    let sink = DirSink::create(&directory).expect("the durable sink");
    let (sender, receiver) = std::sync::mpsc::sync_channel(64);
    drop(receiver);
    let recorder = Recorder::new(
        Sink::required_with_channel(sink, rust_mutants::trace::ChannelSink::new(sender)),
        stepping_clock(),
        standalone_context(),
    );
    recorder.note("durable", "the channel is only an observer");
    recorder
        .run_end("ok", None)
        .expect("the durable authority kept every event");
    let text = fs::read_to_string(directory.join(FILE_NAME)).expect("the durable stream");
    assert_eq!(text.lines().count(), 3, "{text}");
}

#[test]
fn a_full_bounded_progress_observer_cannot_block_or_replace_durable_authority() {
    let temp = tempfile::tempdir().expect("tempdir");
    let directory = temp.path().join("required");
    let sink = DirSink::create(&directory).expect("the durable sink");
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let recorder = Recorder::new(
        Sink::required_with_channel(sink, rust_mutants::trace::ChannelSink::new(sender)),
        stepping_clock(),
        standalone_context(),
    );
    recorder.note("durable", "the bounded observer is already full");
    recorder
        .run_end("ok", None)
        .expect("only the durable authority decides completion");

    assert_eq!(
        receiver.try_iter().count(),
        1,
        "the observer stayed bounded"
    );
    let text = fs::read_to_string(directory.join(FILE_NAME)).expect("the durable stream");
    assert_eq!(text.lines().count(), 3, "{text}");
}

#[test]
fn output_preservation_failure_keeps_the_event_and_the_observers_original_capture() {
    let temp = tempfile::tempdir().expect("tempdir");
    let directory = temp.path().join("required");
    let sink = DirSink::create(&directory).expect("the durable sink");
    let (sender, receiver) = std::sync::mpsc::sync_channel(64);
    let recorder = Recorder::new(
        Sink::required_with_channel(sink, rust_mutants::trace::ChannelSink::new(sender)),
        stepping_clock(),
        standalone_context(),
    );
    fs::write(
        directory.join(OUTPUT_DIRECTORY_NAME),
        b"blocks the output directory",
    )
    .expect("the path-blocking file");
    recorder.exec(ExecRecord {
        output: b"full diagnostic capture".to_vec(),
        ..exec(&["cargo", "check"])
    });
    recorder
        .run_end("ok", None)
        .expect("the event itself remained durable");

    let observed: Vec<_> = receiver.try_iter().collect();
    let observed_exec = observed
        .iter()
        .find_map(|event| match relevant_payload(&event.payload) {
            RelevantPayload::Exec(exec) => Some(exec),
            RelevantPayload::PhaseStart(_)
            | RelevantPayload::PhaseEnd(_)
            | RelevantPayload::Other => None,
        })
        .expect("the observer received the execution");
    assert_eq!(observed_exec.output, b"full diagnostic capture");
    assert_eq!(observed_exec.output_path, None);

    let durable = fs::read_to_string(directory.join(FILE_NAME)).expect("the durable stream");
    let exec = durable
        .lines()
        .map(|line| {
            njutest_devkit::strictjson::decode_str::<serde_json::Value>(line)
                .expect("each durable event is strict JSON")
        })
        .find(|event| {
            event
                .pointer("/payload/type")
                .and_then(serde_json::Value::as_str)
                == Some("exec")
        })
        .expect("the durable stream kept the execution event");
    assert_eq!(
        exec.pointer("/payload/exec/output_path"),
        Some(&serde_json::Value::Null)
    );
    assert_eq!(
        exec.pointer("/payload/exec/output_bytes")
            .and_then(serde_json::Value::as_u64),
        Some(23)
    );
    assert!(
        exec.pointer("/payload/exec/output_sha256")
            .and_then(serde_json::Value::as_str)
            .is_some(),
        "the event still binds the whole capture even when its optional side file cannot be kept"
    );
}

#[test]
fn a_sink_that_counts_its_own_drops_is_the_authority() {
    let recorder = Recorder::new(
        Sink::Memory(MemorySink::bounded(4)),
        stepping_clock(),
        standalone_context(),
    );
    for i in 0..5 {
        recorder.note("n", &i.to_string());
    }
    recorder.run_end("ok", None).expect("trace closes");
    let events = recorder.events();
    assert_eq!(events.len(), 4, "the ring keeps the newest four");
    assert_eq!(type_names(&events), ["note", "note", "note", "run-end"]);
    assert!(matches!(&events[3].payload, Payload::RunEnd { .. }));
    let Payload::RunEnd { run } = &events[3].payload else {
        return;
    };
    assert_eq!(run.events_dropped, 2);
    assert_eq!(run.events_emitted, 4);
}

#[test]
fn a_recording_writes_one_json_line_per_event_and_the_reader_round_trips() {
    let temp = tempfile::tempdir().expect("tempdir");
    let stream = temp.path().join("recording");
    let recorder = Recorder::new(
        Sink::required(DirSink::create(&stream).expect("the sink")),
        stepping_clock(),
        standalone_context(),
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
    recorder.run_end("ok", None).expect("trace closes");

    let text = fs::read_to_string(stream.join(FILE_NAME)).expect("the stream");
    assert_eq!(text.lines().count(), 7);
    assert!(text.ends_with('\n'));
    for line in text.lines() {
        let value: serde_json::Value =
            njutest_devkit::strictjson::decode_str(line).expect("each line is one object");
        assert!(
            value.get("seq").is_some()
                && value
                    .get("payload")
                    .and_then(|payload| payload.get("type"))
                    .is_some(),
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
    let golden = njutest_devkit::paths::workspace_root()
        .join("crates/rust-mutants/tests/testdata/trace/basic.golden");
    let stable = text.replace(rust_mutants::VERSION, "<version>");
    njutest_devkit::golden::golden(&golden, stable.as_bytes()).expect("golden");
}

#[test]
fn dir_sink_claims_its_directory_exclusively_and_preserves_output_beside_the_stream() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().join("run-1");
    let sink = DirSink::create(&dir).expect("create");
    assert_eq!(sink.directory(), dir);
    let refused = DirSink::create(&dir).expect_err("the directory is already claimed");
    assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);

    let recorder = Recorder::new(Sink::required(sink), stepping_clock(), standalone_context());
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
    recorder.run_end("ok", None).expect("trace closes");

    let stream = fs::read_to_string(dir.join(FILE_NAME)).expect("stream");
    let events = read_events(stream.as_bytes()).expect("read");
    assert_eq!(
        type_names(&events),
        ["run-start", "exec", "exec", "exec", "run-end"]
    );
    let execs: Vec<&ExecRecord> = events
        .iter()
        .filter_map(|event| match relevant_payload(&event.payload) {
            RelevantPayload::Exec(exec) => Some(exec),
            RelevantPayload::PhaseStart(_)
            | RelevantPayload::PhaseEnd(_)
            | RelevantPayload::Other => None,
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
    let text = "{\"seq\":1,\"timestamp\":\"2027-01-15T08:00:00Z\",\"elapsed_ms\":0,\"payload\":{\"type\":\"run-start\",\"schema\":\"rust-mutants-trace-v1\",\"engine\":\"0\",\"context\":{\"kind\":\"standalone\",\"run_id\":\"test\",\"build_selection\":\"8ab0bfdf63e67552f235202347a8bd67247a83027fa8a321b61b759d3f35ab85\"}}}\nnot json\n";
    let error = read_events(text.as_bytes()).expect_err("the malformed line is refused");
    assert_eq!(error.line(), 2);
    assert!(error.to_string().contains("line 2"), "{error}");
}

#[test]
fn the_reader_rejects_duplicate_keys_at_every_owned_depth() {
    for text in [
        "{\"seq\":1,\"seq\":2,\"timestamp\":\"2027-01-15T08:00:00Z\",\"elapsed_ms\":0,\"payload\":{\"type\":\"run-start\",\"schema\":\"rust-mutants-trace-v1\",\"engine\":\"0\"}}\n",
        "{\"seq\":1,\"timestamp\":\"2027-01-15T08:00:00Z\",\"elapsed_ms\":0,\"payload\":{\"type\":\"run-start\",\"schema\":\"rust-mutants-trace-v1\",\"schema\":\"rust-mutants-trace-v1\",\"engine\":\"0\"}}\n",
    ] {
        let error = read_events(text.as_bytes()).expect_err("duplicate names are ambiguous");
        assert_eq!(error.line(), 1);
        assert!(
            error.to_string().contains("duplicate JSON object key"),
            "{error}"
        );
    }
}

#[test]
fn the_v1_reader_distinguishes_an_explicit_null_from_a_missing_field() {
    let event = rust_mutants::trace::Event {
        seq: 1,
        timestamp: "2027-01-15T08:00:00Z".to_owned(),
        elapsed_ms: 0,
        payload: Payload::Exec {
            exec: exec(&["cargo", "check"]),
        },
    };
    let exact = serde_json::to_string(&event).expect("an exact event");
    assert!(
        read_events(exact.as_bytes()).is_ok(),
        "the explicitly null fields are part of the v1 shape"
    );

    let mut missing = serde_json::to_value(event).expect("an event value");
    missing["payload"]["exec"]
        .as_object_mut()
        .expect("the exec record")
        .remove("error");
    let text = serde_json::to_string(&missing).expect("the malformed event");
    assert!(
        read_events(text.as_bytes()).is_err(),
        "missing and explicitly null are different v1 documents"
    );
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

    let recorder = Recorder::new(
        Sink::Memory(MemorySink::bounded(2)),
        stepping_clock(),
        standalone_context(),
    );
    recorder.note("a", "1");
    recorder.note("b", "2");
    recorder.run_end("ok", None).expect("trace closes");
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
        remembered: false,
        retried: false,
    });
    recorder.touch(rust_mutants::trace::TouchRecord {
        target: "demo/lib/demo".to_owned(),
        measured: rust_mutants::trace::Measurement::Baseline,
        passed: vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
        summary: rust_mutants::trace::SummaryRecord::Libtest { tests_run: Some(3) },
        reached_sites: vec![0, 1, 2, 3, 4, 5, 6],
        entered_bodies: Vec::new(),
        infected_sites: vec![1, 2],
        tests: 3,
        sites: 7,
        loose: 1,
        infected: 2,
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
    recorder.kept(rust_mutants::trace::KeptRecord {
        path: "/tmp/rust-mutants-snap-1".to_owned(),
        run_id: "20260101T000000000Z".to_owned(),
    });
    one_of_each_execution(recorder);
}

/// The last of it: what a run says about the mutants it put to the tests.
fn one_of_each_execution(recorder: &Recorder) {
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
        step_notice: None,
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
        granularity: rust_mutants::session::Granularity::Block,
        fallback: None,
        reaching: vec!["demo/lib/demo".to_owned()],
        considered: Vec::new(),
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
    let recorder = Recorder::new(Sink::required(sink), stepping_clock(), standalone_context());
    let phase = recorder.phase("prepare");
    one_of_each_preparation(&recorder);
    one_of_each_measurement(&recorder);
    phase.end();
    recorder.run_end("detected", None).expect("trace closes");

    let text = fs::read_to_string(dir.join(FILE_NAME)).expect("the stream");
    let events = read_events(text.as_bytes()).expect("read");
    let seen: BTreeSet<&str> = type_names(&events).into_iter().collect();
    let known: BTreeSet<&str> = EVERY_TYPE.iter().copied().collect();
    assert_eq!(
        seen, known,
        "the golden holds one line of every type the vocabulary knows, and no other"
    );

    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &fs::read_to_string(
            njutest_devkit::paths::workspace_root().join("schema/rust-mutants-trace-v1.json"),
        )
        .expect("the schema file"),
    )
    .expect("the schema parses");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    for line in text.lines() {
        let value: serde_json::Value =
            njutest_devkit::strictjson::decode_str(line).expect("a line is one object");
        let valid = validator.validate(&value);
        assert_eq!(result_state(&valid), Returned, "{line}\n{valid:?}");
    }

    let golden = njutest_devkit::paths::workspace_root()
        .join("crates/rust-mutants/tests/testdata/trace/events.golden");
    let stable = text.replace(rust_mutants::VERSION, "<version>");
    njutest_devkit::golden::golden(&golden, stable.as_bytes()).expect("golden");
}

#[test]
fn the_schema_and_the_vocabulary_name_the_same_types() {
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &fs::read_to_string(
            njutest_devkit::paths::workspace_root().join("schema/rust-mutants-trace-v1.json"),
        )
        .expect("the schema file"),
    )
    .expect("the schema parses");
    let named: BTreeSet<String> = schema["properties"]["payload"]["oneOf"]
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
fn the_trace_schema_ties_step_evidence_to_exactly_the_step_outcome() {
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &fs::read_to_string(
            njutest_devkit::paths::workspace_root().join("schema/rust-mutants-trace-v1.json"),
        )
        .expect("the schema file"),
    )
    .expect("the schema parses");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    let mut event = serde_json::json!({
        "seq": 1,
        "timestamp": "2027-01-15T08:00:00Z",
        "elapsed_ms": 0,
        "payload": {
            "type": "mutant-exec",
            "mutant": {
                "id": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "index": 0,
                "target": "demo/lib/demo",
                "outcome": "survived",
                "step_notice": null,
                "exit_code": 0,
                "duration_ms": 1,
                "tests_run": null,
                "signal": null,
                "failed_tests": [],
                "timeout_ms": 1000,
                "timeout_source": "configured",
                "alone": true
            }
        }
    });
    assert!(validator.is_valid(&event));

    event["payload"]["mutant"]["outcome"] = serde_json::json!("step_limit_reached");
    assert!(!validator.is_valid(&event), "a step fact needs its notice");
    event["payload"]["mutant"]["step_notice"] = serde_json::json!({
        "nonce": "00000000000000000000000000000001",
        "catalog": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "mutant": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "limit": 10,
        "observed": 11
    });
    assert!(validator.is_valid(&event));

    event["payload"]["mutant"]["outcome"] = serde_json::json!("survived");
    assert!(
        !validator.is_valid(&event),
        "a non-step outcome cannot carry step evidence"
    );
    event["payload"]["mutant"]["step_notice"] = serde_json::Value::Null;
    event["payload"]["mutant"]["outcome"] = serde_json::json!("future-outcome");
    assert!(
        !validator.is_valid(&event),
        "an unknown outcome requires a new trace schema"
    );
}

#[test]
fn a_phase_that_never_ended_is_a_problem_a_reader_is_told_about() {
    let events = vec![
        event(
            1,
            Payload::RunStart {
                schema: SCHEMA.to_owned(),
                engine: "0.1.0".to_owned(),
                context: standalone_context(),
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
                context: standalone_context(),
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
    recorder.run_end("detected", None).expect("trace closes");

    let summary = summarize(&recorder.events(), 2);
    assert_eq!(
        result_state(&summary),
        Returned,
        "the fixture trace must summarize exactly: {summary:?}"
    );
    let Ok(summary) = summary else { return };
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
    recorder.run_end("detected", None).expect("trace closes");
    let before = summarize(&recorder.events(), 3);
    assert_eq!(
        result_state(&before),
        Returned,
        "the fixture trace must summarize exactly: {before:?}"
    );
    let Ok(before) = before else { return };

    let rendered = render(&before);
    assert!(rendered.contains("EVENTS\t"), "{rendered}");
    assert!(rendered.contains("PHASE\tprepare"), "{rendered}");
    assert!(rendered.contains("SLOWEST\t"), "{rendered}");

    let second = memory_recorder();
    let phase = second.phase("prepare");
    second.exec(exec(&["cargo", "check"]));
    second.exec(exec(&["cargo", "test"]));
    phase.end();
    second.run_end("detected", None).expect("trace closes");
    let after = summarize(&second.events(), 3);
    assert_eq!(
        result_state(&after),
        Returned,
        "the fixture trace must summarize exactly: {after:?}"
    );
    let Ok(after) = after else { return };

    let moved = diff(&before, &after);
    assert!(
        moved
            .iter()
            .any(|change| change.what == "exec" && change.from == 1 && change.to == 2),
        "{moved:?}"
    );
    assert!(
        moved
            .iter()
            .any(|change| change.what == "events" && change.from < change.to),
        "and the total each recording holds: {moved:?}"
    );
    assert!(
        !moved.iter().any(|change| change.from == change.to),
        "a diff says what moved, not what stayed: {moved:?}"
    );
}

#[test]
fn a_summary_counts_how_many_times_each_program_was_started() {
    let recorder = memory_recorder();
    recorder.exec(exec(&["cargo", "check", "--workspace"]));
    recorder.exec(exec(&["cargo", "test", "--no-run"]));
    recorder.exec(exec(&["cargo", "test", "--no-run"]));
    recorder.exec(exec(&["rustc", "--print", "sysroot"]));
    recorder.run_end("detected", None).expect("trace closes");

    let summary = summarize(&recorder.events(), 2);
    assert_eq!(
        result_state(&summary),
        Returned,
        "the fixture trace must summarize exactly: {summary:?}"
    );
    let Ok(summary) = summary else { return };
    assert_eq!(
        summary.invocations.get("cargo").copied(),
        Some(3),
        "how many times a compiler was started is work; how long it took is the machine: {:?}",
        summary.invocations
    );
    assert_eq!(summary.invocations.get("rustc").copied(), Some(1));
    assert_eq!(summary.invocations.get("nothing").copied(), None);
    let text = render(&summary);
    assert!(
        text.contains("cargo") && text.contains("started"),
        "a reader watching the work fall wants the count, not only the clock: {text}"
    );
}

#[test]
fn the_schema_names_every_granularity_and_every_fallback_a_route_can_carry() {
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &fs::read_to_string(
            njutest_devkit::paths::workspace_root().join("schema/rust-mutants-trace-v1.json"),
        )
        .expect("the schema file"),
    )
    .expect("the schema parses");
    let route = schema["properties"]["payload"]["oneOf"]
        .as_array()
        .expect("one branch per type")
        .iter()
        .find(|branch| branch["properties"]["type"]["const"] == "route")
        .expect("the route branch");
    let named = |field: &str| -> BTreeSet<String> {
        route["properties"]["route"]["properties"][field]["enum"]
            .as_array()
            .expect("a route field is an enum")
            .iter()
            .filter_map(|one| one.as_str())
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(
        named("granularity"),
        rust_mutants::session::Granularity::ALL
            .iter()
            .map(|one| one.name().to_owned())
            .collect::<BTreeSet<String>>(),
        "a route the engine can record is one the schema accepts"
    );
    assert_eq!(
        named("fallback"),
        rust_mutants::session::Fallback::ALL
            .iter()
            .map(|one| one.name().to_owned())
            .collect::<BTreeSet<String>>(),
        "and so is every reason it can give for widening"
    );
}

#[test]
fn every_reason_a_select_record_can_carry_is_one_the_published_schema_allows() {
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(include_str!(
        "../../../schema/rust-mutants-trace-v1.json"
    ))
    .expect("the schema is JSON");
    let reasons = schema["properties"]["payload"]["oneOf"]
        .as_array()
        .expect("the payload alternatives")
        .iter()
        .find(|alternative| alternative["properties"]["type"]["const"] == "select")
        .map(|select| select["properties"]["select"]["properties"]["reason"]["enum"].clone())
        .expect("a select record");
    let published: Vec<&str> = reasons
        .as_array()
        .expect("a closed list")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    let written: Vec<&str> = rust_mutants::run::NotRunReason::ALL
        .iter()
        .map(|reason| reason.name())
        .collect();
    assert_eq!(
        published, written,
        "the engine writes the name of every reason a mutant did not run, so the schema lists each"
    );
}
