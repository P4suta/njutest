// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The log the probe runtime appends to, which is what a discharge by non-infection rests on. Reading a truncated or foreign log as though it were this run's would remove executions on evidence about something else, so it is read fail-closed: it never panics, and every index it accepts is one this catalog holds.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants::probe::log::read;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let catalog = "0".repeat(64);
    for count in [0u32, 1, 7, 4096] {
        let Ok(indices) = read(text, &catalog, count) else {
            continue;
        };
        for index in &indices {
            assert!(
                *index < count,
                "an index the catalog does not hold: {index} of {count}"
            );
        }
    }
});
