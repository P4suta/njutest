// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where each line of one file stands, as a run measured it: the mark a reader sees beside the code.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest::presentation::{MeasuredLine, Missing, Terminal};
use njutest::report::{Decided, MutantRecord, Report, RunKind};
use njutest::spec::{Section, Specification, Subject, specified};
use njutest::testkit::reports::{completed, row};

const IT: &str = "pkg/test/it";

/// `src/lib.rs` as the run measured it, one entry per line.
const MEASURED: [&str; 14] = [
    "pub fn sign(n: i32) -> bool {",
    "    n > 0",
    "        && n < 7",
    "}",
    "pub fn same(n: i32) -> i32 {",
    "    n + 0",
    "}",
    "pub fn slow(n: u64) -> u64 {",
    "    (0..n).sum()",
    "}",
    "pub fn log(n: i32) {",
    "    println!(",
    "        \"{n}\");",
    "}",
];

/// One change in `src/lib.rs`, at `line` of `item`.
fn at(
    index: u32,
    (item, line): (&str, u32),
    rule: (&str, &str, &str),
    outcome: Decided,
) -> MutantRecord {
    row(index, ("src/lib.rs", item, line), rule, outcome)
}

/// A one-build run that changed `MEASURED` in every way a line can stand, and one line of another file.
fn measured() -> Report {
    completed(
        "the-run",
        RunKind::Full,
        vec![(
            "default",
            vec![
                at(
                    0,
                    ("sign", 2),
                    ("gt-to-ge", ">", ">="),
                    Decided::Killed { by: IT.to_owned() },
                ),
                at(
                    1,
                    ("sign", 3),
                    ("and-to-or", "&&", "||"),
                    Decided::Killed { by: IT.to_owned() },
                ),
                at(2, ("sign", 3), ("lt-to-le", "<", "<="), Decided::Survived),
                at(
                    3,
                    ("same", 6),
                    ("add-to-sub", "+", "-"),
                    Decided::Equivalent,
                ),
                at(
                    4,
                    ("slow", 9),
                    ("return-default", "(0..n).sum()", "Default::default()"),
                    Decided::Waited { on: IT.to_owned() },
                ),
                at(
                    5,
                    ("log", 12),
                    ("delete-call-statement", "println!(\n        \"{n}\");", ""),
                    Decided::Survived,
                ),
                row(
                    6,
                    ("src/other.rs", "other", 2),
                    ("gt-to-ge", ">", ">="),
                    Decided::Survived,
                ),
            ],
        )],
    )
    .expect("rows a report can hold")
}

fn specification(report: &Report) -> Specification {
    specified(report, &Subject::Everything).expect("a run with changes")
}

/// `MEASURED` as the lines a run vouches for.
fn lines() -> Vec<(u32, MeasuredLine)> {
    (1..)
        .zip(MEASURED)
        .map(|(number, text)| (number, MeasuredLine::specimen(text)))
        .collect()
}

#[test]
fn a_line_stands_where_the_weakest_change_starting_on_it_stands() {
    let report = measured();
    let spec = specification(&report);
    let marked: Vec<(u32, Section, usize)> = spec
        .lines("src/lib.rs")
        .iter()
        .map(|line| (line.number(), line.section(), line.changes().count()))
        .collect();
    assert_eq!(
        marked,
        [
            (2, Section::Pinned, 1),
            (3, Section::Free, 2),
            (6, Section::Same, 1),
            (9, Section::Unsettled, 1),
            (12, Section::Free, 1),
        ],
        "a line one change of which nothing noticed is a line the tests leave free, whatever \
         else on it they pin; a change that spans two lines is marked where it starts and \
         nowhere else"
    );
}

#[test]
fn a_file_is_named_exactly_and_not_by_the_end_of_its_path() {
    let report = measured();
    let spec = specification(&report);
    assert_eq!(
        spec.lines("src/other.rs")
            .iter()
            .map(njutest::spec::Line::number)
            .collect::<Vec<_>>(),
        [2],
        "each file's lines are its own"
    );
    assert!(
        spec.lines("lib.rs").is_empty(),
        "a mark belongs to one file, and `lib.rs` is the end of several paths rather than one of \
         them"
    );
}

#[test]
fn the_page_marks_each_changed_line_beside_the_code_the_run_measured() {
    let report = measured();
    let spec = specification(&report);
    let page =
        njutest::presentation::guard::page(&spec, "src/lib.rs", &lines(), Terminal::plain(100));
    let drawn: Vec<&str> = page.lines().collect();
    for (number, mark) in [(2, "*"), (3, "o"), (6, "="), (9, "?"), (12, "o")] {
        let text = MEASURED[usize::try_from(number - 1).expect("a small line number")];
        assert!(
            drawn
                .iter()
                .any(|line| line.starts_with(&format!("{mark} {number:>2}  {text}"))),
            "line {number} carries `{mark}` beside the code the run measured:\n{page}"
        );
    }
    for number in [1, 4, 5, 7, 8, 10, 11, 13, 14] {
        let text = MEASURED[usize::try_from(number - 1).expect("a small line number")];
        assert!(
            drawn
                .iter()
                .any(|line| *line == format!("  {number:>2}  {text}")),
            "line {number} changed nothing and carries no mark:\n{page}"
        );
    }
    assert!(
        page.contains("* pinned")
            && page.contains("o left free")
            && page.contains("= the same program")
            && page.contains("? could not tell"),
        "the page says what each mark means: {page}"
    );
    assert!(
        page.starts_with("src/lib.rs as run the-run measured it"),
        "the page says which file and which run: {page}"
    );
}

#[test]
fn a_file_the_run_cannot_vouch_for_is_said_to_be_not_yet_asked_and_none_of_it_is_drawn() {
    let report = measured();
    let spec = specification(&report);
    let said = njutest::presentation::guard::unasked(
        &spec,
        "src/lib.rs",
        Missing::Edited,
        Terminal::plain(200),
    );
    assert!(
        said.starts_with(". src/lib.rs is not yet asked:")
            && said.contains("the file has changed since the run read it")
            && said.contains(
                "nothing run the-run found in this file is shown until a run measures it again"
            ),
        "a file whose bytes are not the run's gets one note, which says why, which run, and \
         what shows the marks again: {said}"
    );
    assert!(!said.contains("n > 0"), "and none of its code: {said}");
}
