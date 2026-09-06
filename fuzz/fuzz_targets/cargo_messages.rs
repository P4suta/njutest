// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The JSON message stream cargo writes while it builds. Validation reads it to decide which mutant each compiler error is about, so a reader that misreads it condemns a mutant the compiler never refused. It never panics, and every diagnostic it accepts either names a primary span or names none at all rather than half of one.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants::cargo::{Message, parse_messages};

fuzz_target!(|data: &[u8]| {
    let Ok(messages) = parse_messages(data) else {
        return;
    };
    for message in &messages {
        let Message::CompilerMessage(compiler) = message else {
            continue;
        };
        let Some(span) = compiler.message.primary_span() else {
            continue;
        };
        assert!(span.is_primary, "the primary span is a primary span");
        assert!(
            !span.file_name.is_empty(),
            "a span that names no file: {span:?}"
        );
        assert!(
            span.byte_start <= span.byte_end,
            "a span that ends before it starts: {span:?}"
        );
    }
});
