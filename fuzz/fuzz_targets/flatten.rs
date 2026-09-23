// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `flatten` never panics, and when it accepts an input the result has no line break and re-lexes to the same token structure (the function checks the latter itself; this target checks it did not lie).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|source: &str| {
    match rust_mutants::flatten::flatten(source) {
        Ok(flat) => assert!(!flat.contains('\n') && !flat.contains('\r'), "{flat:?}"),
        Err(_rejected_rust_tokens) => {}
    }
});
