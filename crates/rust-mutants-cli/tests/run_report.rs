// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run report says about the run, without starting one.

use rust_mutants::report::catalog::selection_document;
use rust_mutants_cli::config::Config;
use rust_mutants_cli::outcomes::Keyed;

fn options(config: &Config) -> rust_mutants::session::PrepareOptions {
    rust_mutants::session::PrepareOptions {
        build: config.build.config(),
        ..rust_mutants::session::PrepareOptions::default()
    }
}

fn keyed(build: &Config) -> Keyed {
    Keyed {
        closure: "w".to_owned(),
        manifests: "m".to_owned(),
        toolchain: "cargo 1.98.0 rustc 1.98.0 x86_64-unknown-linux-gnu".to_owned(),
        args: Vec::new(),
        timeout: "auto".to_owned(),
        steps: build.mutation.steps,
        build: build.build.config().arguments(),
        engine: "e".to_owned(),
        runner: None,
        declared: rust_mutants::outcomes::Declared::of(
            &std::collections::BTreeSet::new(),
            &rust_mutants::vars::Variables::empty(),
        ),
    }
}

#[test]
fn the_build_configuration_enters_the_document_and_the_cache_key() {
    let mut configured = Config::default();
    configured.build.features = vec!["extra".to_owned()];

    assert_eq!(
        selection_document(&options(&configured)).build,
        vec!["--features".to_owned(), "extra".to_owned()],
        "a reader has to be able to tell which program was measured"
    );
    assert!(
        selection_document(&options(&Config::default()))
            .build
            .is_empty()
    );

    let mutant =
        rust_mutants::id::HexDigest::try_from("a".repeat(64)).expect("a canonical mutant id");
    assert_ne!(
        keyed(&configured).key(&mutant),
        keyed(&Config::default()).key(&mutant),
        "the same tree compiled with different features is a different program, and a record \
         kept for one of them answers nothing about the other"
    );
}
