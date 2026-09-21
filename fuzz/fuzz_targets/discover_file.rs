// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Discovery never panics on any input, and every candidate it proposes is coherent: it validates, its original bytes are the span's bytes, its edit lies inside its site, and the result is deterministic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants::rule::{Registry, Tier};
use rust_mutants::syntax::{Selection, discover_file};

static REGISTRY: Registry = Registry::canonical();

fuzz_target!(|source: &[u8]| {
    let selection = Selection::tier(&REGISTRY, Tier::All);
    let Ok(discovery) = discover_file("src/lib.rs", source, &selection) else {
        return;
    };
    for found in &discovery.candidates {
        #[expect(
            clippy::expect_used,
            reason = "the fuzzer must crash when discovery produces an invalid candidate"
        )]
        found.candidate.validate().expect("a candidate validates");
        let span = found.candidate.span;
        let Ok(start) = usize::try_from(span.start) else {
            return;
        };
        let Ok(end) = usize::try_from(span.end) else {
            return;
        };
        assert_eq!(
            source.get(start..end),
            Some(found.candidate.original.as_slice()),
            "a candidate's span names exactly its original bytes"
        );
        let site = found.hint.site;
        assert!(site.start <= span.start && span.end <= site.end);
    }
    #[expect(
        clippy::expect_used,
        reason = "the fuzzer must crash when the same discovery input has two answers"
    )]
    let again = discover_file("src/lib.rs", source, &selection).expect("deterministic");
    assert_eq!(discovery, again);
});
