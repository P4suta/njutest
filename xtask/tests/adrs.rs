// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A decision record has one number, carries it in its heading, is listed once in the book under it, and is named only as it is.

use xtask::adrs::{Record, RecordError, dangling, numbered, records, summary};

/// A record file named `file` whose heading carries `number`.
fn record(file: &str, number: u16) -> (String, String) {
    (
        file.to_owned(),
        format!("<!--\nlicence\n-->\n\n# {number:04} — A decision\n\nWhy.\n"),
    )
}

/// A name no record in these tests has, spelled at run time so that the tree's own gate does not read this file as naming it.
fn gone(prefix: &str) -> String {
    format!("{prefix}{:04}-gone.md", 2)
}

fn one() -> Vec<Record> {
    vec![Record {
        number: 1,
        file: "0001-seam-policy.md".to_owned(),
    }]
}

#[test]
fn a_record_is_named_by_four_digits_and_a_lowercase_slug() {
    assert_eq!(numbered("0031-a-knob-is-one-control.md"), Some(31));
    for refused in [
        "31-a-knob.md",
        "0031_a-knob.md",
        "0031-.md",
        "0031-A-Knob.md",
        "0031-a-knob.txt",
        "003a-a-knob.md",
        "README.md",
    ] {
        assert_eq!(numbered(refused), None, "{refused} is no record's name");
    }
}

#[test]
fn two_records_of_one_number_are_refused_by_both_names() {
    let files = [
        record("0026-a-bound-measures-quiet-not-duration.md", 26),
        record("0026-an-item-is-entered-where-its-body-starts.md", 26),
    ];
    assert_eq!(
        records(&files),
        Err(RecordError::Duplicate {
            number: 26,
            first: "0026-a-bound-measures-quiet-not-duration.md".to_owned(),
            second: "0026-an-item-is-entered-where-its-body-starts.md".to_owned(),
        }),
        "a number names one decision, and a reader following it to one of two lands on a coin \
         toss"
    );
}

#[test]
fn a_heading_that_does_not_carry_its_own_number_is_refused() {
    let files = [record("0027-a-change.md", 26)];
    assert!(
        matches!(
            records(&files),
            Err(RecordError::Heading { ref file, number: 27, .. }) if file == "0027-a-change.md"
        ),
        "a record renumbered by its file name alone says one number and is called by another: \
         {:?}",
        records(&files)
    );
    let unnamed = [("notes.md".to_owned(), "# notes".to_owned())];
    assert!(
        matches!(records(&unnamed), Err(RecordError::Unnamed { ref file }) if file == "notes.md"),
        "the directory holds decision records only: {:?}",
        records(&unnamed)
    );
}

#[test]
fn the_book_lists_every_record_once_under_its_own_number() {
    let listed = &format!("- [0001 Seam policy]{}adr/0001-seam-policy.md)\n", "(");
    assert_eq!(summary(listed, &one()), Ok(()));
    let missing = summary("# Summary\n", &one());
    assert!(
        matches!(missing, Err(RecordError::Summary { ref detail }) if detail.contains("does not list adr/0001-seam-policy.md")),
        "{missing:?}"
    );
    let twice = summary(&format!("{listed}{listed}"), &one());
    assert!(
        matches!(twice, Err(RecordError::Summary { ref detail }) if detail.contains("twice")),
        "{twice:?}"
    );
    let misnumbered = summary(
        &format!("- [0002 Seam policy]{}adr/0001-seam-policy.md)\n", "("),
        &one(),
    );
    assert!(
        matches!(misnumbered, Err(RecordError::Summary { ref detail }) if detail.contains("where it is decision 0001")),
        "{misnumbered:?}"
    );
    let nowhere = summary(
        &format!("{listed}- [0002 Gone]({})\n", gone("adr/")),
        &one(),
    );
    assert!(
        matches!(nowhere, Err(RecordError::Summary { ref detail }) if detail.contains("no decision record has that name")),
        "{nowhere:?}"
    );
}

#[test]
fn a_name_of_a_record_nobody_wrote_is_found_wherever_it_is() {
    let at_the_end_of_a_sentence = format!("See {}.", gone("docs/adr/"));
    let found = dangling("docs/limitations.md", &at_the_end_of_a_sentence, &one());
    assert_eq!(
        found,
        vec![RecordError::Dangling {
            page: "docs/limitations.md".to_owned(),
            link: gone("docs/adr/"),
        }],
        "the period that ends a sentence is not part of the name it ends on"
    );
    let beside = format!("as [ADR 0002]({}#decision) says", gone(""));
    assert_eq!(
        dangling("docs/adr/0001-seam-policy.md", &beside, &one()).len(),
        1,
        "a record names another beside it without `adr/`, and an anchor does not change which"
    );
    assert!(
        dangling("docs/limitations.md", &beside, &one()).is_empty(),
        "outside the records a bare NNNN-slug.md is not a record's name"
    );
    assert!(
        dangling(
            "CLAUDE.md",
            "read `docs/adr/0001-seam-policy.md` first",
            &one()
        )
        .is_empty(),
        "a name a record has is no dangling name"
    );
}

#[test]
fn the_tree_holds_together() {
    let root = xtask::gates::workspace_root();
    let said = xtask::gates::adrs(&root);
    assert!(
        said.as_ref().is_ok_and(|line| line.starts_with("adrs: ")),
        "{said:?}"
    );
}
