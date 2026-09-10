// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands that read what a run left behind, driven in this process against one real run.
//!
//! `toolchain_commands.rs` drives the same commands as a process, which is
//! what a person does and what the exit codes are about. This drives them
//! here, because a measurement of what a crate's own tests reach does not
//! follow a guard into a child: of the 268 mutations of these ten modules,
//! 222 were ones nothing was ever routed to.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "the helpers that copy a fixture and run one verification are not themselves \
              tests, a setup that fails is reported by panicking, and a test reads a \
              document by the names the run it drove put there"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use mjutest_cli::cli::Environment;
use mjutest_devkit::fixture::copy_tree;
use rust_mutants::runner::Cancel;

/// One workspace with one completed run in it, kept for the length of a test.
struct Verified {
    root: PathBuf,
    run: String,
    _dir: tempfile::TempDir,
}

/// What one command said, driven in this process.
#[derive(Debug)]
struct Said {
    code: u8,
    out: String,
    err: String,
}

fn environment(root: &Path) -> Environment {
    let scratch = root.join("mjutest-scratch");
    std::fs::create_dir_all(&scratch).expect("a directory to work in");
    Environment {
        cache_directory: root.join("mjutest-cache"),
        working_directory: root.to_path_buf(),
        temp_directory: scratch,
        vars: std::env::vars_os().collect(),
        cancel: Cancel::new(),
    }
}

fn ask(root: &Path, args: &[&str]) -> Said {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = mjutest_cli::run_from(
        std::iter::once("mjutest")
            .chain(args.iter().copied())
            .map(OsString::from),
        &environment(root),
        &mut out,
        &mut err,
    );
    Said {
        code,
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
    }
}

/// A copy of `fixture-baseline` that one verification has already run in.
fn verified() -> Verified {
    let dir = tempfile::Builder::new()
        .prefix("mjutest-commands-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-baseline");
    copy_tree(
        &mjutest_devkit::paths::fixtures_dir().join("fixture-baseline"),
        &root,
    );
    let said = ask(
        &root,
        &["verify", "--offline", "--locked", "--trace", "--ui=plain"],
    );
    assert_eq!(
        said.code, 2,
        "this fixture has a gap its own tests cannot see: {}{}",
        said.out, said.err
    );
    let index: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join(mjutest_cli::app::reports::LATEST_ANY))
            .expect("the index"),
    )
    .expect("the index is JSON");
    let run = index["run_id"]
        .as_str()
        .expect("the run the index names")
        .to_owned();
    Verified {
        root,
        run,
        _dir: dir,
    }
}

#[test]
fn every_command_that_reads_a_run_reads_the_one_that_ran() {
    let it = verified();
    let lines = ask(&it.root, &["report"]);
    assert_eq!(
        lines.code, 0,
        "reading a report establishes nothing, so it has nothing to fail about: {}",
        lines.err
    );
    assert!(
        lines.out.starts_with("RUN\t") && lines.out.trim_end().ends_with("VERDICT\tINSUFFICIENT"),
        "a report says which run it is about first and what it concluded last, because \
         those are the two things a person came for: {}",
        lines.out
    );

    let document = ask(&it.root, &["report", "--format", "json"]);
    assert_eq!(document.code, 0, "{}", document.err);
    let parsed: serde_json::Value =
        serde_json::from_str(&document.out).expect("the report is a document");
    assert_eq!(
        parsed["run_id"].as_str(),
        Some(it.run.as_str()),
        "and the document names the same run the lines did: two commands over one \
         directory answering about two runs is a report nobody can act on"
    );

    let named = ask(&it.root, &["report", &it.run]);
    assert_eq!(
        (named.code, named.out == lines.out),
        (0, true),
        "a run named by its identity is the run the latest pointer names, when they are \
         the same run: {}",
        named.out
    );

    let missing = ask(&it.root, &["report", "20200101T000000Z-000000"]);
    assert_eq!(missing.code, 3, "{}", missing.out);
    assert!(
        missing.err.contains("20200101T000000Z-000000"),
        "a run that is not there is named rather than answered about, or a person reads \
         another run's verdict believing it is this one's: {}",
        missing.err
    );
}

#[test]
fn a_mutant_is_explained_by_the_run_that_judged_it_and_never_by_a_guess() {
    let it = verified();
    let document = ask(&it.root, &["report", "--format", "json"]);
    let parsed: serde_json::Value =
        serde_json::from_str(&document.out).expect("the report is a document");
    let mutant = parsed["mutants"][0]["display_id"]
        .as_str()
        .expect("a mutant the run judged")
        .to_owned();

    let explained = ask(&it.root, &["explain", &mutant]);
    assert_eq!(explained.code, 0, "{}{}", explained.out, explained.err);
    assert!(
        explained.out.contains(&mutant),
        "what a run recorded about one mutation is shown against the name a person \
         typed: {}",
        explained.out
    );

    let nobody = ask(&it.root, &["explain", "ffffffffffffffff"]);
    assert_eq!(
        nobody.code, 3,
        "while a mutation this run never judged is refused rather than explained as one \
         it did: {}{}",
        nobody.out, nobody.err
    );

    let ambiguous = ask(&it.root, &["explain", ""]);
    assert_ne!(
        ambiguous.code, 0,
        "and a name that could be any of them is not one of them: {}{}",
        ambiguous.out, ambiguous.err
    );
}

