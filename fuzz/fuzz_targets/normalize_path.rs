// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Normalizing any path never panics, and a normalized path is a fixed point: normalizing it again gives the same answer.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants::id::normalize_path;

fuzz_target!(|path: &str| {
    if let Ok(normalized) = normalize_path(path) {
        assert!(!normalized.contains('\\'));
        assert!(!normalized.starts_with('/'));
        let again = normalize_path(&normalized).expect("a normalized path normalizes");
        assert_eq!(again, normalized);
    }
});
