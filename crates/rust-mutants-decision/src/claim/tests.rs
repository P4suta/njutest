// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

extern crate std;

use std::vec::Vec;

use super::{Edit, Located, Wanted, located, moved, names_item, narrows};

const WANTED: Wanted<'static> = Wanted {
    path: "src/lib.rs",
    item: "Clamp::low",
    rule: "lt-to-le",
    original: "<",
};

const EDIT: Edit<'static> = Edit {
    path: "src/lib.rs",
    rule: "lt-to-le",
    original: b"<",
    item: Some("shapes::Clamp::low"),
};

#[test]
fn a_locator_names_an_edit_only_where_every_field_it_gives_is_the_edits() {
    assert!(WANTED.names(&EDIT), "every field agrees");
    for (differing, edit) in [
        (
            "another file",
            Edit {
                path: "src/main.rs",
                ..EDIT
            },
        ),
        (
            "another rule",
            Edit {
                rule: "le-to-lt",
                ..EDIT
            },
        ),
        (
            "other bytes",
            Edit {
                original: b"<=",
                ..EDIT
            },
        ),
        (
            "another item",
            Edit {
                item: Some("shapes::Clamp::high"),
                ..EDIT
            },
        ),
        ("no item", Edit { item: None, ..EDIT }),
    ] {
        assert!(
            !WANTED.names(&edit),
            "a locator does not name an edit in {differing}"
        );
    }
    let any_text = Wanted {
        original: "",
        ..WANTED
    };
    assert!(
        any_text.names(&Edit {
            original: b"<=",
            ..EDIT
        }),
        "a locator that gives no text names the edit whatever it replaces"
    );
}

#[test]
fn an_item_is_named_by_itself_or_by_its_last_whole_segments() {
    for (item, wanted, named) in [
        ("clamp", "clamp", true),
        ("shapes::clamp", "clamp", true),
        ("a::shapes::Clamp::low", "Clamp::low", true),
        ("a::shapes::Clamp::low", "a::shapes::Clamp::low", true),
        ("unclamp", "clamp", false),
        ("a::unclamp", "clamp", false),
        ("clamp", "shapes::clamp", false),
        ("clamp::inner", "clamp", false),
        ("a:clamp", "clamp", false),
    ] {
        assert_eq!(
            names_item(item, wanted),
            named,
            "{item:?} named by {wanted:?}"
        );
    }
}

#[test]
fn only_a_line_given_among_several_narrows_what_a_locator_names() {
    assert_eq!(
        [0, 1, 2, 3].map(narrows),
        [false, false, true, true],
        "a line narrows only a choice, so a line one mutation left is a move rather than nothing"
    );
}

fn by_the_rules(held: usize, count: Option<u32>) -> Located {
    match count {
        Some(wanted) if held > 0 && usize::try_from(wanted) == Ok(held) => Located::Named,
        Some(wanted) if held > 0 => Located::Counted { wanted },
        None if held == 1 => Located::Named,
        None if held > 1 => Located::Several,
        Some(_) | None => Located::Nothing,
    }
}

#[test]
fn every_number_of_matches_comes_to_what_the_rules_say_under_every_stated_count() {
    let mut disagreeing = Vec::new();
    for held in 0..=4_usize {
        for count in [None, Some(0), Some(1), Some(2), Some(3), Some(u32::MAX)] {
            let said = located(held, count);
            if said != by_the_rules(held, count) {
                disagreeing.push((held, count, said));
            }
        }
    }
    assert!(
        disagreeing.is_empty(),
        "{} placements disagree with the rules, the first {:?}",
        disagreeing.len(),
        disagreeing.first()
    );
}

#[test]
fn a_claim_moved_only_where_it_holds_a_line_the_first_it_names_is_not_on() {
    assert_eq!(moved(Some(12), Some(15)), Some((12, 15)));
    assert_eq!(moved(Some(12), Some(12)), None, "the line is where it is");
    assert_eq!(
        moved(None, Some(15)),
        None,
        "a claim with no line holds none"
    );
    assert_eq!(moved(Some(12), None), None, "nothing named is on no line");
}

#[test]
fn a_planted_count_that_takes_any_number_is_caught_by_the_rules() {
    let planted = |held: usize, count: Option<u32>| match (held, count) {
        (0, _) => Located::Nothing,
        (_, Some(_)) | (1, None) => Located::Named,
        (_, None) => Located::Several,
    };
    assert!(
        (0..=4_usize).any(|held| {
            [None, Some(1), Some(3)]
                .into_iter()
                .any(|count| planted(held, count) != by_the_rules(held, count))
        }),
        "the rules did not catch a claim of three mutations that names two"
    );
}
