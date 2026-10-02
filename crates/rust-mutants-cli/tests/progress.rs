// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run says while it is still preparing.

use njutest_devkit::thread::ScopedThread;
use rust_mutants::trace::{Recorder, Sink};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

struct Heard<'a> {
    written: Vec<u8>,
    acknowledgement: Option<std::sync::mpsc::SyncSender<()>>,
    working: &'a AtomicBool,
}

impl Write for Heard<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.written.extend_from_slice(bytes);
        if self.written.contains(&b'\n')
            && let Some(acknowledgement) = self.acknowledgement.take()
        {
            if !self.working.load(Ordering::SeqCst) {
                return Err(io::Error::other(
                    "the reader did not observe the phase before its producer finished",
                ));
            }
            acknowledgement.send(()).map_err(io::Error::other)?;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_phase_whose_end_carries_no_duration_ends_the_stream_rather_than_taking_no_time() {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    sender
        .send(rust_mutants::trace::Event {
            seq: 1,
            timestamp: "2026-09-29T00:00:00Z".to_owned(),
            elapsed_ms: 0,
            payload: rust_mutants::trace::Payload::PhaseEnd {
                phase: rust_mutants::trace::PhaseRecord {
                    name: "prepare".to_owned(),
                    duration_ms: None,
                },
            },
        })
        .expect("the channel holds one event");
    drop(sender);
    let mut written: Vec<u8> = Vec::new();
    let refused = rust_mutants_cli::stream::watch(&receiver, &mut written, &|| false)
        .expect_err("a phase's end with no duration is no line the stream can write");
    assert!(
        refused.to_string().contains("prepare"),
        "the refusal names the phase: {refused}"
    );
    assert_eq!(
        written,
        Vec::<u8>::new(),
        "nothing was written as though the phase took no time"
    );
}

/// A recorder whose events arrive on `receiver`, as the command line builds one for a display.
fn watched() -> (
    Recorder,
    std::sync::mpsc::Receiver<rust_mutants::trace::Event>,
) {
    let (sender, receiver) = std::sync::mpsc::sync_channel(64);
    (
        Recorder::wall(
            Sink::Channel(rust_mutants::trace::ChannelSink::waking(
                sender,
                std::thread::current(),
            )),
            rust_mutants::testkit::trace::standalone_context(),
        ),
        receiver,
    )
}

#[test]
fn a_phase_that_ended_is_written_before_the_work_that_follows_it_is_done() {
    let (recorder, events) = watched();
    let working = AtomicBool::new(true);
    let (acknowledgement, heard) = std::sync::mpsc::sync_channel(1);
    let mut written = Heard {
        written: Vec::new(),
        acknowledgement: Some(acknowledgement),
        working: &working,
    };
    let reader = std::thread::current();
    std::thread::scope(|scope| {
        let working = &working;
        let doing = ScopedThread::launch(scope, move || {
            let open = recorder.phase("open");
            open.end();
            heard
                .recv()
                .expect("the reader has written the first phase");
            let pristine = recorder.phase("pristine");
            pristine.end();
            working.store(false, Ordering::SeqCst);
            reader.unpark();
        });
        rust_mutants_cli::ui::watch(&events, &mut written, &|| working.load(Ordering::SeqCst))
            .expect("the in-memory progress stream accepts every line");
        doing.join().expect("the observed work finishes");
    });
    let text = String::from_utf8(written.written).expect("the display writes text");
    assert!(
        text.lines().any(|line| line.starts_with("open")),
        "the first phase is the first thing a reader hears: {text}"
    );
    assert!(
        text.lines().any(|line| line.starts_with("pristine")),
        "and so is the next one: {text}"
    );
}

#[test]
fn a_display_that_is_told_the_work_is_over_stops_looking() {
    let (recorder, events) = watched();
    let phase = recorder.phase("open");
    phase.end();
    let mut written: Vec<u8> = Vec::new();
    rust_mutants_cli::ui::watch(&events, &mut written, &|| false)
        .expect("the in-memory progress stream accepts every line");
    let text = String::from_utf8(written).expect("the display writes text");
    assert!(
        text.lines().any(|line| line.starts_with("open")),
        "what had already arrived is still written: {text}"
    );
}

#[test]
fn a_display_with_nothing_to_say_says_nothing_and_returns() {
    let (recorder, events) = watched();
    drop(recorder);
    let mut written: Vec<u8> = Vec::new();
    rust_mutants_cli::ui::watch(&events, &mut written, &|| true)
        .expect("the in-memory progress stream accepts every line");
    assert!(
        written.is_empty(),
        "a recorder that is gone leaves a display with nothing to wait for"
    );
}

#[test]
fn observed_displays_retain_actual_waits_without_waking_on_their_own_notes() {
    for streaming in [false, true] {
        observed_display_waits(streaming);
    }
}

/// Runs one real subscribed display and independently reads its durable wait authority.
fn observed_display_waits(streaming: bool) {
    use rust_mutants::observation::{Event, Observation};
    let observed = Observation::subscribe();
    let producer = observed.signal();
    let directory = tempfile::tempdir().expect("the actual authority's directory");
    let trace = directory.path().join("trace");
    let (sender, events) = std::sync::mpsc::sync_channel(64);
    let recorder = Recorder::wall(
        Sink::required_with_channel(
            rust_mutants::trace::DirSink::create(&trace).expect("the actual durable recorder"),
            rust_mutants::trace::ChannelSink::observed(sender, observed.signal()),
        ),
        rust_mutants::testkit::trace::standalone_context(),
    );
    let working = AtomicBool::new(true);
    let begin = AtomicBool::new(false);
    let (start, started) = std::sync::mpsc::sync_channel(1);
    let (acknowledgement, heard) = std::sync::mpsc::sync_channel(1);
    let mut written = Heard {
        written: Vec::new(),
        acknowledgement: Some(acknowledgement),
        working: &working,
    };
    std::thread::scope(|scope| {
        let recorder = &recorder;
        let working = &working;
        let doing = ScopedThread::launch(scope, move || {
            started
                .recv()
                .expect("the subscribed display begins waiting");
            recorder.phase("open").end();
            heard
                .recv()
                .expect("the reader acknowledged the actual phase");
            recorder.phase("pristine").end();
            working.store(false, Ordering::Release);
            producer.publish(Event::Completed);
        });
        let alive = || {
            if !begin.swap(true, Ordering::AcqRel) {
                start.send(()).expect("release the subscribed producer");
            }
            working.load(Ordering::Acquire)
        };
        let watched = if streaming {
            rust_mutants_cli::stream::watch_observed(
                &events,
                &mut written,
                &alive,
                (&observed, recorder),
            )
        } else {
            rust_mutants_cli::ui::watch_observed(
                &events,
                &mut written,
                &alive,
                (&observed, recorder),
            )
        };
        watched.expect("every producer observation and output is retained");
        doing.join().expect("the real producer is joined");
    });
    let text = String::from_utf8(written.written).expect("the actual display's text");
    assert!(text.contains("open") && text.contains("pristine"), "{text}");
    retained_display_waits(&trace, &events);
}

/// Requires genuine host measurements without recirculating them as presentation wakes.
fn retained_display_waits(
    trace: &std::path::Path,
    events: &std::sync::mpsc::Receiver<rust_mutants::trace::Event>,
) {
    let actual = rust_mutants::trace::read_events(io::BufReader::new(
        std::fs::File::open(trace.join(rust_mutants::trace::FILE_NAME))
            .expect("the actual authority stream"),
    ))
    .expect("the independently read durable authority");
    let waits: Vec<_> = actual
        .iter()
        .filter_map(|event| {
            if let rust_mutants::trace::Payload::Note { note } = &event.payload
                && note.kind == "host-wait"
            {
                Some(
                    njutest_devkit::strictjson::decode_str::<serde_json::Value>(&note.detail)
                        .expect("the actual measured host wait"),
                )
            } else {
                None
            }
        })
        .collect();
    assert!(!waits.is_empty(), "the authority retains the actual waits");
    for wait in waits {
        assert_eq!(
            wait.get("owner").and_then(serde_json::Value::as_str),
            Some("workspace-preparation")
        );
        assert!(
            wait.get("elapsed_ns")
                .and_then(serde_json::Value::as_u64)
                .is_some()
        );
        assert!(
            wait.get("machine")
                .and_then(|machine| machine.get("cpus"))
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|cpus| cpus > 0)
        );
    }
    assert!(
        events.try_iter().all(|event| !matches!(&event.payload,
            rust_mutants::trace::Payload::Note { note } if note.kind == "host-wait")),
        "a display's own waits never become another display wake"
    );
}

#[test]
fn an_observed_display_refuses_a_producer_failure_after_its_last_phase() {
    for streaming in [false, true] {
        let observed = rust_mutants::observation::Observation::subscribe();
        let (sender, events) = std::sync::mpsc::sync_channel(64);
        let recorder = Recorder::wall(
            Sink::Channel(rust_mutants::trace::ChannelSink::observed(
                sender,
                observed.signal(),
            )),
            rust_mutants::testkit::trace::standalone_context(),
        );
        recorder.phase("prepare").end();
        observed.signal().failed(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the actual producer lost its terminal evidence",
        ));
        let mut written = Vec::new();
        let watched = if streaming {
            rust_mutants_cli::stream::watch_observed(
                &events,
                &mut written,
                &|| false,
                (&observed, &recorder),
            )
        } else {
            rust_mutants_cli::ui::watch_observed(
                &events,
                &mut written,
                &|| false,
                (&observed, &recorder),
            )
        };
        let refusal = watched.expect_err("an ended display erased the producer's retained failure");
        assert!(
            refusal.to_string().contains("lost its terminal evidence"),
            "{refusal}"
        );
    }
}
