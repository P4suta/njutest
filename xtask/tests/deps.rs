// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The dependency direction rule.

use xtask::deps::{Edge, EdgeKind, check};

fn edge(from: &str, to: &str, kind: EdgeKind) -> Edge {
    Edge {
        from: from.to_owned(),
        to: to.to_owned(),
        kind,
    }
}

#[test]
fn the_runner_may_depend_on_the_engine_and_the_api_but_not_the_reverse() {
    let allowed = [
        edge("mjutest-cli", "rust-mutants", EdgeKind::Normal),
        edge("mjutest-cli", "mjutest", EdgeKind::Normal),
        edge("rust-mutants-cli", "rust-mutants", EdgeKind::Normal),
        edge("mjutest", "mjutest-macros", EdgeKind::Normal),
    ];
    assert!(check(&allowed).is_empty());

    let refused = [
        edge("rust-mutants", "mjutest-cli", EdgeKind::Normal),
        edge("rust-mutants", "mjutest", EdgeKind::Normal),
        edge("mjutest-macros", "mjutest", EdgeKind::Normal),
        edge("mjutest", "rust-mutants", EdgeKind::Normal),
        edge("xtask", "rust-mutants", EdgeKind::Normal),
    ];
    assert_eq!(check(&refused), refused);
}

#[test]
fn the_devkit_is_a_dev_dependency_of_anybody_and_a_dependency_of_nobody() {
    assert!(check(&[edge("rust-mutants", "mjutest-devkit", EdgeKind::Dev)]).is_empty());
    assert!(check(&[edge("mjutest-cli", "mjutest-devkit", EdgeKind::Dev)]).is_empty());
    let refused = [edge("rust-mutants", "mjutest-devkit", EdgeKind::Normal)];
    assert_eq!(check(&refused), refused);
}

#[test]
fn a_crate_may_dev_depend_on_itself_to_enable_its_own_testkit_feature() {
    assert!(check(&[edge("mjutest-cli", "mjutest-cli", EdgeKind::Dev)]).is_empty());
}

#[test]
fn a_refused_edge_is_reported_in_words() {
    assert_eq!(
        edge("rust-mutants", "mjutest-cli", EdgeKind::Normal).to_string(),
        "rust-mutants depends on mjutest-cli"
    );
    assert_eq!(
        edge("a", "b", EdgeKind::Dev).to_string(),
        "a dev-depends on b"
    );
}
