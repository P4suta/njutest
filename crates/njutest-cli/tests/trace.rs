// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The runner's trace: diagnostic exhaust under the rules of ADR 0002.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::as_conversions,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::fs;
use std::io;
use std::time::Duration;

use jiff::Timestamp;
use njutest_cli::trace::{
    ArtifactRecord, AskedRecord, Clock, DirSink, DischargeRecord, Event, ExecRecord, FILE_NAME,
    MemorySink, MutantExecRecord, Payload, ProbeExecRecord, Problem, ProgressRecord, RING_CAPACITY,
    Recorder, RouteRecord, SCHEMA, Sink, StartRecord, check, read_events,
};
use sha2::{Digest as _, Sha256};

/// A clock that advances one second per reading, from a fixed origin.
fn stepping_clock() -> Clock {
    Clock::stepping(
        Timestamp::from_second(1_800_000_000).expect("in range"),
        Duration::from_secs(1),
    )
}

/// A recorder over a memory sink, which is what a test reads back: the recorder owns its sink and answers with what the sink kept.
fn recording() -> Recorder {
    Recorder::new(
        Sink::Memory(MemorySink::unbounded()),
        stepping_clock(),
        start(),
    )
}

fn start() -> StartRecord {
    StartRecord::of(
        "20260905T081500Z-abcdef",
        njutest_cli::report::RunKind::Full,
        njutest_cli::config::Contract::StandardV1,
    )
}

fn types(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .map(|event| event.payload.type_name().to_owned())
        .collect()
}

#[test]
fn the_disabled_recorder_keeps_nothing_and_says_so() {
    let trace = Recorder::disabled();
    assert!(!trace.is_enabled());
    trace.note("phase", "nothing is listening");
    trace.phase("baseline").end();
    trace.run_end("ASSURED", None, None);
}

#[test]
fn a_recording_opens_with_run_start_and_closes_with_run_end() {
    let trace = recording();
    trace.note("note", "in between");
    trace.run_end("ASSURED", None, None);

    let events = trace.events();
    assert_eq!(types(&events), ["run-start", "note", "run-end"]);
    let Payload::RunStart { start } = &events[0].payload else {
        panic!("a run-start first: {:?}", events[0]);
    };
    assert_eq!(start.schema, SCHEMA);
    assert_eq!(start.schema, "njutest-trace-v1");
    assert_eq!(start.njutest, njutest_cli::VERSION);
    assert_eq!(start.rust_mutants, rust_mutants::VERSION);
    assert_eq!(start.run_id, "20260905T081500Z-abcdef");
    let Payload::RunEnd { run } = &events[2].payload else {
        panic!("a run-end last: {:?}", events[2]);
    };
    assert_eq!(run.verdict, "ASSURED");
    assert_eq!(
        run.events_emitted, 2,
        "the run-start and the note; a recording cannot count the event it is writing"
    );
    assert_eq!(run.events_dropped, 0);
}

