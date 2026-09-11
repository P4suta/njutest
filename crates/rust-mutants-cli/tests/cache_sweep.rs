// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What `cache` says is on the disk, and what a sweep takes back.
//!
//! Every line here is a number a person reads before deciding to remove
//! something, so the two failures that matter are a sweep that removed what it
//! only said it would, and one that said it removed what is still there. Both
//! are the same line read two ways, so the tests read the disk afterwards
//! rather than the line alone.

#![expect(
    clippy::expect_used,
    reason = "the helpers that arrange a temporary directory are not themselves tests: one that \
              could not be made leaves nothing to sweep"
)]
#![expect(
    clippy::panic,
    reason = "the helper that finds one line of a sweep is not itself a test: a sweep that lost \
              the line leaves nothing to read out of it"
)]

use std::ffi::OsString;
use std::path::Path;

use mjutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

/// What one command said, driven in this process.
struct Said {
    code: u8,
    out: String,
    err: String,
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: mjutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
}

fn asked(environment: &Environment, args: &[&str]) -> Said {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .map(OsString::from),
        environment,
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    Said {
        code,
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
    }
}

/// The marker a run writes into a directory it made, so a sweep can tell whose it is.
fn owned(
    directory: &Path,
    role: rust_mutants::tempowner::Role,
    kept: bool,
    keyed_to: Option<&Path>,
) {
    std::fs::create_dir_all(directory).expect("a directory a run made");
    std::fs::write(directory.join("something"), b"bytes").expect("with something in it");
    let marker = rust_mutants::tempowner::Marker {
        schema: rust_mutants::tempowner::SCHEMA.to_owned(),
        pid: std::process::id(),
        started: "2026-01-01T00:00:00Z".parse().expect("an instant"),
        kept,
        role,
        keyed_to: keyed_to.map(|tree| tree.display().to_string()),
    };
    std::fs::write(
        rust_mutants::tempowner::marker_path(directory),
        serde_json::to_vec(&marker).expect("a marker"),
    )
    .expect("the marker beside it");
}

/// A snapshot and a build cache an interrupted run left behind, each with a file in it.
fn abandoned(temp: &Path, tree: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let snapshot = temp.join(format!("{}abandoned", rust_mutants::snapshot::DIR_PREFIX));
    let cache = temp.join(format!(
        "{}abandoned",
        rust_mutants::workspace::TARGET_DIR_PREFIX
    ));
    owned(
        &snapshot,
        rust_mutants::tempowner::Role::Scratch,
        false,
        None,
    );
    owned(
        &cache,
        rust_mutants::tempowner::Role::Cache,
        false,
        Some(tree),
    );
    (snapshot, cache)
}

/// The byte count a sweep line carries.
fn bytes(line: &str) -> u64 {
    line.split(" bytes")
        .next()
        .and_then(|before| before.split_whitespace().last())
        .and_then(|number| number.parse().ok())
        .unwrap_or_else(|| panic!("a sweep says how many bytes went: {line}"))
}

/// The line of `text` that starts with `name`.
fn line<'a>(text: &'a str, name: &str) -> &'a str {
    text.lines()
        .find(|line| line.starts_with(name))
        .unwrap_or_else(|| panic!("a sweep says {name}:\n{text}"))
}

#[test]
fn saying_what_is_there_removes_nothing() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let (snapshot, cache) = abandoned(fixture.temp(), fixture.root());

    let said = asked(&environment, &["cache"]);
    assert_eq!(said.code, 0, "{}{}", said.out, said.err);
    assert!(
        snapshot.is_dir() && cache.is_dir(),
        "a person asks what is there before deciding, and an answer that removed it \
         first is an answer to a question nobody asked: {}",
        said.out
    );
    assert!(
        line(&said.out, "snapshots").contains("reclaimable")
            && line(&said.out, "caches").contains("reclaimable"),
        "so both lines say what could go rather than what went: {}",
        said.out
    );
    assert!(
        line(&said.out, "temp").contains(&fixture.temp().display().to_string()),
        "and name the directory they are about, because it is usually not the one a \
         person expected: {}",
        said.out
    );
}

#[test]
fn collecting_takes_the_snapshots_and_spares_the_build_caches() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let (snapshot, cache) = abandoned(fixture.temp(), fixture.root());

    let said = asked(&environment, &["cache", "--gc"]);
    assert_eq!(said.code, 0, "{}{}", said.out, said.err);
    assert!(
        !snapshot.exists(),
        "a snapshot nothing owns is a copy of a tree that was measured and will not be \
         again: {}",
        said.out
    );
    assert!(
        cache.is_dir(),
        "and a build cache keyed to a tree that is still there is spared, because \
         sparing it is what makes the next run fast and it is the whole reason `--all` \
         is a separate word: {}",
        said.out
    );
    assert!(
        line(&said.out, "snapshots").contains("1 removed")
            && line(&said.out, "caches").contains("0 removed"),
        "which is what the two lines say: {}",
        said.out
    );
    assert!(
        line(&said.out, "caches").contains("1 kept for the next run"),
        "and the cache that was spared is counted where a person deciding whether to \
         pass --all can see there is one to take: {}",
        said.out
    );
}

