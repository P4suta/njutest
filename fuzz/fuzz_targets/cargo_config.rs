// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A cargo configuration file says what a build compiles with, and a coverage build has to put those flags back because the variable it uses replaces them. A reader that invents a flag, drops one, or hands on a flag holding the separator that variable splits on compiles something other than the project's own binaries. It never panics, and what it accepts can always be encoded back as itself.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants::cargo::config::{SEPARATOR, encoded, read};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let found = read(text);
    if found.unreadable {
        assert!(
            found.build.is_empty(),
            "a file this could not read says nothing about flags: {found:?}"
        );
        return;
    }
    for flag in &found.build {
        assert!(
            !flag.contains(SEPARATOR),
            "a flag holding the separator would reach the compiler as two: {flag:?}"
        );
    }
    let Ok(value) = encoded(&rust_mutants::vars::Variables::empty(), &found, &[]) else {
        return;
    };
    let Some(value) = value else {
        assert!(found.build.is_empty());
        return;
    };
    assert!(
        value.to_str().is_some(),
        "flags read from UTF-8 TOML encoded as a non-UTF-8 argument"
    );
    let Some(encoded) = value.to_str() else {
        return;
    };
    let parts: Vec<String> = encoded
        .split(SEPARATOR)
        .map(ToOwned::to_owned)
        .collect();
    assert_eq!(
        parts, found.build,
        "what was encoded is what was read, argument for argument"
    );
    assert_eq!(read(text), found, "reading is a function of the text alone");
});
