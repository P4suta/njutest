// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A stored run report is read back by `rust-mutants report`, by a later run, and by whatever a user pipes it into. A document this reader accepts must render, and one it rejects must say so rather than panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants_cli::report::run::{RunDocument, lines};

fuzz_target!(|text: &str| {
    let Ok(document) = serde_json::from_str::<RunDocument>(text) else {
        return;
    };
    let rendered = lines(&document);
    assert!(
        rendered.contains("MUTANTS"),
        "every report renders its tally"
    );
    let written = serde_json::to_string(&document).expect("what was read, writes");
    let again: RunDocument = serde_json::from_str(&written).expect("what it writes, it reads");
    assert_eq!(
        serde_json::to_string(&again).expect("writes"),
        written,
        "the round trip is the identity"
    );
});