#[test]
fn collecting_everything_takes_the_build_caches_too() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let (snapshot, cache) = abandoned(fixture.temp(), fixture.root());

    let said = asked(&environment, &["cache", "--gc", "--all"]);
    assert_eq!(said.code, 0, "{}{}", said.out, said.err);
    assert!(
        !snapshot.exists() && !cache.exists(),
        "which is what a person means when the disk is full: {}",
        said.out
    );
    assert!(
        line(&said.out, "caches").contains("removed"),
        "and the line says removed rather than reclaimable, because reading it as a \
         plan when it was the act is how a person removes it twice: {}",
        said.out
    );
}

#[test]
fn everything_is_counted_in_bytes_as_well_as_in_directories() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let _left = abandoned(fixture.temp(), fixture.root());

    let said = asked(&environment, &["cache", "--gc", "--all"]);
    for name in ["snapshots", "caches"] {
        let counted = line(&said.out, name);
        assert!(
            counted.contains("1 removed"),
            "one directory went: {counted}"
        );
        assert!(
            bytes(counted) > 0,
            "and how much room it gave back is the number a person is deciding by, so a \
             directory with a file in it is not nothing: {counted}"
        );
    }
    assert!(
        line(&said.out, "failures").contains('0'),
        "and what could not be removed is counted rather than thrown away, because a \
         sweep that says removed and left it there is the one failure that matters: {}",
        said.out
    );
}

#[test]
fn what_a_run_was_asked_to_keep_is_listed_and_never_swept() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let reports = fixture
        .root()
        .join(rust_mutants_cli::config::DEFAULT_REPORTS_DIRECTORY);
    let preserved = fixture.temp().join(format!(
        "{}kept-on-purpose",
        rust_mutants::snapshot::DIR_PREFIX
    ));
    owned(
        &preserved,
        rust_mutants::tempowner::Role::Scratch,
        true,
        None,
    );
    rust_mutants_cli::kept::Ledger::record(
        &reports,
        "20260101T000000000Z",
        std::slice::from_ref(&preserved),
    )
    .expect("a ledger naming it");

    let said = asked(&environment, &["cache", "--gc", "--all"]);
    assert!(
        preserved.is_dir(),
        "a directory somebody asked for has the name a sweep looks at and is the one \
         thing it never takes: they asked for it in order to look at it: {}",
        said.out
    );
    assert!(
        line(&said.out, "snapshots").contains("1 preserved on purpose"),
        "and the sweep counted it rather than passing over a name it did not recognise, \
         which is how this reads as spared when nothing spared it: {}",
        said.out
    );
    assert!(
        said.out.contains("kept        1") && said.out.contains("20260101T000000000Z"),
        "and it is listed with the run that asked, because that is what a person is \
         looking for when they go back to it: {}",
        said.out
    );
    assert!(
        said.out.contains(&preserved.display().to_string()),
        "along with where it is: {}",
        said.out
    );
}

#[test]
fn collecting_what_was_kept_removes_it_and_says_how_many() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let reports = fixture
        .root()
        .join(rust_mutants_cli::config::DEFAULT_REPORTS_DIRECTORY);
    let preserved = fixture.temp().join(format!(
        "{}kept-on-purpose",
        rust_mutants::snapshot::DIR_PREFIX
    ));
    owned(
        &preserved,
        rust_mutants::tempowner::Role::Scratch,
        true,
        None,
    );
    rust_mutants_cli::kept::Ledger::record(
        &reports,
        "20260101T000000000Z",
        std::slice::from_ref(&preserved),
    )
    .expect("a ledger naming it");

    let said = asked(&environment, &["cache", "--gc", "--kept"]);
    assert_eq!(said.code, 0, "{}{}", said.out, said.err);
    assert!(
        !preserved.exists(),
        "--kept is the word that takes them, and it is a separate word because nothing \
         else does: {}",
        said.out
    );
    assert!(
        said.out.contains("kept        1 removed"),
        "and the count is said, because a ledger emptied without a number leaves a \
         person unsure whether it was already empty: {}",
        said.out
    );
    assert_eq!(
        rust_mutants_cli::kept::Ledger::read(&reports).kept.len(),
        0,
        "and the ledger is empty afterwards, or the next sweep reports directories \
         that are gone"
    );
}

