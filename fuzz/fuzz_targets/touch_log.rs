// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The log the guards append to, which is what routing by test and a discharge by non-infection both rest on. A truncated line or a header naming another catalog read as though it were this run's would take tests out of a route on evidence about something else, so it is read fail-closed: it never panics, it yields no facts at all where it yields an error, and every index it accepts is one this catalog holds.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants::touch::read;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let catalog = "0".repeat(64);
    for count in [0u32, 1, 7, 4096] {
        let Ok(touches) = read(text, &catalog, count) else {
            continue;
        };
        for seen in [&touches.reached, &touches.bodies, &touches.infected] {
            for index in seen
                .loose
                .iter()
                .chain(seen.tests.values().flat_map(|indices| indices.iter()))
            {
                assert!(
                    *index < count,
                    "an index the catalog does not hold: {index} of {count}"
                );
            }
            for (test, held) in &seen.tests {
                assert!(!test.is_empty(), "a record attributed to nothing at all");
                for index in held {
                    assert!(seen.by(test, *index), "a record its own test cannot see");
                    assert!(seen.any(*index), "a record nothing of the target made");
                }
            }
        }
    }
});
