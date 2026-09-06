// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run report says about the run, without starting one.

use rust_mutants_cli::config::Config;
use rust_mutants_cli::outcomes::Keyed;
use rust_mutants_cli::report::selection_document;

fn keyed(build: &Config) -> Keyed {
    Keyed {
        workspace: "w".to_owned(),
        catalog: "c".to_owned(),
        args: Vec::new(),
        timeout_ms: 1000,
        build: build.build.config().arguments(),
    }
}

#[test]
fn the_build_configuration_enters_the_document_and_the_cache_key() {
    let mut configured = Config::default();
    configured.build.features = vec!["extra".to_owned()];

    assert_eq!(
        selection_document(&configured).build,
        vec!["--features".to_owned(), "extra".to_owned()],
        "a reader has to be able to tell which program was measured"
    );
    assert!(selection_document(&Config::default()).build.is_empty());

    assert_ne!(
        keyed(&configured).key("m"),
        keyed(&Config::default()).key("m"),
        "the same tree compiled with different features is a different program, and a record \
         kept for one of them answers nothing about the other"
    );
}
