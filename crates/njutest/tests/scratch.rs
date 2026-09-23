// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where a run writes, and who it belongs to.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::disallowed_methods,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::fs;

use jiff::Timestamp;
use njutest::scratch::{DIR_PREFIX, MARKER_SCHEMA, Scratch};
use rust_mutants::id::RunId;
use rust_mutants::tempowner;

fn run_id(value: &str) -> RunId {
    RunId::try_from(value).expect("a canonical writable run id")
}

/// The real moment, because the sweep judges an unowned directory by its age on the filesystem: a made-up "now" months away from the file times would call every directory a leftover.
fn now() -> Timestamp {
    Timestamp::now()
}

#[test]
fn a_scratch_is_named_for_its_run_and_holds_the_places_a_run_writes() {
    let parent = tempfile::tempdir().expect("a temporary root");
    let scratch =
        Scratch::create(parent.path(), &run_id("20260905t081500z-abcdef"), now()).expect("scratch");

    assert_eq!(
        scratch
            .dir()
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .expect("a name"),
        format!("{DIR_PREFIX}20260905t081500z-abcdef")
    );
    assert!(scratch.build_dir().is_dir(), "the isolated build directory");
    assert!(scratch.profiles_dir().is_dir(), "coverage profiles");
    assert!(scratch.output_dir().is_dir(), "preserved command output");
    assert!(scratch.is_claimed(), "the lock is held for the whole run");
}

#[test]
fn the_marker_names_the_runner_so_a_sweep_knows_who_left_it() {
    let parent = tempfile::tempdir().expect("a temporary root");
    let started = now();
    let scratch = Scratch::create(parent.path(), &run_id("run"), started).expect("scratch");

    let marker = tempowner::read_marker(scratch.dir()).expect("a marker");
    assert_eq!(MARKER_SCHEMA, "njutest-temp-owner-v1");
    assert_eq!(marker.schema, MARKER_SCHEMA);
    assert_eq!(marker.pid, std::process::id());
    assert_eq!(marker.started, started);
    assert!(!marker.kept);
    assert!(tempowner::lock_path(scratch.dir()).is_file());
}

#[test]
fn a_round_directory_is_made_under_the_scratch_and_nowhere_else() {
    let parent = tempfile::tempdir().expect("a temporary root");
    let scratch = Scratch::create(parent.path(), &run_id("run"), now()).expect("scratch");

    let round = scratch.round_dir("baseline-1").expect("a round directory");
    assert!(round.is_dir());
    assert!(round.starts_with(scratch.dir()), "{}", round.display());
    assert_eq!(
        scratch.round_dir("baseline-1").expect("again"),
        round,
        "asking twice is asking for the same place"
    );
}

#[test]
fn closing_a_scratch_removes_everything_it_made() {
    let parent = tempfile::tempdir().expect("a temporary root");
    let scratch = Scratch::create(parent.path(), &run_id("run"), now()).expect("scratch");
    let dir = scratch.dir().to_path_buf();
    fs::write(scratch.build_dir().join("artifact"), b"x").expect("a file in it");

    assert_eq!(
        scratch.close().expect("closed"),
        Vec::<std::path::PathBuf>::new()
    );
    assert!(!dir.exists(), "{}", dir.display());
}

#[test]
fn dropping_a_scratch_removes_everything_it_made() {
    let parent = tempfile::tempdir().expect("a temporary root");
    let dir = {
        let scratch = Scratch::create(parent.path(), &run_id("run"), now()).expect("scratch");
        let dir = scratch.dir().to_path_buf();
        fs::write(scratch.build_dir().join("artifact"), b"x").expect("a file in it");
        dir
    };

    assert!(
        !dir.exists(),
        "an early return relies on RAII to remove {}",
        dir.display()
    );
}

#[test]
fn a_kept_scratch_survives_and_says_it_was_kept_on_purpose() {
    let parent = tempfile::tempdir().expect("a temporary root");
    let scratch = Scratch::create(parent.path(), &run_id("run"), now()).expect("scratch");
    let dir = scratch.dir().to_path_buf();

    assert_eq!(scratch.keep().expect("kept"), vec![dir.clone()]);
    assert!(dir.is_dir(), "a keep is a keep");
    let marker = tempowner::read_marker(&dir).expect("a marker");
    assert!(marker.kept, "so a later sweep leaves it alone");
}

#[test]
fn creating_a_scratch_collects_what_an_earlier_run_abandoned() {
    let parent = tempfile::tempdir().expect("a temporary root");
    let abandoned = parent.path().join(format!("{DIR_PREFIX}earlier"));
    fs::create_dir_all(abandoned.join("build")).expect("the directory");
    tempowner::claim_as(&abandoned, now(), MARKER_SCHEMA)
        .expect("claimed")
        .release()
        .expect("and then its holder went away");
    assert!(abandoned.is_dir());

    let scratch = Scratch::create(parent.path(), &run_id("later"), now()).expect("scratch");
    assert_eq!(scratch.swept().removed.len(), 1, "{:?}", scratch.swept());
    assert!(!abandoned.exists(), "{}", abandoned.display());
}

#[test]
fn a_sweep_leaves_alone_what_a_run_kept_on_purpose() {
    let parent = tempfile::tempdir().expect("a temporary root");
    let kept = Scratch::create(parent.path(), &run_id("earlier"), now())
        .expect("scratch")
        .keep()
        .expect("kept");

    let scratch = Scratch::create(parent.path(), &run_id("later"), now()).expect("scratch");
    assert_eq!(scratch.swept().kept, 1);
    assert!(kept[0].is_dir(), "{}", kept[0].display());
}

#[test]
fn a_directory_this_run_cannot_claim_costs_the_claim_and_not_the_run() {
    let parent = tempfile::tempdir().expect("a temporary root");
    let dir = parent.path().join(format!("{DIR_PREFIX}run"));
    fs::create_dir_all(&dir).expect("the directory");
    let held = tempowner::acquire(&tempowner::lock_path(&dir))
        .expect("the lock file opens")
        .expect("nobody else holds it");

    let scratch =
        Scratch::create(parent.path(), &run_id("run"), now()).expect("the run still runs");
    assert!(!scratch.is_claimed(), "somebody else holds the lock");
    assert!(
        scratch.build_dir().is_dir(),
        "and the run still has its places"
    );
    drop(held);
}
