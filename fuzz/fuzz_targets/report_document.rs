// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A stored report is read back by `njutest report`, `diagnostics`, and the cache. A document this reader accepts is one the rest of the program will act on, so it must round-trip and it must still satisfy the audit that let it be written in the first place.

#![no_main]

use libfuzzer_sys::fuzz_target;
use njutest_cli::report::{audit, json};

fuzz_target!(|text: &str| {
    let Ok(report) = json::parse(text) else {
        return;
    };
    #[expect(
        clippy::expect_used,
        reason = "the fuzzer must crash when a parsed report cannot be rendered"
    )]
    let rendered = json::render(&report).expect("a report renders");
    #[expect(
        clippy::expect_used,
        reason = "the fuzzer must crash when the renderer emits a report its parser refuses"
    )]
    let reparsed = json::parse(&rendered).expect("it reads back");
    assert_eq!(reparsed, report);

    let Ok(stream) = njutest_cli::report::lines::stream(&report) else {
        return;
    };
    let verdicts = stream
        .lines()
        .filter(|line| line.split('\t').next() == Some("VERDICT"))
        .count();
    assert_eq!(verdicts, 1);

    if !audit::validate_for_persistence(&report).is_empty() {
        #[expect(
            clippy::expect_used,
            reason = "the fuzzer must crash when persistence accepts an unsound report"
        )]
        json::document(&report).expect_err("an unsound report is not written");
    }
});
