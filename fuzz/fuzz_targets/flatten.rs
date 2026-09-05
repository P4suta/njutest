// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `flatten` never panics, and when it accepts an input the result has no
//! line break and re-lexes to the same token structure (the function checks
//! the latter itself; this target checks it did not lie).

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|source: &str| {
    if let Ok(flat) = rust_mutants::flatten::flatten(source) {
        assert!(!flat.contains('\n') && !flat.contains('\r'), "{flat:?}");
    }
});
