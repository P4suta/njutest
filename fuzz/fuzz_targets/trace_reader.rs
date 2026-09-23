// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The trace reader never panics on a hostile stream, and what it accepts round-trips through the writer's encoding and reads back equal.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants::trace::{check, read_events};

fuzz_target!(|data: &[u8]| {
    let Ok(events) = read_events(data) else {
        return;
    };
    let problems = check(&events);
    std::hint::black_box(problems);
    let mut again = Vec::new();
    for event in &events {
        #[expect(
            clippy::expect_used,
            reason = "the fuzzer must crash when a parsed event cannot be serialized"
        )]
        again.extend(serde_json::to_vec(event).expect("an event serializes"));
        again.push(b'\n');
    }
    #[expect(
        clippy::expect_used,
        reason = "the fuzzer must crash when the writer emits a trace its reader refuses"
    )]
    let reread = read_events(again.as_slice()).expect("the writer's encoding reads back");
    assert_eq!(events, reread);
});
