// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The watch loop: what makes it run a round, and what makes it stop.

use std::cell::Cell;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use njutest_cli::app::watch::{POLL, Seen, look, until};
use njutest_cli::cli::{EXIT_ERROR, Environment};
use njutest_cli::testkit::watch_until_with_wait;
use rust_mutants::runner::Cancel;

/// An environment whose watch has already been stopped, so nothing it starts runs a round.
fn stopped(root: &Path) -> Environment {
    let cancel = Cancel::new();
    cancel.cancel();
    Environment {
        vars: Vec::new(),
        working_directory: root.to_owned(),
        temp_directory: root.to_owned(),
        program: std::path::PathBuf::from("this test never runs it"),
        cache_directory: root.to_owned(),
        cancel,
    }
}

fn watching(arguments: &[&str], environment: &Environment) -> (u8, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args = std::iter::once("njutest")
        .chain(arguments.iter().copied())
        .map(OsString::from);
    let code = njutest_cli::run_from(args, environment, &mut out, &mut err);
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

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
fn a_look_that_failed_after_one_that_did_not_is_still_not_a_tree_that_changed() {
    let cancel = Cancel::new();
    let rounds = Cell::new(0u32);
    let looks = Cell::new(0u64);

    let _code = until(
        &cancel,
        Duration::ZERO,
        || {
            looks.set(looks.get().saturating_add(1));
            if looks.get() >= 4 {
                cancel.cancel();
            }
            (looks.get() == 1).then(|| seen(&[("src/lib.rs", 10)]))
        },
        || {
            rounds.set(rounds.get().saturating_add(1));
            0
        },
    );

    assert_eq!(
        rounds.get(),
        1,
        "the tree was read once and then could not be read at all: a refusal differs \
         from what the last look saw, and taking that difference for an edit would run \
         a round against a state nothing observed"
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

#[test]
fn a_tree_that_did_not_change_is_waited_on_rather_than_given_up_on() {
    let cancel = Cancel::new();
    let rounds = Cell::new(0u32);
    let looks = Cell::new(0u64);

    let _code = until(
        &cancel,
        Duration::ZERO,
        || {
            looks.set(looks.get().saturating_add(1));
            if looks.get() >= 6 {
                cancel.cancel();
            }
            Some(seen(&[(
                "src/lib.rs",
                if looks.get() < 3 { 10 } else { 11 },
            )]))
        },
        || {
            rounds.set(rounds.get().saturating_add(1));
            0
        },
    );

    assert_eq!(
        rounds.get(),
        2,
        "a tree that has not changed yet is a reason to wait and not a reason to stop \
         watching: the edit that came after it is one a watch that gave up would never \
         see"
    );
}

#[test]
fn looking_reads_every_file_under_verification_and_none_of_what_a_run_writes() {
    let root = tempfile::tempdir().expect("a directory");
    std::fs::create_dir_all(root.path().join("src")).expect("mkdir");
    std::fs::create_dir_all(root.path().join("target")).expect("mkdir");
    std::fs::create_dir_all(root.path().join("reports")).expect("mkdir");
    std::fs::write(root.path().join("src/lib.rs"), "pub fn f() {}\n").expect("a source file");
    std::fs::write(root.path().join("Cargo.toml"), "[package]\n").expect("a manifest");
    std::fs::write(root.path().join("target/debug"), "x").expect("something a build wrote");
    std::fs::write(root.path().join("reports/latest.json"), "{}").expect("something a run wrote");

    let seen = look(root.path()).expect("a directory that can be walked");

    assert_eq!(
        seen.get("src/lib.rs").map(|(_when, held)| *held),
        Some(14),
        "the size a look records is the size the file is, because what it compares two \
         looks by is the number itself: {seen:?}"
    );
    let mut named: Vec<&String> = seen.keys().collect();
    named.sort();
    assert_eq!(
        named,
        vec!["Cargo.toml", "src/lib.rs"],
        "what a run writes is not what a watch waits for: a round that answered its own \
         report would start the next one, and the loop would never stop"
    );
}

#[test]
fn a_file_that_grows_is_a_file_that_changed() {
    let root = tempfile::tempdir().expect("a directory");
    std::fs::write(root.path().join("one.rs"), "fn f() {}\n").expect("a source file");
    let before = look(root.path()).expect("a walk");

    std::fs::write(root.path().join("one.rs"), "fn f() { g() }\n").expect("an edit");
    let after = look(root.path()).expect("a walk");

    assert_ne!(
        before, after,
        "a watch that could not tell these apart would sit still through every edit \
         that keeps a file's name"
    );
}

#[test]
fn a_directory_that_cannot_be_walked_is_not_a_tree_that_changed() {
    let gone = tempfile::tempdir().expect("a directory");
    let path = gone.path().join("never-made");

    assert!(
        look(&path).is_err(),
        "a path that is not there is a question this cannot answer, and answering it \
         with an empty tree would read as every file having been deleted"
    );
}

#[test]
fn the_loop_waits_between_looks_and_stops_waiting_once_it_is_cancelled() {
    let cancel = Cancel::new();
    let looks = Cell::new(0u64);
    let waits = Cell::new(0u64);

    let poll = Duration::from_millis(200);
    let code = watch_until_with_wait(
        &cancel,
        || {
            looks.set(looks.get().saturating_add(1));
            if looks.get() >= 3 {
                cancel.cancel();
            }
            Some(seen(&[("src/lib.rs", 10)]))
        },
        || 0,
        (poll, |duration| {
            assert_eq!(duration, poll, "the configured interval is the one waited");
            waits.set(waits.get().saturating_add(1));
        }),
    );

    assert_eq!(
        code, 0,
        "nothing ran after the first round, so nothing changed it"
    );
    assert_eq!(
        waits.get(),
        1,
        "the tree did not change between the first look and the second, so the loop \
         waited once; one that did not would spin a core while nothing was happening"
    );
}

#[test]
fn a_watch_cancelled_while_it_would_have_waited_does_not_wait() {
    let cancel = Cancel::new();
    let looks = Cell::new(0u64);
    let waits = Cell::new(0u64);
    let poll = Duration::from_millis(200);

    let code = watch_until_with_wait(
        &cancel,
        || {
            looks.set(looks.get().saturating_add(1));
            if looks.get() >= 2 {
                cancel.cancel();
            }
            Some(seen(&[("src/lib.rs", 10)]))
        },
        || 0,
        (poll, |_duration| waits.set(waits.get().saturating_add(1))),
    );

    assert_eq!(code, 0);
    assert_eq!(looks.get(), 2, "the tree was read twice and changed once");
    assert_eq!(
        waits.get(),
        0,
        "the only look that found nothing new is the one that was cancelled, so there \
         was never a moment to wait through: a watch that waits on its way out keeps a \
         person waiting for one it has already stopped, and waiting once at some other \
         moment is not the same rule"
    );
}

#[test]
fn a_watch_that_ran_nothing_says_what_a_run_that_found_nothing_says() {
    let cancel = Cancel::new();
    cancel.cancel();

    assert_eq!(
        until(&cancel, POLL, || Some(seen(&[])), || 3),
        0,
        "a watch cancelled before it looked has run nothing, and nothing is not a \
         failure: the code it carries is the last round's, and there was none"
    );
}

#[test]
fn a_watch_says_what_it_is_watching_and_how_often_it_will_ask() {
    let root = tempfile::tempdir().expect("a directory");
    let environment = stopped(root.path());

    let (code, said, _complained) = watching(&["watch", "--poll-ms", "70"], &environment);

    assert_eq!(code, 0, "a watch stopped before its first look ran nothing");
    assert!(
        said.contains(&format!("watching\t{}\tevery 70ms", root.path().display())),
        "a person who starts a watch is told which tree it is on and how often it will \
         ask, because a watch that says nothing is one they cannot tell from a hang: \
         {said}"
    );
}

#[test]
fn a_watch_told_nothing_about_how_often_asks_at_the_rate_the_default_names() {
    let root = tempfile::tempdir().expect("a directory");
    let environment = stopped(root.path());

    let (_code, said, _complained) = watching(&["watch"], &environment);

    assert!(
        said.contains(&format!("every {}ms", POLL.as_millis())),
        "the rate a watch asks at when nobody said is the one the default names, and \
         not some other number this line was written with: {said}"
    );
}

#[test]
fn a_watch_on_a_directory_it_was_given_watches_that_one_and_not_where_it_was_started() {
    let elsewhere = tempfile::tempdir().expect("a directory");
    let asked = tempfile::tempdir().expect("another directory");
    let environment = stopped(elsewhere.path());

    let (_code, said, _complained) = watching(
        &["watch", "--directory", &asked.path().display().to_string()],
        &environment,
    );

    assert!(
        said.contains(&asked.path().display().to_string())
            && !said.contains(&elsewhere.path().display().to_string()),
        "the tree a watch was pointed at is the one it watches: {said}"
    );
}

#[test]
fn a_configuration_that_cannot_be_read_ends_the_watch_before_it_looks() {
    let root = tempfile::tempdir().expect("a directory");
    std::fs::write(root.path().join(".njutest.toml"), "[project\n").expect("a configuration");
    let environment = stopped(root.path());

    let (code, said, complained) = watching(&["watch"], &environment);

    assert_eq!(
        code, EXIT_ERROR,
        "a watch that cannot read what it was told to verify has nothing to say about \
         the tree, and saying nothing while looking busy is the one answer it must not \
         give: {complained}"
    );
    assert!(
        complained.contains(".njutest.toml"),
        "and it names the file it could not read: {complained}"
    );
    assert!(
        !said.contains("watching"),
        "a watch that never started does not announce itself: {said}"
    );
}