#[test]
fn an_acceptance_is_written_where_the_next_run_reads_it() {
    let it = verified();
    let document = ask(&it.root, &["report", "--format", "json"]);
    let parsed: serde_json::Value =
        serde_json::from_str(&document.out).expect("the report is a document");
    let survivor = parsed["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .find(|finding| finding["kind"] == "surviving-mutant")
        .and_then(|finding| finding["subject"].as_str())
        .expect("a survivor to accept")
        .to_owned();

    let accepted = ask(
        &it.root,
        &[
            "accept",
            &survivor,
            "--reason",
            "reviewed: the two forms compile to one",
        ],
    );
    assert_eq!(accepted.code, 0, "{}{}", accepted.out, accepted.err);
    let written = std::fs::read_to_string(it.root.join(mjutest_cli::config::FILE_NAME))
        .expect("the configuration the acceptance was written into");
    assert!(
        written.contains("[[acceptance]]")
            && written.contains("reviewed: the two forms compile to one"),
        "an acceptance is a line in the file the next run reads, with the reason beside \
         it: one recorded anywhere else is a decision the run cannot find, and one with \
         no reason is a mutant nobody looked at: {written}"
    );
    let config = mjutest_cli::config::Config::load(&it.root).expect("a configuration it can read");
    assert!(
        config.acceptance.iter().any(|one| one
            .id
            .starts_with(survivor.split_whitespace().next().unwrap_or(&survivor))
            || survivor.starts_with(&one.id)),
        "and it is one the reader accepts, naming the mutation a person accepted: {:?}",
        config.acceptance
    );
}

#[test]
fn what_a_run_left_behind_is_listed_bundled_and_collected() {
    let it = verified();
    let bundled = ask(&it.root, &["diagnostics", &it.run]);
    assert_eq!(bundled.code, 0, "{}{}", bundled.out, bundled.err);
    let directory = bundled
        .out
        .split_whitespace()
        .map(PathBuf::from)
        .find(|path| path.is_dir())
        .expect("the directory it says it wrote");
    assert!(
        directory
            .join(mjutest_cli::app::diagnostics::MANIFEST_NAME)
            .exists(),
        "a bundle says what is in it, or it is a directory somebody has to guess at: \
         {bundled:?}",
    );

    let held = ask(&it.root, &["cache"]);
    assert_eq!(held.code, 0, "{}{}", held.out, held.err);
    assert!(
        !held.out.trim().is_empty(),
        "a store says what it holds, because the answer to a slow run is often that it \
         holds nothing: {}",
        held.out
    );

    let carried = it.root.join("carried.jsonl");
    let out = ask(
        &it.root,
        &["cache", "--export", &carried.display().to_string()],
    );
    assert_eq!(out.code, 0, "{}{}", out.out, out.err);
    let back = ask(
        &it.root,
        &["cache", "--import", &carried.display().to_string()],
    );
    assert_eq!(
        back.code, 0,
        "what one machine established is written out and read back on another, and a \
         store that cannot take back what it wrote is one nobody can carry: {}{}",
        back.out, back.err
    );

    let elsewhere = ask(&it.root, &["cache", "--import", "nowhere.jsonl"]);
    assert_eq!(
        elsewhere.code, 3,
        "while a file that is not there is named rather than read as an empty store: an \
         import that quietly carried nothing is a machine that does all the work again \
         and reports that it did not: {}{}",
        elsewhere.out, elsewhere.err
    );

    let planned = ask(&it.root, &["plan", "--offline", "--locked"]);
    assert_eq!(
        planned.code, 0,
        "saying what a run would measure measures nothing, so it establishes nothing to \
         fail about: {}{}",
        planned.out, planned.err
    );
    assert!(
        !planned.out.trim().is_empty(),
        "and says it rather than saying nothing at all: {}",
        planned.out
    );
}

#[test]
fn a_configuration_is_written_once_and_never_over_one_somebody_wrote() {
    let dir = tempfile::Builder::new()
        .prefix("mjutest-init-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().to_path_buf();

    let written = ask(&root, &["init"]);
    assert_eq!(written.code, 0, "{}{}", written.out, written.err);
    let path = root.join(mjutest_cli::config::FILE_NAME);
    assert_eq!(
        std::fs::read_to_string(&path).expect("the skeleton"),
        mjutest_cli::config::skeleton(),
        "what init writes is the skeleton, so a person who reads the file and a person \
         who reads the documentation of it are reading one thing"
    );

    std::fs::write(&path, "version = 1\n# mine\n").expect("a configuration somebody wrote");
    let refused = ask(&root, &["init"]);
    assert_eq!(
        refused.code, 3,
        "a second init does not write over a configuration somebody has edited: the \
         acceptances in it are the reviews of every survivor this project has looked at, \
         and they are not recoverable from anywhere else: {}{}",
        refused.out, refused.err
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the configuration"),
        "version = 1\n# mine\n",
        "and leaves it exactly as it was"
    );

    let forced = ask(&root, &["init", "--force"]);
    assert_eq!(
        forced.code, 0,
        "while a person who says to replace it is one who meant to: {}{}",
        forced.out, forced.err
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the skeleton"),
        mjutest_cli::config::skeleton()
    );
}
