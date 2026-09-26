// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A test's decline is believed only where the process it names ran whole, and only as the baseline made it (ADR 0043).

use rust_mutants::decline::{Decline, Declines, Held, Unbelieved, held};
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
fn a_silent_notice_declines_nothing_whatever_the_reading() {
    for reading in Reading::ALL {
        for bytes in [&b""[..], b"\n", b"\r\n"] {
            let read = Declines::read(bytes, reading, &names(&["a"]));
            assert!(read.is_silent(), "{reading:?} {bytes:?}: {read:?}");
        }
    }
}

#[test]
fn each_line_names_a_test_the_process_passed_and_its_words() {
    let read = Declines::read(
        b"tests::b\tcannot share blocks\r\ntests::a\tno network\ntests::b\tcannot share blocks\n",
        Reading::Whole,
        &names(&["tests::a", "tests::b", "tests::c"]),
    );
    assert_eq!(
        read,
        Declines::Read {
            declined: vec![
                decline("tests::a", "no network"),
                decline("tests::b", "cannot share blocks"),
            ],
            quoted: Vec::new(),
        },
        "a repeated line is one decline, and the declines come in name order"
    );
}

#[test]
fn a_line_that_names_no_test_is_the_one_tests_or_is_only_quoted() {
    assert_eq!(
        Declines::read(b"\tno network\n", Reading::Whole, &names(&["tests::a"])),
        Declines::Read {
            declined: vec![decline("tests::a", "no network")],
            quoted: Vec::new(),
        },
        "a process that ran one test leaves no doubt which declined"
    );
    assert_eq!(
        Declines::read(
            b"\tno network\n",
            Reading::Whole,
            &names(&["tests::a", "tests::b"])
        ),
        Declines::Read {
            declined: Vec::new(),
            quoted: vec!["no network".to_owned()],
        },
        "with several, the line cannot say which test declined, so it sets none aside"
    );
}

#[test]
fn a_notice_the_engine_cannot_hold_to_the_tests_that_ran_is_not_believed() {
    let passed = names(&["tests::a", "tests::b"]);
    for (bytes, reading, because) in [
        (
            &b"tests::a\tno network\n"[..],
            Reading::Short,
            Unbelieved::ReadingNotWhole,
        ),
        (
            b"tests::a\tno network\n",
            Reading::Unspoken,
            Unbelieved::ReadingNotWhole,
        ),
        (
            b"tests::a no network\n",
            Reading::Whole,
            Unbelieved::Malformed {
                line: "tests::a no network".to_owned(),
            },
        ),
        (
            b"tests::z\tno network\n",
            Reading::Whole,
            Unbelieved::NotATest {
                test: "tests::z".to_owned(),
            },
        ),
        (
            b"tests::a\tno network\ntests::a\tno disk\n",
            Reading::Whole,
            Unbelieved::Contradicted {
                test: "tests::a".to_owned(),
            },
        ),
        (b"tests::a\t\xff\n", Reading::Whole, Unbelieved::NotText),
    ] {
        let read = Declines::read(bytes, reading, &passed);
        assert_eq!(
            read,
            Declines::Unbelieved {
                because: because.clone()
            },
            "{bytes:?} under {reading:?}"
        );
        assert!(
            !because.said().trim().is_empty(),
            "every refusal says why: {because:?}"
        );
    }
}

#[test]
fn only_a_decline_the_baseline_made_in_the_same_words_is_set_aside() {
    let baseline = [decline("tests::a", "cannot share blocks")];
    assert_eq!(
        held(&[decline("tests::a", "cannot share blocks")], &baseline),
        Held::SetAside(vec![decline("tests::a", "cannot share blocks")])
    );
    assert_eq!(held(&[], &baseline), Held::SetAside(Vec::new()));
    assert_eq!(
        held(&[decline("tests::b", "cannot add")], &baseline),
        Held::Detected {
            by: decline("tests::b", "cannot add")
        },
        "a test that measured in the baseline and declined under the mutation was changed by it"
    );
    assert_eq!(
        held(&[decline("tests::a", "cannot share either")], &baseline),
        Held::Detected {
            by: decline("tests::a", "cannot share either")
        },
        "other words are another decline"
    );
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
        Declines::of(Some(&unreadable), Reading::Whole, &passed),
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
