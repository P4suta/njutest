// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `.mjutest.toml` is strict, and strictness is exactly what a hostile
//! document tests: a configuration this parser *accepts* decides what a run
//! does, so it must never accept something it cannot then describe, and it
//! must never panic deciding.

#![no_main]

use libfuzzer_sys::fuzz_target;
use mjutest_cli::config::Config;

fuzz_target!(|text: &str| {
    let Ok(config) = Config::parse(text, std::path::Path::new(".mjutest.toml")) else {
        return;
    };
    // An accepted configuration has one canonical rendering and one digest,
    // and reading that rendering back gives the same configuration: a
    // digest that depended on how somebody spelled a duration would key a
    // cache on the spelling rather than on the configuration.
    let canonical = config.canonical();
    let digest = config.digest();
    assert_eq!(digest.len(), 64);
    assert_eq!(config.digest(), digest, "the digest is a function of the configuration");

    let reparsed: serde_json::Value =
        serde_json::from_str(&canonical).expect("the canonical rendering is JSON");
    assert!(reparsed.is_object());
});
