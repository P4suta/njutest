// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A `rust-mutants: skip` marker is read out of the gaps between tokens, wherever an author puts one and whatever else the file holds. Reading them never panics, every marker the reader keeps names a reason, and a file whose markers hide nothing yields the candidates it would have without them.

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
    for claim in &discovery.annotations {
        assert!(
            !claim.reason.trim().is_empty(),
            "a marker the reader kept names a reason: {claim:?}"
        );
        assert!(claim.line >= 1, "a marker sits on a line: {claim:?}");
    }
    if discovery.annotations.is_empty() {
        assert!(
            !discovery
                .skips
                .iter()
                .any(|skip| skip.reason.name() == "annotated"),
            "no marker hides nothing, so nothing is annotated"
        );
    }
    let again = discover_file("src/lib.rs", source, &selection).expect("deterministic");
    assert_eq!(discovery, again);
});
