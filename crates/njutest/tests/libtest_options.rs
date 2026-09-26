// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! This reading of libtest's harness arguments holds to the one contract both readings are written against, so the runner's and the audit's cannot drift apart.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

/// The options libtest reads, as schema/libtest-harness-options.json states them.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    description: String,
    valued: Vec<String>,
    flags: Vec<String>,
}

fn spec() -> Spec {
    njutest_devkit::strictjson::decode_str(include_str!(
        "../../../schema/libtest-harness-options.json"
    ))
    .expect("the contract is JSON")
}

fn one_thread(words: &[&str]) -> bool {
    let args: Vec<String> = words.iter().map(|one| (*one).to_owned()).collect();
    njutest::concurrency::proof::threads_of(&args) == njutest::concurrency::proof::Threads::One
}

fn table(name: &str) -> Vec<String> {
    njutest_devkit::rust_source::strings_listed(include_str!("../src/concurrency/proof.rs"), name)
        .expect("the source writes the table as an array of string literals")
}

#[test]
fn the_tables_written_in_the_source_are_the_contracts() {
    let spec = spec();
    assert!(!spec.description.is_empty());
    assert_eq!(table("LIBTEST_VALUED"), spec.valued);
    assert_eq!(table("LIBTEST_FLAGS"), spec.flags);
}

#[test]
fn every_option_of_the_contract_is_read_as_the_contract_says() {
    let spec = spec();
    for valued in spec.valued.iter().filter(|one| *one != "--test-threads") {
        assert!(
            !one_thread(&[valued, "--test-threads=1"]),
            "{valued} takes the next word as its value"
        );
        assert!(
            one_thread(&[valued, "value", "--test-threads=1"]),
            "{valued} takes one word and no more"
        );
    }
    for flag in &spec.flags {
        assert!(
            one_thread(&[flag, "--test-threads=1"]),
            "{flag} takes no value"
        );
    }
    assert!(!one_thread(&["--not-an-option", "--test-threads=1"]));
    assert!(!one_thread(&["--", "--test-threads=1"]));
}
