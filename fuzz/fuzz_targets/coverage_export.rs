// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The coverage export comes from another program, and a run that misreads it routes every mutant wrongly while looking perfectly healthy. So the reader never panics, and what it accepts describes real regions: a block that ended before it began would make `contains` answer nonsense.

#![no_main]

use libfuzzer_sys::fuzz_target;
use njutest::coverage::{covered, instrumented, parse_export};

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
    assert!(reached.is_subset(&built));
});
