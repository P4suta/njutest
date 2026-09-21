// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Normalizing any path never panics, and a normalized path is a fixed point: normalizing it again gives the same answer.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants::id::normalize_path;

fuzz_target!(|path: &str| {
    match normalize_path(path) {
        Ok(normalized) => {
            assert!(!normalized.contains('\\'));
            assert!(!normalized.starts_with('/'));
            #[expect(
                clippy::expect_used,
                reason = "the fuzzer must crash when normalized output is not accepted as normalized"
            )]
            let again = normalize_path(&normalized).expect("a normalized path normalizes");
            assert_eq!(again, normalized);
        }
        Err(_rejected_path) => {}
    }
});
