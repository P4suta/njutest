// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run is, as one number. Two runs share an identity exactly when nothing that could change what the tests say has changed.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest_cli::config::Contract;
use njutest_cli::evidence::digest::{EVIDENCE_DOMAIN, Fields, Inputs, Mode, identity};

fn inputs() -> Inputs {
    Inputs {
        tree: "a".repeat(64),
        corpus: "b".repeat(64),
        dependencies: "c".repeat(64),
        toolchain: "rustc 1.98.0 (abc 2026-01-01)".to_owned(),
        platform: "x86_64-unknown-linux-gnu".to_owned(),
        environment: vec![
            ("CC".to_owned(), "clang".to_owned()),
            ("RUSTFLAGS".to_owned(), "-Copt-level=1".to_owned()),
        ],
        contract: Contract::StandardV1,
        configuration: "d".repeat(64),
        test_args: vec!["--nocapture".to_owned()],
        mode: Mode::Full,
        shard: None,
    }
}

#[test]
fn a_field_list_is_length_prefixed_so_two_different_lists_never_collide() {
    let mut one = Fields::new(EVIDENCE_DOMAIN);
    one.field("a", "bc").field("d", "");
    let mut other = Fields::new(EVIDENCE_DOMAIN);
    other.field("a", "b").field("d", "c");
    assert_ne!(
        one.finish(),
        other.finish(),
        "\"bc\" then \"\" is not \"b\" then \"c\""
    );

    let mut named = Fields::new(EVIDENCE_DOMAIN);
    named.field("tree", "x");
    let mut differently = Fields::new(EVIDENCE_DOMAIN);
    differently.field("corpus", "x");
    assert_ne!(
        named.finish(),
        differently.finish(),
        "the name of a field is part of what is hashed"
    );

    let mut domain = Fields::new("njutest-something-else-v1");
    domain.field("tree", "x");
    let mut other_domain = Fields::new(EVIDENCE_DOMAIN);
    other_domain.field("tree", "x");
    assert_ne!(domain.finish(), other_domain.finish());
}

#[test]
fn a_list_field_is_the_order_it_was_given() {
    let mut one = Fields::new(EVIDENCE_DOMAIN);
    one.list("packages", ["a", "b"]);
    let mut other = Fields::new(EVIDENCE_DOMAIN);
    other.list("packages", ["b", "a"]);
    assert_ne!(
        one.finish(),
        other.finish(),
        "a caller that wants order not to matter sorts before it hashes"
    );
    let mut empty = Fields::new(EVIDENCE_DOMAIN);
    empty.list("packages", Vec::<&str>::new());
    let absent = Fields::new(EVIDENCE_DOMAIN);
    assert_ne!(
        empty.finish(),
        absent.finish(),
        "a list that is there and empty is not a list that is not there"
    );
}

#[test]
fn every_digest_is_sixty_four_lowercase_hex_characters() {
    let value = identity(&inputs());
    assert_eq!(value.len(), 64, "{value}");
    assert!(
        value
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
        "{value}"
    );
}

#[test]
fn the_identity_is_a_function_of_everything_that_can_change_what_the_tests_say() {
    let base = identity(&inputs());
    assert_eq!(
        base,
        identity(&inputs()),
        "the same inputs, the same number"
    );

    let mut cases: Vec<(&str, Inputs)> = Vec::new();
    let mut one = inputs();
    one.tree = "e".repeat(64);
    cases.push(("tree", one));
    let mut one = inputs();
    one.corpus = "e".repeat(64);
    cases.push(("corpus", one));
    let mut one = inputs();
    one.dependencies = "e".repeat(64);
    cases.push(("dependencies", one));
    let mut one = inputs();
    one.toolchain = "rustc 1.99.0".to_owned();
    cases.push(("toolchain", one));
    let mut one = inputs();
    one.platform = "aarch64-apple-darwin".to_owned();
    cases.push(("platform", one));
    let mut one = inputs();
    one.environment[0].1 = "gcc".to_owned();
    cases.push(("an environment value", one));
    let mut one = inputs();
    one.environment.push(("CXX".to_owned(), "c++".to_owned()));
    cases.push(("an environment name", one));
    let mut one = inputs();
    one.contract = Contract::DeepV1;
    cases.push(("contract", one));
    let mut one = inputs();
    one.configuration = "e".repeat(64);
    cases.push(("configuration", one));
    let mut one = inputs();
    one.test_args.push("--test-threads=1".to_owned());
    cases.push(("the harness arguments", one));
    let mut one = inputs();
    one.mode = Mode::Scoped {
        packages: vec!["a".to_owned()],
    };
    cases.push(("mode", one));

    for (what, changed) in cases {
        assert_ne!(
            identity(&changed),
            base,
            "{what} did not change the identity"
        );
    }
}

