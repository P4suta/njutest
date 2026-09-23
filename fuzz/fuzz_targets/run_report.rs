// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A stored run report is read back by `rust-mutants report`, by a later run, and by whatever a user pipes it into. A document this reader accepts must render, and one it rejects must say so rather than panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants_cli::report::lines;
use rust_mutants_cli::report::run::RunDocument;

fuzz_target!(|text: &str| {
    let Ok(document) = rust_mutants_cli::report::run::parse(text) else {
        return;
    };
    let Ok(rendered) = lines(&document) else {
        return;
    };
    assert!(
        rendered.contains("MUTANTS"),
        "every report renders its tally"
    );
    #[expect(
        clippy::expect_used,
        reason = "the fuzzer must crash when a parsed report cannot be serialized"
    )]
    let written = serde_json::to_string(&document).expect("what was read, writes");
    #[expect(
        clippy::expect_used,
        reason = "the fuzzer must crash when the serializer emits a report its parser refuses"
    )]
    let again: RunDocument =
        rust_mutants_cli::report::run::parse(&written).expect("what it writes, it reads");
    #[expect(
        clippy::expect_used,
        reason = "the fuzzer must crash when the reparsed report cannot be serialized"
    )]
    let again_written = serde_json::to_string(&again).expect("writes");
    assert_eq!(
        again_written,
        written,
        "the round trip is the identity"
    );
});
