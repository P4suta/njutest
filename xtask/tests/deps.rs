// SPDX-FileCopyrightText: 2026 njutest contributors
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
        edge("njutest-cli", "rust-mutants", EdgeKind::Normal),
        edge("njutest-cli", "njutest", EdgeKind::Normal),
        edge("rust-mutants-cli", "rust-mutants", EdgeKind::Normal),
        edge("njutest", "njutest-macros", EdgeKind::Normal),
    ];
    assert!(check(&allowed).is_empty());

    let refused = [
        edge("rust-mutants", "njutest-cli", EdgeKind::Normal),
        edge("rust-mutants", "njutest", EdgeKind::Normal),
        edge("njutest-macros", "njutest", EdgeKind::Normal),
        edge("njutest", "rust-mutants", EdgeKind::Normal),
        edge("xtask", "rust-mutants", EdgeKind::Normal),
    ];
    assert_eq!(check(&refused), refused);
}

#[test]
fn the_devkit_is_a_dev_dependency_of_anybody_and_a_dependency_of_nobody() {
    assert!(check(&[edge("rust-mutants", "njutest-devkit", EdgeKind::Dev)]).is_empty());
    assert!(check(&[edge("njutest-cli", "njutest-devkit", EdgeKind::Dev)]).is_empty());
    let refused = [edge("rust-mutants", "njutest-devkit", EdgeKind::Normal)];
    assert_eq!(check(&refused), refused);
}

#[test]
fn a_crate_may_dev_depend_on_itself_to_enable_its_own_testkit_feature() {
    assert!(check(&[edge("njutest-cli", "njutest-cli", EdgeKind::Dev)]).is_empty());
}

#[test]
fn a_refused_edge_is_reported_in_words() {
    assert_eq!(
        edge("rust-mutants", "njutest-cli", EdgeKind::Normal).to_string(),
        "rust-mutants depends on njutest-cli"
    );
    assert_eq!(
        edge("a", "b", EdgeKind::Dev).to_string(),
        "a dev-depends on b"
    );
}
