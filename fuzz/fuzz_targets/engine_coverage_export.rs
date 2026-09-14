// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The engine's own coverage reader. Its answer decides which targets a mutation is run against, so a document it misreads narrows a route to targets that never reached the place and reports a mutation as surviving that nothing ran. It never panics, and every region it accepts is one `contains` can answer about: a region that ended before it began describes nothing.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants::coverage::{covered, instrumented, parse_export};

fuzz_target!(|data: &[u8]| {
    let Ok(files) = parse_export(data) else {
        return;
    };
    for file in &files {
        for region in &file.regions {
            assert!(
                (region.start.line, region.start.column) <= (region.end.line, region.end.column),
                "a region that ends before it starts: {region:?}"
            );
        }
    }
    let reached = covered(&files);
    let built = instrumented(&files);
    assert!(
        reached.is_subset(&built),
        "what a run reached is part of what the build instrumented"
    );
});
