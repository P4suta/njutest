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
        found.candidate.validate().expect("a candidate validates");
        let span = found.candidate.span;
        assert_eq!(
            &source[span.start as usize..span.end as usize],
            found.candidate.original.as_slice()
        );
        let site = found.hint.site;
        assert!(site.start <= span.start && span.end <= site.end);
    }
    let again = discover_file("src/lib.rs", source, &selection).expect("deterministic");
    assert_eq!(discovery, again);
});
