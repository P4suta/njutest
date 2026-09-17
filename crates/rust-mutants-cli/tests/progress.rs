// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run says while it is still preparing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rust_mutants::trace::{Recorder, Sink};

/// A recorder whose events arrive on `receiver`, as the command line builds one for a display.
fn watched() -> (
    Recorder,
    std::sync::mpsc::Receiver<rust_mutants::trace::Event>,
) {
    let (sender, receiver) = std::sync::mpsc::channel();
    (
        Recorder::wall(Sink::Channel(rust_mutants::trace::ChannelSink::new(sender))),
        receiver,
    )
}

#[test]
fn a_phase_that_ended_is_written_before_the_work_that_follows_it_is_done() {
    let (recorder, events) = watched();
    let working = AtomicBool::new(true);
    let mut written: Vec<u8> = Vec::new();
    std::thread::scope(|scope| {
        let doing = scope.spawn(|| {
            let open = recorder.phase("open");
            open.end();
            let pristine = recorder.phase("pristine");
            pristine.end();
            let after_the_phases_a_reader_is_waiting_for = Duration::from_millis(120);
            std::thread::sleep(after_the_phases_a_reader_is_waiting_for);
            working.store(false, Ordering::SeqCst);
        });
        rust_mutants_cli::ui::watch(&events, &mut written, &|| working.load(Ordering::SeqCst));
        doing.join().expect("the work finishes");
    });
    let text = String::from_utf8(written).expect("the display writes text");
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
    rust_mutants_cli::ui::watch(&events, &mut written, &|| false);
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
    rust_mutants_cli::ui::watch(&events, &mut written, &|| true);
    assert!(
        written.is_empty(),
        "a recorder that is gone leaves a display with nothing to wait for"
    );
}
