// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Decline, Declines, Reading, Unbelieved};

fn names(tests: &[&str]) -> Vec<String> {
    tests.iter().map(|one| (*one).to_owned()).collect()
}

fn decline(test: &str, why: &str) -> Decline {
    Decline {
        test: test.to_owned(),
        why: why.to_owned(),
    }
}

fn unbelieved(because: Unbelieved) -> Declines {
    Declines::Unbelieved { because }
}

#[test]
fn a_silent_notice_declines_nothing_whatever_the_reading() {
    for reading in Reading::ALL {
        for bytes in [&b""[..], b"\n", b"\r\n", b"\n\n"] {
            let read = Declines::read(bytes, reading, &names(&["a"]));
            assert_eq!(read, Declines::none(), "{reading:?} {bytes:?}");
            assert!(read.is_silent(), "{reading:?} {bytes:?}");
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
    assert_eq!(
        read.believed(),
        [
            decline("tests::a", "no network"),
            decline("tests::b", "cannot share blocks")
        ]
    );
    assert!(!read.is_silent(), "a notice that declines says something");
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
    let several = Declines::read(
        b"\tno network\ntests::b\tcannot add\n",
        Reading::Whole,
        &names(&["tests::a", "tests::b"]),
    );
    assert_eq!(
        several,
        Declines::Read {
            declined: vec![decline("tests::b", "cannot add")],
            quoted: vec!["no network".to_owned()],
        },
        "with several, the line cannot say which test declined, so it sets none aside, and the \
         lines after it are read as well"
    );
    let quoted_alone = Declines::Read {
        declined: Vec::new(),
        quoted: vec!["no network".to_owned()],
    };
    assert!(!quoted_alone.is_silent(), "a quoted line is something said");
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
            unbelieved(because.clone()),
            "{bytes:?} under {reading:?}"
        );
        assert_eq!(read.believed(), [], "nothing is believed of it");
        assert!(!read.is_silent(), "a notice nobody believes is not silence");
    }
}

#[test]
fn every_refusal_says_what_was_wrong_in_its_own_words() {
    for (because, said) in [
        (
            Unbelieved::ReadingNotWhole,
            "a test declined in a process whose tests were not read whole, so the decline names \
             no test that is known to have run",
        ),
        (
            Unbelieved::Malformed {
                line: "a b".to_owned(),
            },
            "the decline notice holds \"a b\", which is not a test's name, a tab, and why it \
             declined; a line appended in more than one write can have another test's land \
             inside it, so each line is appended in one",
        ),
        (
            Unbelieved::NotATest {
                test: "z".to_owned(),
            },
            "the decline notice names \"z\", which is not a test this process passed",
        ),
        (
            Unbelieved::Contradicted {
                test: "a".to_owned(),
            },
            "the decline notice gives \"a\" two different reasons",
        ),
        (Unbelieved::NotText, "the decline notice is not text"),
        (
            Unbelieved::Unreadable {
                message: "denied".to_owned(),
            },
            "the decline notice could not be read: denied",
        ),
    ] {
        assert_eq!(because.said(), said, "{because:?}");
    }
}

#[test]
fn a_notice_nobody_wrote_declines_nothing_and_one_nobody_can_read_is_not_believed() {
    let passed = names(&["tests::a"]);
    assert_eq!(
        Declines::answered(None, Reading::Whole, &passed),
        Declines::none(),
        "a process the engine named no notice for declined nothing"
    );
    assert_eq!(
        Declines::answered(
            Some(Err(std::io::Error::from(std::io::ErrorKind::NotFound))),
            Reading::Whole,
            &passed
        ),
        Declines::none(),
        "a process that wrote no notice declined nothing"
    );
    assert_eq!(
        Declines::answered(
            Some(Err(std::io::Error::other("denied"))),
            Reading::Whole,
            &passed
        ),
        unbelieved(Unbelieved::Unreadable {
            message: "denied".to_owned()
        })
    );
    assert_eq!(
        Declines::answered(
            Some(Ok(b"tests::a\tno network\n".to_vec())),
            Reading::Whole,
            &passed
        ),
        Declines::read(b"tests::a\tno network\n", Reading::Whole, &passed),
        "a notice that was read is read as its bytes say"
    );
}

#[test]
fn a_planted_reading_that_believes_a_short_account_is_caught() {
    let planted = |bytes: &[u8], passed: &[String]| Declines::read(bytes, Reading::Whole, passed);
    let passed = names(&["tests::a"]);
    assert_ne!(
        planted(b"tests::a\tno network\n", &passed),
        Declines::read(b"tests::a\tno network\n", Reading::Short, &passed),
        "the rule did not catch a notice believed of tests that were not read whole"
    );
}