#[test]
fn sequence_numbers_run_from_one_and_the_elapsed_time_is_measured_from_the_start() {
    let trace = recording();
    trace.note("a", "one");
    trace.note("b", "two");
    trace.run_end("ERROR", None, None);

    let events = trace.events();
    assert_eq!(
        events.iter().map(|event| event.seq).collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
    assert_eq!(
        events
            .iter()
            .map(|event| event.elapsed_ms)
            .collect::<Vec<_>>(),
        [0, 1_000, 2_000, 3_000],
        "the stepping clock advances one second per reading"
    );
    assert!(events[0].timestamp.starts_with("2027-01-15T"), "RFC 3339");
}

#[test]
fn a_phase_ends_once_whether_the_caller_ends_it_or_drops_it() {
    let trace = recording();
    {
        let phase = trace.phase("baseline");
        phase.end();
    }
    drop(trace.phase("mutation"));
    trace.run_end("INSUFFICIENT", None, None);

    let events = trace.events();
    assert_eq!(
        types(&events),
        [
            "run-start",
            "phase-start",
            "phase-end",
            "phase-start",
            "phase-end",
            "run-end"
        ]
    );
    let Payload::PhaseEnd { phase } = &events[2].payload else {
        panic!("a phase-end: {:?}", events[2]);
    };
    assert_eq!(phase.name, "baseline");
    assert_eq!(phase.duration_ms, Some(1_000));
}

#[test]
fn phases_nest_and_each_guard_times_its_own() {
    let trace = recording();
    let outer = trace.phase("verify");
    let inner = trace.phase("baseline");
    inner.end();
    outer.end();
    trace.run_end("ASSURED", None, None);

    let events = trace.events();
    assert_eq!(
        types(&events),
        [
            "run-start",
            "phase-start",
            "phase-start",
            "phase-end",
            "phase-end",
            "run-end"
        ]
    );
    let durations: Vec<Option<u64>> = events
        .iter()
        .filter_map(|event| match &event.payload {
            Payload::PhaseEnd { phase } => Some(phase.duration_ms),
            _ => None,
        })
        .collect();
    assert_eq!(durations, [Some(1_000), Some(3_000)]);
}

#[test]
fn a_recording_ends_once_and_keeps_nothing_after() {
    let trace = recording();
    trace.run_end("ASSURED", None, None);
    trace.note("late", "after the end");
    trace.run_end("DEFECT", None, None);

    let events = trace.events();
    assert_eq!(types(&events), ["run-start", "run-end"]);
    let Payload::RunEnd { run } = &events[1].payload else {
        panic!("a run-end: {:?}", events[1]);
    };
    assert_eq!(run.verdict, "ASSURED", "the first end is the one");
}

#[test]
fn an_exec_event_carries_environment_names_and_never_a_value() {
    let trace = recording();
    trace.exec(ExecRecord {
        argv: vec!["cargo".to_owned(), "test".to_owned()],
        env_names: vec![
            "RUSTFLAGS".to_owned(),
            "AWS_SECRET_ACCESS_KEY".to_owned(),
            "RUSTFLAGS".to_owned(),
        ],
        ..ExecRecord::default()
    });
    trace.run_end("ASSURED", None, None);

    let events = trace.events();
    let Payload::Exec { exec } = &events[1].payload else {
        panic!("an exec: {:?}", events[1]);
    };
    assert_eq!(
        exec.env_names,
        ["AWS_SECRET_ACCESS_KEY", "RUSTFLAGS"],
        "sorted and deduplicated"
    );
    let line = serde_json::to_string(&events[1]).expect("one line");
    assert!(!line.contains("secret-value"), "{line}");
}

#[test]
fn an_exec_event_digests_the_output_rather_than_carrying_it() {
    let trace = recording();
    let output = b"error: something a person would want to grep for".to_vec();
    trace.exec(ExecRecord {
        argv: vec!["cargo".to_owned()],
        output: output.clone(),
        ..ExecRecord::default()
    });
    trace.run_end("DEFECT", None, None);

    let events = trace.events();
    let Payload::Exec { exec } = &events[1].payload else {
        panic!("an exec: {:?}", events[1]);
    };
    assert_eq!(exec.output_bytes, output.len() as u64);
    assert_eq!(
        exec.output_sha256.as_deref(),
        Some(hex::encode(Sha256::digest(&output)).as_str())
    );
    let line = serde_json::to_string(&events[1]).expect("one line");
    assert!(
        !line.contains("would want to grep"),
        "the capture rides along for a sink that preserves it, never onto the wire: {line}"
    );
}

#[test]
fn a_progress_note_and_an_artifact_are_records_of_their_own() {
    let trace = recording();
    trace.progress(ProgressRecord {
        message: "running targets".to_owned(),
        done: Some(3),
        total: Some(7),
    });
    trace.artifact(ArtifactRecord {
        kind: "kept-temp".to_owned(),
        path: "/tmp/njutest-run-abcdef".to_owned(),
        bytes: Some(4_096),
    });
    trace.run_end("ASSURED", None, None);

    assert_eq!(
        types(&trace.events()),
        ["run-start", "progress", "artifact", "run-end"]
    );
}

#[test]
fn a_full_ring_drops_its_oldest_and_the_run_end_says_how_many() {
    let trace = Recorder::new(
        Sink::Memory(MemorySink::bounded(3)),
        stepping_clock(),
        start(),
    );
    for index in 0..5_u32 {
        trace.note("fill", &index.to_string());
    }
    trace.run_end("ASSURED", None, None);

    let events = trace.events();
    assert_eq!(events.len(), 3, "the newest three");
    let Payload::RunEnd { run } = &events[2].payload else {
        panic!("a run-end last: {:?}", events[2]);
    };
    assert_eq!(
        run.events_dropped, 3,
        "the run-start and the first two notes, which is all it could know about"
    );
    assert_eq!(run.events_emitted, 3);
    let problems = check(&events);
    assert!(
        problems.contains(&Problem::Dropped(3)),
        "the reader repeats what the run admitted: {problems:?}"
    );
    assert!(
        problems.contains(&Problem::MissingRunStart),
        "and sees that the beginning itself is gone, which the run-end could not say: {problems:?}"
    );
}

#[test]
fn the_default_ring_holds_the_last_events_of_a_run_that_asked_for_no_trace() {
    assert_eq!(RING_CAPACITY, 4096);
    assert!(Sink::ring().events().is_empty());
}

#[test]
fn a_sink_that_cannot_write_costs_the_count_and_never_the_run() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let sink = DirSink::create(&dir.path().join("recording")).expect("the sink");
    sink.close().expect("closed");

    let trace = Recorder::new(Sink::Dir(sink), stepping_clock(), start());
    trace.note("note", "into the void");
    trace.run_end("ASSURED", None, None);
}