#[test]
fn emptying_what_earlier_runs_established_says_how_much_was_in_it() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let store = rust_mutants::outcomes::Store::new(fixture.cache());
    store.put(
        &"a".repeat(64),
        &rust_mutants::outcomes::Record {
            schema: "rust-mutants/outcome-v1".to_owned(),
            mutant: "b".repeat(64),
            outcome: "killed".to_owned(),
            target: "fixture-simple/lib".to_owned(),
            tests_run: Some(3),
            failed_tests: vec!["adds".to_owned()],
            run_id: "20260101T000000000Z".to_owned(),
        },
    );

    let listed = asked(&environment, &["cache"]);
    assert!(
        line(&listed.out, "outcomes").contains("1 records"),
        "what earlier runs established is counted where a person is deciding what to \
         remove: {}",
        listed.out
    );

    let cleared = asked(&environment, &["cache", "--clear-outcomes"]);
    assert_eq!(cleared.code, 0, "{}{}", cleared.out, cleared.err);
    assert!(
        line(&cleared.out, "outcomes").contains("1 removed"),
        "and emptying it says how many went, because a store emptied silently is one a \
         person empties again: {}",
        cleared.out
    );
    assert_eq!(
        store.size().0,
        0,
        "and it is empty afterwards, or the next run reuses what was supposed to be gone"
    );
}

#[test]
fn emptying_the_store_touches_nothing_on_the_disk_a_run_would_reuse() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let (snapshot, cache) = abandoned(fixture.temp(), fixture.root());

    let said = asked(&environment, &["cache", "--clear-outcomes"]);
    assert_eq!(said.code, 0, "{}{}", said.out, said.err);
    assert!(
        snapshot.is_dir() && cache.is_dir(),
        "the store and the temporary directory are two things, and a person emptying \
         one has not asked about the other: {}",
        said.out
    );
}

#[test]
fn a_store_somewhere_else_is_the_one_that_is_counted() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let elsewhere = fixture.temp().join("another-cache");
    std::fs::create_dir_all(&elsewhere).expect("a cache directory somewhere else");

    let said = asked(
        &environment,
        &["cache", "--cache-dir", &elsewhere.display().to_string()],
    );
    assert!(
        line(&said.out, "outcomes").contains(&elsewhere.display().to_string()),
        "--cache-dir is where the store is, and a count of the one under the home \
         directory would have a person removing the wrong one: {}",
        said.out
    );
}

#[test]
fn what_measuring_established_is_counted_where_it_lives() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let directory = fixture
        .cache()
        .join(rust_mutants::reach::remembered::LAYOUT);

    let empty = asked(&environment, &["cache"]);
    assert!(
        line(&empty.out, "measurements").contains("none yet")
            && line(&empty.out, "measurements").contains(&directory.display().to_string()),
        "a directory that is not there yet is said as none yet with where it will be, \
         rather than as zero trees: a person looking for it wants the path: {}",
        empty.out
    );

    std::fs::create_dir_all(&directory).expect("where measurements are remembered");
    std::fs::write(directory.join("a.json"), b"{}").expect("a measurement");
    std::fs::write(directory.join("notes.txt"), b"not a measurement")
        .expect("and a file beside it");

    let counted = asked(&environment, &["cache"]);
    assert!(
        line(&counted.out, "measurements").contains("1 trees"),
        "and a file that is not a measurement is not counted as one: {}",
        counted.out
    );
    assert!(
        !line(&counted.out, "measurements").contains("0 bytes"),
        "and the bytes are the measurement's own: {}",
        counted.out
    );
}

#[test]
fn a_build_cache_keyed_to_a_tree_that_is_gone_is_swept_without_asking_for_everything() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let orphan = fixture.temp().join(format!(
        "{}orphan",
        rust_mutants::workspace::TARGET_DIR_PREFIX
    ));
    owned(
        &orphan,
        rust_mutants::tempowner::Role::Cache,
        false,
        Some(&fixture.temp().join("a-tree-that-was-removed")),
    );

    let said = asked(&environment, &["cache", "--gc"]);
    assert!(
        !orphan.exists(),
        "a cache is spared however old it is because a later run will look it up, and \
         one keyed to a tree nobody can name again is one no run will ever look up: \
         sparing it is how a temporary directory grows without bound: {}",
        said.out
    );
}

#[test]
fn a_directory_a_run_holds_is_counted_as_in_use_rather_than_removed() {
    let fixture = Fixture::copy("fixture-simple");
    let environment = environment(&fixture);
    let preserved = fixture
        .temp()
        .join(format!("{}on-purpose", rust_mutants::snapshot::DIR_PREFIX));
    owned(
        &preserved,
        rust_mutants::tempowner::Role::Scratch,
        true,
        None,
    );

    let said = asked(&environment, &["cache", "--gc", "--all"]);
    assert!(
        preserved.is_dir(),
        "a marker that says the directory was preserved on purpose is the run saying \
         somebody is going to look at it: {}",
        said.out
    );
    assert!(
        line(&said.out, "snapshots").contains("1 preserved on purpose"),
        "and it is counted where a person reading the line can see why the number of \
         removed ones is smaller than the number there are: {}",
        said.out
    );
}
