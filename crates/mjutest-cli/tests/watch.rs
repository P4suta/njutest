// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The watch loop: what makes it run a round, and what makes it stop.
//!
//! Every test here bounds how many times the loop may look before the watch
//! is cancelled. Without that, a rule broken in the loop stops the test from
//! terminating rather than making it fail, and a test that hangs when a rule
//! goes missing is not a test that holds the rule — it is one that has to be
//! killed by a timeout and reports nothing.

use std::cell::Cell;
use std::time::Duration;

use mjutest_cli::app::watch::{Seen, until};
use rust_mutants::runner::Cancel;

fn seen(files: &[(&str, u64)]) -> Seen {
    files
        .iter()
        .map(|(name, size)| ((*name).to_owned(), (None, *size)))
        .collect()
}

/// A look that counts itself and cancels the watch once it has been asked `most` times.
fn bounded<'a>(
    cancel: &'a Cancel,
    looks: &'a Cell<u64>,
    most: u64,
) -> impl FnMut() -> u64 + use<'a> {
    move || {
        looks.set(looks.get().saturating_add(1));
        if looks.get() >= most {
            cancel.cancel();
        }
        looks.get()
    }
}

#[test]
fn the_first_look_is_a_change_and_runs_a_round() {
    let cancel = Cancel::new();
    let rounds = Cell::new(0u32);
    let looks = Cell::new(0u64);
    let mut count = bounded(&cancel, &looks, 8);

    let code = until(
        &cancel,
        Duration::ZERO,
        || {
            let _at = count();
            Some(seen(&[("src/lib.rs", 10)]))
        },
        || {
            rounds.set(rounds.get().saturating_add(1));
            cancel.cancel();
            0
        },
    );

    assert_eq!(
        rounds.get(),
        1,
        "a watch verifies what is there before it waits for it to change: it was asked \
         {} times and never ran",
        looks.get()
    );
    assert_eq!(code, 0);
}

#[test]
fn a_tree_that_did_not_change_is_not_verified_again() {
    let cancel = Cancel::new();
    let rounds = Cell::new(0u32);
    let looks = Cell::new(0u64);
    let mut count = bounded(&cancel, &looks, 8);

    let code = until(
        &cancel,
        Duration::ZERO,
        || {
            let _at = count();
            Some(seen(&[("src/lib.rs", 10)]))
        },
        || {
            rounds.set(rounds.get().saturating_add(1));
            2
        },
    );

    assert_eq!(
        rounds.get(),
        1,
        "the tree was looked at {} times and changed once, so it was verified once",
        looks.get()
    );
    assert_eq!(
        code, 2,
        "and the round's own verdict is what the watch carries"
    );
}

#[test]
fn every_change_is_a_round_of_its_own() {
    let cancel = Cancel::new();
    let rounds = Cell::new(0u32);
    let looks = Cell::new(0u64);
    let mut count = bounded(&cancel, &looks, 20);

    let _code = until(
        &cancel,
        Duration::ZERO,
        || Some(seen(&[("src/lib.rs", count())])),
        || {
            rounds.set(rounds.get().saturating_add(1));
            if rounds.get() >= 3 {
                cancel.cancel();
            }
            0
        },
    );

    assert_eq!(
        rounds.get(),
        3,
        "a tree that keeps changing keeps being verified: the loop does not coalesce two \
         edits into one answer, because the second edit has not been answered for"
    );
}

#[test]
fn a_tree_that_could_not_be_read_waits_rather_than_verifying_what_it_did_not_see() {
    let cancel = Cancel::new();
    let rounds = Cell::new(0u32);
    let looks = Cell::new(0u64);
    let mut count = bounded(&cancel, &looks, 6);

    let _code = until(
        &cancel,
        Duration::ZERO,
        || {
            let _at = count();
            None
        },
        || {
            rounds.set(rounds.get().saturating_add(1));
            0
        },
    );

    assert_eq!(
        rounds.get(),
        0,
        "a directory that could not be walked is not a tree that changed: verifying on \
         it would put a round's report against a state nothing observed"
    );
}

#[test]
fn an_edit_that_lands_while_a_round_runs_gets_a_round_of_its_own() {
    let cancel = Cancel::new();
    let rounds = Cell::new(0u32);
    let size = Cell::new(10u64);
    let looks = Cell::new(0u64);
    let mut count = bounded(&cancel, &looks, 12);

    let _code = until(
        &cancel,
        Duration::ZERO,
        || {
            let _at = count();
            Some(seen(&[("src/lib.rs", size.get())]))
        },
        || {
            rounds.set(rounds.get().saturating_add(1));
            if rounds.get() == 1 {
                size.set(size.get().saturating_add(1));
            }
            if rounds.get() >= 2 {
                cancel.cancel();
            }
            0
        },
    );

    assert_eq!(
        rounds.get(),
        2,
        "the state a round answered for is the one read before it: an edit that lands \
         while it runs was not answered for, and reading the tree again afterwards \
         would fold that edit into an answer that never saw it"
    );
}
