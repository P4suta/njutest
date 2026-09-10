// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands that read what a run left behind, driven in this process against one real run.
//!
//! Thirty-seven test files drive this program and every one of them starts a
//! process, which is what a person does and what the exit codes are about. A
//! measurement of what a crate's own tests reach does not follow a guard into
//! a child, so the command layer — `app/mod.rs` alone is 1680 lines — was
//! reached by nothing at all.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reads a document by the names the run it drove put there"
)]

use std::ffi::OsString;

use mjutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

/// What one command said, driven in this process.
#[derive(Debug)]
struct Said {
    code: u8,
    out: String,
    err: String,
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: std::env::vars_os().collect(),
        temp_directory: fixture.temp().to_path_buf(),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
}

fn ask(fixture: &Fixture, args: &[&str]) -> Said {
    let root = fixture.root().to_string_lossy().into_owned();
    rooted(fixture, args, &["--root", &root])
}

/// The same, for the commands that read no tree and so take no `--root`.
fn asked_alone(fixture: &Fixture, args: &[&str]) -> Said {
    rooted(fixture, args, &[])
}

fn rooted(fixture: &Fixture, args: &[&str], tail: &[&str]) -> Said {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .chain(tail.iter().copied())
            .map(OsString::from),
        &environment(fixture),
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

/// A copy of a fixture that one measurement has already run in.
fn measured() -> Fixture {
    let fixture = Fixture::copy("fixture-unicode");
    let ran = ask(&fixture, &["run", "--offline", "--locked"]);
    assert!(
        ran.code <= 1,
        "the run establishes something: {}{}",
        ran.out,
        ran.err
    );
    fixture
}

#[test]
fn a_report_is_read_back_in_every_shape_its_readers_take() {
    let fixture = measured();
    let lines = ask(&fixture, &["report"]);
    assert!(lines.code <= 1, "{}{}", lines.out, lines.err);
    assert!(
        !lines.out.trim().is_empty(),
        "reading a report back answers with what the run concluded: {}",
        lines.err
    );

    for (format, opens) in [
        ("json", "{"),
        ("stryker", "{"),
        ("sarif", "{"),
        ("html", "<!doctype html>"),
    ] {
        let projected = ask(&fixture, &["report", "--format", format]);
        assert!(
            projected.code <= 1 && projected.out.trim_start().starts_with(opens),
            "a projection is the shape its reader takes, from the first byte: {format} \
             answered {}\n{}",
            projected.code,
            projected.out.chars().take(80).collect::<String>()
        );
    }

    let nobody = ask(&fixture, &["report", "--run", "20200101T000000000Z"]);
    assert!(
        nobody.code != 0
            && nobody.err.contains("RM0007")
            && nobody.err.contains("20200101T000000000Z"),
        "while a run that is not there is named, with the code a person greps for, \
         rather than answered about: reading another run's verdict believing it is this \
         one's is the one mistake a report has to make impossible: {}{}",
        nobody.out,
        nobody.err
    );
}

#[test]
fn a_mutation_is_explained_and_a_name_that_is_not_one_is_refused() {
    let fixture = measured();
    let document = ask(&fixture, &["report", "--format", "json"]);
    let parsed: serde_json::Value =
        serde_json::from_str(&document.out).expect("the report is a document");
    let mutant = parsed["mutants"][0]["display_id"]
        .as_str()
        .expect("a mutation the run judged")
        .to_owned();

    let explained = ask(&fixture, &["explain", &mutant]);
    assert_eq!(explained.code, 0, "{}{}", explained.out, explained.err);
    assert!(
        explained.out.contains(&mutant),
        "what a run recorded about one mutation is shown against the name a person \
         typed: {}",
        explained.out
    );

    let nobody = ask(&fixture, &["explain", "ffffffffffffffff"]);
    assert!(
        nobody.code != 0 && nobody.err.contains("RM0007"),
        "and a mutation this catalog never held is refused rather than explained as one \
         it did: {}{}",
        nobody.out,
        nobody.err
    );
}

#[test]
fn what_the_engine_can_say_about_a_tree_without_measuring_it() {
    let fixture = Fixture::copy("fixture-unicode");

    let cataloged = ask(&fixture, &["list", "--offline", "--locked"]);
    assert_eq!(cataloged.code, 0, "{}{}", cataloged.out, cataloged.err);
    assert!(
        !cataloged.out.trim().is_empty(),
        "listing what would be mutated measures nothing and still has to say something, \
         or a person cannot tell an empty catalog from a command that did not run: {}",
        cataloged.err
    );

    let rules = asked_alone(&fixture, &["rules"]);
    assert_eq!(rules.code, 0, "{}{}", rules.out, rules.err);
    assert!(
        rules.out.lines().count() > 30,
        "the operators are what the engine is, and every one of them is on the list a \
         person reads before they trust a score: {}",
        rules.out
    );

    let skipped = ask(&fixture, &["why-skipped", "--offline", "--locked"]);
    assert!(
        skipped.code <= 1,
        "and what discovery passed over is asked the same way: {}{}",
        skipped.out,
        skipped.err
    );
}

#[test]
fn a_configuration_is_written_once_and_never_over_one_somebody_wrote() {
    let fixture = Fixture::copy("fixture-unicode");
    let path = fixture.root().join(rust_mutants_cli::config::FILE_NAME);
    let mine = "version = 1\n# mine\n";
    std::fs::write(&path, mine).expect("a configuration somebody wrote");

    let refused = ask(&fixture, &["init"]);
    assert_ne!(
        refused.code, 0,
        "a second init does not write over a configuration somebody has edited: the \
         expectations in it are the reviews of every survivor this project has looked \
         at: {}{}",
        refused.out, refused.err
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the configuration"),
        mine,
        "and leaves it exactly as it was"
    );

    std::fs::remove_file(&path).expect("a tree with no configuration in it");
    let written = ask(&fixture, &["init"]);
    assert_eq!(written.code, 0, "{}{}", written.out, written.err);
    let skeleton = std::fs::read_to_string(&path).expect("the skeleton");
    let parsed: Result<rust_mutants_cli::config::Config, _> = toml::from_str(&skeleton);
    assert!(
        parsed.is_ok(),
        "what init writes is what the reader reads, or the first thing a person does \
         with this program leaves them a file it refuses: {:?}",
        parsed.err()
    );
}

#[test]
fn the_toolchain_a_run_needs_is_reported_as_a_document_and_as_lines() {
    let fixture = Fixture::copy("fixture-unicode");
    let lines = ask(&fixture, &["doctor"]);
    assert!(
        lines.code <= 1 && !lines.out.trim().is_empty(),
        "a doctor says what a run needs and what is there, because the answer to a run \
         that will not start is usually one of them: {}{}",
        lines.out,
        lines.err
    );

    let document = ask(&fixture, &["doctor", "--json"]);
    let parsed: serde_json::Value =
        serde_json::from_str(&document.out).expect("the doctor answers as a document");
    assert_eq!(
        parsed["document_type"], "rust-mutants/doctor",
        "and says which shape it is before anything reads it: {}",
        document.out
    );
}

#[test]
fn a_recording_that_is_not_there_is_named_rather_than_summarised() {
    let fixture = measured();
    let asked = ask(
        &fixture,
        &["trace", "summary", "--run", "20200101T000000000Z"],
    );
    assert!(
        asked.code != 0 && asked.err.contains("RM0007"),
        "a recording nobody kept is named rather than summarised as an empty run: a \
         table of zeroes reads as a run that did nothing, which is a different thing to \
         go and look for: {}{}",
        asked.out,
        asked.err
    );
}