#[test]
fn the_environment_is_read_in_a_fixed_order_however_the_process_listed_it() {
    let mut one = inputs();
    one.environment.reverse();
    assert_eq!(
        identity(&one),
        identity(&inputs()),
        "the process's ordering of its own environment is not a fact about the run"
    );
}

#[test]
fn each_mode_is_its_own_identity_and_says_what_it_looked_at() {
    let modes = [
        Mode::Full,
        Mode::Changed {
            base: "HEAD~1".to_owned(),
        },
        Mode::Changed {
            base: "main".to_owned(),
        },
        Mode::Scoped {
            packages: vec!["a".to_owned()],
        },
        Mode::Scoped {
            packages: vec!["a".to_owned(), "b".to_owned()],
        },
    ];
    let mut seen = std::collections::BTreeSet::new();
    for mode in modes {
        let mut one = inputs();
        one.mode = mode.clone();
        assert!(seen.insert(identity(&one)), "{mode:?} collided");
        assert!(!mode.name().is_empty());
    }
    assert_eq!(Mode::Full.name(), "full");
    assert_eq!(
        Mode::Scoped {
            packages: vec!["b".to_owned(), "a".to_owned()]
        }
        .name(),
        "scoped"
    );
}

#[test]
fn a_scoped_mode_reads_its_packages_in_a_fixed_order() {
    let mut one = inputs();
    one.mode = Mode::Scoped {
        packages: vec!["b".to_owned(), "a".to_owned()],
    };
    let mut other = inputs();
    other.mode = Mode::Scoped {
        packages: vec!["a".to_owned(), "b".to_owned()],
    };
    assert_eq!(
        identity(&one),
        identity(&other),
        "the order two packages were named in is not a fact about the run"
    );
}

#[test]
fn a_run_that_judged_one_part_of_a_catalog_is_not_the_run_that_judged_all_of_it() {
    let whole = identity(&inputs());
    let half = identity(&Inputs {
        shard: Some("1/2".to_owned()),
        ..inputs()
    });
    let other_half = identity(&Inputs {
        shard: Some("2/2".to_owned()),
        ..inputs()
    });

    assert_ne!(
        whole, half,
        "a run that judged half a catalog did not establish what a run that judged all \
         of it established, and an identity that says they are the same hands a part's \
         answer back as the whole's"
    );
    assert_ne!(half, other_half, "and one part is not the other");
    assert_eq!(EVIDENCE_DOMAIN, "njutest-evidence-v3");
}

#[test]
fn the_identity_is_about_the_variables_and_not_about_how_the_list_was_built() {
    let once = identity(&inputs());

    let twice = identity(&Inputs {
        environment: vec![
            ("RUSTFLAGS".to_owned(), "-Copt-level=1".to_owned()),
            ("CC".to_owned(), "clang".to_owned()),
            ("CC".to_owned(), "clang".to_owned()),
        ],
        ..inputs()
    });

    assert_eq!(
        once, twice,
        "the same variables with the same values, gathered twice or in another order, \
         are the same environment: a run whose identity depended on how its list was \
         assembled would refuse to reuse what an identical run established, and nothing \
         in either report would say why"
    );

    let different = identity(&Inputs {
        environment: vec![("CC".to_owned(), "gcc".to_owned())],
        ..inputs()
    });
    assert_ne!(
        once, different,
        "and a different environment is a different question"
    );
}

#[test]
fn the_identity_of_a_known_run_is_the_one_it_has_always_been() {
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/evidence-identity.golden");

    njutest_devkit::golden::golden(&golden, identity(&inputs()).as_bytes())
        .expect("the recorded identity");
}
