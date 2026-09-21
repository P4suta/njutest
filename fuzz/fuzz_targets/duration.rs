// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every bound a person writes in a configuration file is a duration this parser read. A parse that silently rounds or overflows turns a two-minute bound into something else and every timeout after it is about the wrong thing. It never panics, and what it renders it reads back as the same duration.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants::duration::{parse, render};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(value) = parse(text) else {
        return;
    };
    let rendered = render(value);
    #[expect(
        clippy::expect_used,
        reason = "the fuzzer must crash when the renderer emits text its own parser refuses"
    )]
    let again = parse(&rendered).expect("what the parser renders it reads back");
    assert_eq!(
        again, value,
        "{text:?} became {value:?}, which renders as {rendered:?} and reads back as {again:?}"
    );
});
