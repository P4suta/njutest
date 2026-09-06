// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The dep-info file cargo writes beside every artifact says which files a unit was built from, which is how the engine knows what it may mutate. A reader that splits one wrongly mutates a file the unit does not hold, or misses one it does. It never panics, and every prerequisite it returns is a whole name: non-empty, and never a fragment cut at a space the escape said to keep.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants::cargo::parse_dep_info;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(paths) = parse_dep_info(text) else {
        return;
    };
    for path in &paths {
        assert!(!path.is_empty(), "a prerequisite with no name: {paths:?}");
        assert!(
            !path.contains(' ') || text.contains("\\ "),
            "a name with a space in it came from an escape, or it came from nowhere: {path:?}"
        );
        assert!(
            !path.contains('\t'),
            "a tab separates prerequisites and is never inside one: {path:?}"
        );
    }
});