#[test]
fn a_tee_keeps_what_one_sink_keeps_when_the_other_cannot() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let broken = DirSink::create(&dir.path().join("recording")).expect("the sink");
    broken.close().expect("closed");

    let trace = Recorder::new(
        Sink::Tee(vec![
            Sink::Dir(broken),
            Sink::Memory(MemorySink::unbounded()),
        ]),
        stepping_clock(),
        start(),
    );
    trace.note("note", "kept by one of them");
    trace.run_end("ASSURED", None, None);

    assert_eq!(
        types(&trace.events()),
        ["run-start", "note", "run-end"],
        "a full disk must not cost the ring the last thing the run did"
    );
}

#[test]
fn a_directory_sink_writes_one_json_object_per_line_and_the_reader_reads_it_back() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let recording = dir.path().join("trace/20260905T081500Z-1234");
    let sink = DirSink::create(&recording).expect("the sink");
    let trace = Recorder::new(Sink::Dir(sink), stepping_clock(), start());
    trace.phase("baseline").end();
    trace.run_end("ASSURED", None, None);

    let path = recording.join(FILE_NAME);
    let text = fs::read_to_string(&path).expect("the stream");
    assert_eq!(text.lines().count(), 4, "{text}");
    for line in text.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("one object per line");
        assert!(value.get("type").is_some(), "{line}");
        assert!(value.get("seq").is_some(), "{line}");
    }
    let events = read_events(io::BufReader::new(
        fs::File::open(&path).expect("the stream opens"),
    ))
    .expect("the events read back");
    assert_eq!(
        types(&events),
        ["run-start", "phase-start", "phase-end", "run-end"]
    );
    assert!(check(&events).is_empty(), "{:?}", check(&events));
}

#[test]
fn the_reader_reports_a_gap_a_missing_end_and_what_the_run_said_it_dropped() {
    let trace = recording();
    trace.note("a", "one");
    trace.note("b", "two");
    let mut events = trace.events();

    let unfinished = check(&events);
    assert!(
        unfinished.contains(&Problem::MissingRunEnd),
        "a stream with no end is a run that did not finish: {unfinished:?}"
    );

    let lost = events.remove(1);
    let gapped = check(&events);
    assert!(
        gapped.contains(&Problem::SequenceGap {
            expected: lost.seq,
            found: lost.seq.saturating_add(1),
        }),
        "a gap says which number was expected and which arrived, because that is what \
         says how many events went and where: a reader told only that there was a gap \
         cannot tell one lost event from a thousand. It lost {} and said {gapped:?}",
        lost.seq
    );
}

/// One event, as a recording writes it down.
fn written() -> String {
    let trace = recording();
    trace.note("a", "one");
    serde_json::to_string(&trace.events()[0]).expect("an event is a document")
}

/// A reader that gives what it holds and then stops working, which is a recording on a disk going away under it.
struct Failing {
    line: String,
    given: bool,
}

