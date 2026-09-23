// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `.rust-mutants.toml` decides what a run mutates and what it believes. A document this parser *accepts* must be one every later stage can honour, so what it accepts is checked here against the same rules the reader states, and no document may make it panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rust_mutants_cli::config::{ALLOWED_TEST_ARGS, Config, allowed_test_arg};

fuzz_target!(|text: &str| {
    let Ok(config) = Config::parse(text, std::path::Path::new(".rust-mutants.toml")) else {
        return;
    };
    assert_eq!(config.version, 1, "only version 1 is ever accepted");
    for pattern in config.project.include.iter().chain(&config.project.exclude) {
        #[expect(
            clippy::expect_used,
            reason = "the fuzzer must crash when accepted configuration carries an invalid glob"
        )]
        rust_mutants::glob::Pattern::compile(pattern).expect("an accepted pattern compiles");
    }
    let registry = rust_mutants::rule::Registry::canonical();
    for operator in &config.mutation.operators {
        assert!(registry.lookup(operator).is_some(), "{operator}");
    }
    for argument in &config.execution.test_binary_args {
        assert!(allowed_test_arg(argument), "{argument} of {ALLOWED_TEST_ARGS:?}");
    }
    let mut named = std::collections::BTreeSet::new();
    for expectation in &config.mutation.expect {
        assert!(!expectation.reason.trim().is_empty(), "a claim says why");
        assert!(
            matches!(
                expectation.outcome(),
                Some(
                    rust_mutants::outcome::Outcome::Survived
                        | rust_mutants::outcome::Outcome::Killed
                )
            ),
            "only an outcome a run confirms may be expected: {:?}",
            expectation.outcome
        );
        assert!(
            named.insert(expectation.name()),
            "two claims name {:?}, and a mutant has one reason",
            expectation.name()
        );
    }
    let directory = &config.reports.directory;
    assert!(!directory.as_os_str().is_empty());
    assert!(!directory.is_absolute());

    #[expect(
        clippy::expect_used,
        reason = "the fuzzer must crash when accepted configuration cannot be serialized"
    )]
    let rendered = toml::to_string(&config).expect("an accepted configuration writes back");
    #[expect(
        clippy::expect_used,
        reason = "the fuzzer must crash when the serializer emits configuration the parser refuses"
    )]
    let again = Config::parse(&rendered, std::path::Path::new(".rust-mutants.toml"))
        .expect("what it writes, it reads");
    assert_eq!(again, config, "the round trip is the identity");
});
