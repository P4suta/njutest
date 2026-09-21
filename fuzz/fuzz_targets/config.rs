// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `.njutest.toml` is strict, and strictness is exactly what a hostile document tests: a configuration this parser *accepts* decides what a run does, so it must never accept something it cannot then describe, and it must never panic deciding.

#![no_main]

use libfuzzer_sys::fuzz_target;
use njutest_cli::config::Config;

fuzz_target!(|text: &str| {
    let Ok(config) = Config::parse(text, std::path::Path::new(".njutest.toml")) else {
        return;
    };
    let Ok(canonical) = config.canonical() else {
        return;
    };
    let Ok(digest) = config.digest() else {
        return;
    };
    assert_eq!(digest.len(), 64);
    assert_eq!(
        config.digest().unwrap_or_else(|_refused| digest.clone()),
        digest,
        "the digest is a function of the configuration"
    );

    #[expect(
        clippy::expect_used,
        reason = "the fuzzer must crash when canonical configuration is not JSON"
    )]
    let reparsed: serde_json::Value = njutest_devkit::strictjson::decode_str(&canonical)
        .expect("the canonical rendering is JSON");
    assert!(reparsed.is_object());
});
