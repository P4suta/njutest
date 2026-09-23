// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Kani's raw export is an untrusted protocol boundary. Arbitrary bytes must
//! either inhabit one of its three closed outcomes or fail closed, never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use njutest::testkit::{ModelResultClass, model_result};

fuzz_target!(|data: &[u8]| {
    match model_result(data) {
        ModelResultClass::Proved
        | ModelResultClass::Noticed
        | ModelResultClass::Undecided => {}
    }
});
