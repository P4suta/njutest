// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Asking a test whether its own assertions are load-bearing.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use rust_mutants::rule::{Registry, Tier};
use rust_mutants::syntax::{FileDiscovery, Selection, SkipReason, discover_file};

static REGISTRY: Registry = Registry::canonical();

const SOURCE: &str = "\
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::add;

    #[test]
    fn it_adds() {
        let total = add(2, 2);
        assert!(total > 3);
    }
}
";

/// Where the test module starts, as a byte offset into the source above.
fn boundary() -> u32 {
    u32::try_from(SOURCE.find("mod tests").expect("the module")).expect("a span")
}

fn discovered(asking: bool) -> FileDiscovery {
    let mut selection = Selection::tier(&REGISTRY, Tier::All);
    if asking {
        selection = selection.asking_the_tests();
    }
    discover_file("src/lib.rs", SOURCE.as_bytes(), &selection).expect("the file parses")
}

#[test]
fn a_run_that_is_not_asking_the_tests_leaves_their_own_code_alone() {
    let found = discovered(false);
    assert!(
        found
            .candidates
            .iter()
            .all(|one| one.candidate.span.start < boundary()),
        "mutating a test and then asking whether the tests notice is a question \
         about nothing, which is why the walk passes over test code by default"
    );
    assert!(
        found
            .skips
            .iter()
            .any(|skip| skip.reason == SkipReason::TestCode),
        "and it says so rather than passing over it silently: {:?}",
        found.skips
    );
}

#[test]
fn a_run_asking_the_tests_catalogues_what_their_assertions_are_made_of() {
    let found = discovered(true);
    let at = boundary();
    let inside: Vec<&rust_mutants::syntax::Found> = found
        .candidates
        .iter()
        .filter(|one| one.candidate.span.start >= at)
        .collect();
    assert!(
        !inside.is_empty(),
        "a test whose assertion can be weakened without the test failing is a test \
         that asserts nothing, and the only way to ask is to weaken it: {:?}",
        found.candidates
    );
    assert!(
        inside
            .iter()
            .any(|one| one.candidate.rule.name == "gt-to-ge"),
        "the comparison in `assert!(total > 3)` is exactly the thing to weaken: {inside:?}"
    );
    assert!(
        !found
            .skips
            .iter()
            .any(|skip| skip.reason == SkipReason::TestCode),
        "and nothing is passed over as test code, because test code is what this \
         run is about: {:?}",
        found.skips
    );
}

#[test]
fn asking_the_tests_leaves_what_a_cfg_gates_alone() {
    let source = "\
#[cfg(feature = \"extra\")]
pub fn extra(a: i32) -> i32 {
    a + 1
}
";
    let selection = Selection::tier(&REGISTRY, Tier::All).asking_the_tests();
    let found = discover_file("src/lib.rs", source.as_bytes(), &selection).expect("it parses");
    assert!(
        found
            .skips
            .iter()
            .any(|skip| skip.reason == SkipReason::CfgAttribute),
        "asking the tests is not a licence to mutate what the compiler removes: a \
         mutation in a branch that is not compiled never dies and never lives: {:?}",
        found.skips
    );
}