impl io::Read for Failing {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("the recording went away"))
    }
}

impl io::BufRead for Failing {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.given {
            return Err(io::Error::other("the recording went away"));
        }
        self.given = true;
        Ok(self.line.as_bytes())
    }

    fn consume(&mut self, _amount: usize) {}
}

#[test]
fn a_recording_that_stops_being_readable_is_an_error_and_never_a_shorter_recording() {
    let stopped = read_events(Failing {
        line: format!("{}\n", written()),
        given: false,
    })
    .expect_err("a stream that stops part way through");
    assert!(
        matches!(stopped, njutest_cli::trace::ReadError::Io { .. }),
        "the events before the failure are not the recording: reading them as one hands \
         an audit a run that ended where the disk did, with no run-end and no way to \
         tell that from a run somebody killed. It said {stopped}"
    );
}

#[test]
fn a_blank_line_is_passed_over_and_a_line_that_is_not_an_event_names_itself() {
    let one = written();
    let read = read_events(io::BufReader::new(
        format!("{one}\n\n   \n{one}\n").as_bytes(),
    ))
    .expect("a recording with blank lines in it");
    assert_eq!(
        read.len(),
        2,
        "a blank line is nothing a run wrote, so it is passed over rather than taken as \
         the end: a recording flushed in pieces has them, and stopping at the first \
         would hand an audit the beginning of a run and call it the whole"
    );

    let refused = read_events(io::BufReader::new(
        format!("{one}\n\n{{\"seq\":2}}\n").as_bytes(),
    ))
    .expect_err("a line that is not an event");
    assert!(
        matches!(
            refused,
            njutest_cli::trace::ReadError::Malformed { line: 3, .. }
        ),
        "and a line that is not an event names the line it is on, counted from one and \
         counting the blank ones, because a person opens the file at that number. It \
         said {refused}"
    );
}

#[test]
fn a_recording_that_lost_one_event_says_so_like_any_other() {
    let trace = recording();
    trace.run_end("ASSURED", None, None);
    let mut events = trace.events();
    let Some(Payload::RunEnd { run }) = events.last_mut().map(|event| &mut event.payload) else {
        panic!("a run-end last");
    };
    run.events_dropped = 1;
    let problems = check(&events);
    assert!(
        problems.contains(&Problem::Dropped(1)),
        "one event lost is a recording with a hole in it, and a reader who is told about \
         three but not about one has a threshold nobody wrote down between a complete \
         recording and an incomplete one: {problems:?}"
    );
}

fn a_route() -> RouteRecord {
    RouteRecord {
        mutant: "9e5cc4f98f8e".to_owned(),
        granularity: "block".to_owned(),
        fallback: None,
        reaching: vec!["core/test/lib fast".to_owned()],
        discharged: vec![
            DischargeRecord {
                target: "core/test/lib outside".to_owned(),
                proof: "branch-never-taken".to_owned(),
            },
            DischargeRecord {
                target: "core/test/lib inside".to_owned(),
                proof: "never-infected".to_owned(),
            },
        ],
        tests: vec![AskedRecord {
            target: "core/test/lib fast".to_owned(),
            tests: vec!["adds".to_owned(), "subtracts".to_owned()],
        }],
        considered: Vec::new(),
        reused: None,
        refused: Some("key-changed".to_owned()),
    }
}

#[test]
fn a_stage_ends_where_the_next_begins_and_the_last_ends_with_the_run() {
    let trace = recording();

    trace.stage("open");
    trace.stage("baseline");
    trace.run_end("INSUFFICIENT", None, None);

    let events = trace.events();
    assert_eq!(
        types(&events),
        [
            "run-start",
            "phase-start",
            "phase-end",
            "phase-start",
            "phase-end",
            "run-end"
        ],
        "a run reaches one stage at a time, and what no module guards is still somewhere"
    );
    let named: Vec<(&str, Option<u64>)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            Payload::PhaseStart { phase } | Payload::PhaseEnd { phase } => {
                Some((phase.name.as_str(), phase.duration_ms))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        named,
        [
            ("open", None),
            ("open", Some(1000)),
            ("baseline", None),
            ("baseline", Some(1000))
        ]
    );
}

