// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A stored report is read back by `mjutest report`, `diagnostics`, and the
//! cache. A document this reader accepts is one the rest of the program
//! will act on, so it must round-trip and it must still satisfy the audit
//! that let it be written in the first place.

#![no_main]

use libfuzzer_sys::fuzz_target;
use mjutest_cli::report::{audit, json};

fuzz_target!(|text: &str| {
    let Ok(report) = json::parse(text) else {
        return;
    };
    // What was read writes back and reads back equal: a projection that
    // lost a field would make two readers of one run disagree.
    let rendered = json::render(&report).expect("a report renders");
    assert_eq!(json::parse(&rendered).expect("it reads back"), report);

    // The record stream never forges a record out of what the document
    // carried, whatever a hostile document put in a message.
    let stream = mjutest_cli::report::lines::stream(&report);
    let verdicts = stream
        .lines()
        .filter(|line| line.split('\t').next() == Some("VERDICT"))
        .count();
    assert_eq!(verdicts, 1);

    // And a document that fails the audit is one `document` refuses to
    // write, however it was obtained.
    if !audit::validate_for_persistence(&report).is_empty() {
        json::document(&report).expect_err("an unsound report is not written");
    }
});
