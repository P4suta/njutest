// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A test's decline is believed only where the process it names ran whole, and only as the baseline made it (ADR 0043).

use rust_mutants::decline::{Decline, Declines, Unbelieved};
use rust_mutants::execute::Reading;

fn names(tests: &[&str]) -> Vec<String> {
    tests.iter().map(|one| (*one).to_owned()).collect()
}

fn decline(test: &str, why: &str) -> Decline {
    Decline {
        test: test.to_owned(),
        why: why.to_owned(),
    }
}

#[test]
fn every_reading_decides_one_well_formed_notice_by_its_one_rule() {
    let passed = names(&["tests::a"]);
    for reading in Reading::ALL {
        let expected = match reading {
            Reading::Whole => Declines::Read {
                declined: vec![decline("tests::a", "no network")],
                quoted: Vec::new(),
            },
            Reading::Short | Reading::Unspoken => Declines::Unbelieved {
                because: Unbelieved::ReadingNotWhole,
            },
        };
        assert_eq!(
            Declines::read(b"tests::a\tno network\n", reading, &passed),
            expected,
            "{reading:?}: a notice is believed only where the tests it names are the harness's \
             own account, so a reading added to the set is a row the compiler asks for here"
        );
    }
}

#[test]
fn every_refusal_of_a_notice_is_one_a_notice_can_reach() {
    let passed = names(&["tests::a"]);
    let directory = tempfile::tempdir().expect("a directory to hold notices");
    let unreadable = directory.path().join("a-directory-not-a-notice");
    std::fs::create_dir_all(&unreadable).expect("a directory where the notice should be");
    let reached = [
        Declines::read(b"tests::a\tno network\n", Reading::Short, &passed),
        Declines::read(b"tests::a no network\n", Reading::Whole, &passed),
        Declines::read(b"tests::z\tno network\n", Reading::Whole, &passed),
        Declines::read(
            b"tests::a\tno network\ntests::a\tno disk\n",
            Reading::Whole,
            &passed,
        ),
        Declines::read(b"tests::a\t\xff\n", Reading::Whole, &passed),
        rust_mutants::decline::of(Some(&unreadable), Reading::Whole, &passed),
    ];
    let mut covered = std::collections::BTreeSet::new();
    for read in &reached {
        assert!(
            matches!(read, Declines::Unbelieved { .. }),
            "each notice here is one the engine refuses: {read:?}"
        );
        let Declines::Unbelieved { because } = read else {
            continue;
        };
        covered.insert(match because {
            Unbelieved::ReadingNotWhole => "reading-not-whole",
            Unbelieved::Malformed { .. } => "malformed",
            Unbelieved::NotATest { .. } => "not-a-test",
            Unbelieved::Contradicted { .. } => "contradicted",
            Unbelieved::NotText => "not-text",
            Unbelieved::Unreadable { .. } => "unreadable",
        });
    }
    assert_eq!(
        covered.len(),
        6,
        "every way a notice is refused is reached by a notice, and a refusal added to the set is \
         an arm the compiler asks for above and a count this holds: {covered:?}"
    );
}