#[test]
fn a_route_names_every_target_a_proof_discharged_beside_the_proof() {
    let trace = recording();
    trace.route(a_route());
    trace.mutant_exec(MutantExecRecord {
        mutant: "9e5cc4f98f8e".to_owned(),
        target: "core/test/lib fast".to_owned(),
        args: vec!["--exact".to_owned(), "tests::adds".to_owned()],
        outcome: "killed".to_owned(),
        duration_ms: 42,
        alone: false,
    });
    trace.probe_exec(ProbeExecRecord {
        target: "core/test/lib fast".to_owned(),
        outcome: "measured".to_owned(),
        infected: Some(3),
    });
    trace.run_end("INSUFFICIENT", None, None);

    let events = trace.events();
    assert_eq!(
        types(&events),
        ["run-start", "route", "mutant-exec", "probe-exec", "run-end"],
        "a routing decision has a shape of its own rather than a sentence in a note"
    );
    let Some(Payload::Route { route }) = events.get(1).map(|event| &event.payload) else {
        panic!("the route event")
    };
    assert_eq!(
        route
            .discharged
            .iter()
            .map(|one| (one.target.as_str(), one.proof.as_str()))
            .collect::<Vec<(&str, &str)>>(),
        [
            ("core/test/lib outside", "branch-never-taken"),
            ("core/test/lib inside", "never-infected")
        ],
        "two layers answer for one route, and a reader who cannot tell which removed a test \
         cannot audit either"
    );
}

#[test]
fn the_wire_shape_is_the_recorded_one() {
    let trace = recording();
    let phase = trace.phase("baseline");
    trace.exec(ExecRecord {
        argv: vec!["cargo".to_owned(), "test".to_owned(), "--no-run".to_owned()],
        dir: Some("/w".to_owned()),
        env_names: vec!["CARGO_TARGET_DIR".to_owned()],
        timeout_ms: Some(600_000),
        exit_code: Some(0),
        duration_ms: 1_200,
        output: b"ok".to_vec(),
        ..ExecRecord::default()
    });
    trace.progress(ProgressRecord {
        message: "1 of 2".to_owned(),
        done: Some(1),
        total: Some(2),
    });
    phase.end();
    trace.route(a_route());
    trace.mutant_exec(MutantExecRecord {
        mutant: "9e5cc4f98f8e".to_owned(),
        target: "core/test/lib fast".to_owned(),
        args: vec!["--exact".to_owned(), "tests::adds".to_owned()],
        outcome: "killed".to_owned(),
        duration_ms: 42,
        alone: false,
    });
    trace.probe_exec(ProbeExecRecord {
        target: "core/test/lib fast".to_owned(),
        outcome: "not-measured".to_owned(),
        infected: None,
    });
    trace.note("limitation", "mutation-phase-not-implemented");
    trace.run_end("INSUFFICIENT", None, None);

    let mut lines = Vec::new();
    for event in trace.events() {
        lines.extend_from_slice(serde_json::to_string(&event).expect("one line").as_bytes());
        lines.push(b'\n');
    }
    let golden =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/trace.golden.jsonl");
    njutest_devkit::golden::golden(&golden, &lines).expect("the recorded stream");
}

#[test]
fn a_stage_and_the_work_inside_it_do_not_answer_to_one_name() {
    let trace = recording();
    trace.stage("mutation");
    let inner = trace.phase("mutation-judge");
    inner.end();
    trace.stage("equivalence");
    trace.run_end("ASSURED", None, None);
    assert!(
        !check(&trace.events())
            .iter()
            .any(|problem| matches!(problem, Problem::PhaseRepeated { .. })),
        "a stage and the work inside it are two phases with two names, and a recording \
         that names them apart has nothing to answer for: {:?}",
        check(&trace.events())
    );

    let twice = recording();
    twice.stage("equivalence");
    let same = twice.phase("equivalence");
    same.end();
    twice.run_end("ASSURED", None, None);
    let problems = check(&twice.events());
    assert!(
        problems.contains(&Problem::PhaseRepeated {
            name: "equivalence".to_owned(),
            times: 2
        }),
        "while a recording that opens one name twice is one whose phase durations are \
         summed by name, so that phase is reported at the sum of the two and a person \
         reading it goes looking for time the run never spent there: {problems:?}"
    );
}
