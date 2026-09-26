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
