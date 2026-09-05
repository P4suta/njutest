// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The public API, end to end: open a read-only tree, prepare it, and run
//! mutants against the build that preparation produced.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};

use rust_mutants::outcome::Outcome;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Request, Session};
use rust_mutants::workspace::{OpenOptions, Workspace};

/// A copy of a fixture, so the source tree the engine opens is a throwaway.
struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
    _temp: tempfile::TempDir,
    temp_root: PathBuf,
}

fn fixture(name: &str) -> Fixture {
    let dir = tempfile::Builder::new()
        .prefix("rust-mutants-session-")
        .tempdir()
        .expect("tempdir");
    let root = dir.path().join(name);
    copy_dir(&mjutest_devkit::paths::fixtures_dir().join(name), &root);
    let temp = tempfile::Builder::new()
        .prefix("rust-mutants-session-temp-")
        .tempdir()
        .expect("tempdir");
    let temp_root = temp.path().to_path_buf();
    Fixture {
        root,
        _dir: dir,
        _temp: temp,
        temp_root,
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("mkdir");
    for entry in std::fs::read_dir(from).expect("read_dir") {
        let entry = entry.expect("entry");
        if entry.file_name() == "target" {
            continue;
        }
        let destination = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_dir(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), &destination).expect("copy");
        }
    }
}

fn open(fixture: &Fixture) -> Workspace {
    Workspace::open(
        &fixture.root,
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp_root.clone(),
            env: std::env::vars_os().collect(),
            locked: true,
            offline: true,
            ..OpenOptions::default()
        },
        &Cancel::new(),
    )
    .expect("open")
}

fn prepare(fixture: &Fixture) -> Session {
    open(fixture)
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                ..PrepareOptions::default()
            },
            &Cancel::new(),
        )
        .expect("prepare")
}

/// The digest of every file of a tree, so "the source was not touched" can
/// be asserted rather than hoped.
fn fingerprint(root: &Path) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read_dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if entry.file_type().expect("type").is_dir() {
                if entry.file_name() != "target" {
                    stack.push(path);
                }
                continue;
            }
            let bytes = std::fs::read(&path).expect("read");
            entries.push((
                path.strip_prefix(root)
                    .expect("under the root")
                    .to_string_lossy()
                    .into_owned(),
                hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&bytes)),
            ));
        }
    }
    entries.sort();
    entries
}

#[test]
fn opening_copies_the_tree_and_never_writes_to_it() {
    let fixture = fixture("fixture-simple");
    let before = fingerprint(&fixture.root);
    let workspace = open(&fixture);

    assert_eq!(workspace.root(), fixture.root.canonicalize().expect("real"));
    assert!(workspace.snapshot_root().join("src/lib.rs").is_file());
    assert_ne!(workspace.snapshot_root(), fixture.root);
    assert_eq!(workspace.workspace_digest().len(), 64);
    assert!(workspace.toolchain().host().contains('-'));
    let members: Vec<&str> = workspace
        .metadata()
        .members()
        .map(|package| package.name.as_str())
        .collect();
    assert_eq!(members, ["fixture-simple"]);
    assert!(
        workspace
            .target_dir()
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .starts_with("rust-mutants-target-")
    );
    assert!(workspace.swept().failures.is_empty());

    let dir = workspace.snapshot_dir().to_path_buf();
    assert!(workspace.close().expect("close").is_empty());
    assert!(!dir.exists(), "the snapshot goes when the workspace does");
    assert_eq!(
        fingerprint(&fixture.root),
        before,
        "the source tree is read-only"
    );
}

#[test]
fn preparing_catalogs_instruments_validates_and_builds() {
    let fixture = fixture("fixture-simple");
    let before = fingerprint(&fixture.root);
    let session = prepare(&fixture);

    assert_eq!(session.catalog().len(), 6);
    assert_eq!(session.accepted().len(), 6, "{:?}", session.rejections());
    assert!(session.rejections().is_empty());
    let skips: Vec<(&str, u32)> = session
        .skips()
        .iter()
        .map(|skip| (skip.reason.name(), skip.count))
        .collect();
    assert_eq!(skips, [("test-code", 3), ("test-only-file", 2)]);

    let targets: Vec<&str> = session
        .targets()
        .iter()
        .map(|target| target.id.as_str())
        .collect();
    assert_eq!(
        targets,
        [
            "fixture-simple/lib/fixture_simple",
            "fixture-simple/test/parity"
        ]
    );
    for target in session.targets() {
        assert!(
            target.executable.is_file(),
            "{}",
            target.executable.display()
        );
        assert_eq!(target.cwd, session.snapshot_root());
    }
    assert_eq!(
        fingerprint(&fixture.root),
        before,
        "the source tree is read-only"
    );
    session.close().expect("close");
}

#[test]
fn a_mutant_runs_against_every_target_until_one_kills_it() {
    let fixture = fixture("fixture-simple");
    let session = prepare(&fixture);
    let cancel = Cancel::new();

    let by_rule = |rule: &str| -> String {
        session
            .catalog()
            .mutants()
            .iter()
            .find(|mutant| mutant.candidate.rule.name == rule)
            .unwrap_or_else(|| panic!("a {rule} mutant"))
            .display_id
            .clone()
    };

    // `max` returning the default is what the library's own test is about.
    let killed = session
        .exec(
            &Request {
                mutant: by_rule("return-default"),
                ..Request::default()
            },
            &cancel,
        )
        .expect("exec");
    assert_eq!(killed.outcome, Outcome::Killed);
    assert_eq!(killed.target, "fixture-simple/lib/fixture_simple");
    assert!(killed.tests_run.unwrap_or_default() > 0);

    // `>` and `>=` differ only on equal arguments, which no test passes.
    let survivor = session
        .exec(
            &Request {
                mutant: by_rule("gt-to-ge"),
                ..Request::default()
            },
            &cancel,
        )
        .expect("exec");
    assert_eq!(survivor.outcome, Outcome::Survived);
    assert_eq!(
        survivor.target, "fixture-simple/test/parity",
        "every target ran, and the last one had the last word"
    );

    // One target, one test.
    let one = session
        .exec(
            &Request {
                mutant: by_rule("return-default"),
                target: Some("fixture-simple/lib/fixture_simple".to_owned()),
                test: Some("tests::max_picks_the_larger".to_owned()),
                ..Request::default()
            },
            &cancel,
        )
        .expect("exec");
    assert_eq!(one.outcome, Outcome::Killed);
    assert_eq!(one.summary.expect("a summary").failed, 1);

    // A filter that matches nothing is green and empty, and says so.
    let nothing = session
        .exec(
            &Request {
                mutant: by_rule("return-default"),
                target: Some("parity".to_owned()),
                test: Some("no::such::test".to_owned()),
                ..Request::default()
            },
            &cancel,
        )
        .expect("exec");
    assert_eq!(nothing.outcome, Outcome::Inconclusive);

    let drift = session.changes().expect("changes");
    assert!(
        drift.is_empty(),
        "no test wrote into the tree: {:?}",
        drift
            .iter()
            .map(|one| (one.kind().name(), one.rel_path()))
            .collect::<Vec<_>>()
    );
    session.close().expect("close");
}

#[test]
fn a_request_that_names_nothing_is_refused_by_name() {
    let fixture = fixture("fixture-simple");
    let session = prepare(&fixture);
    let cancel = Cancel::new();

    let unknown = session
        .exec(
            &Request {
                mutant: "ffffffff".to_owned(),
                ..Request::default()
            },
            &cancel,
        )
        .unwrap_err();
    assert!(unknown.to_string().contains("RM5003"), "{unknown}");

    let short = session
        .exec(
            &Request {
                mutant: "a".to_owned(),
                ..Request::default()
            },
            &cancel,
        )
        .unwrap_err();
    assert!(short.to_string().contains("RM5003"), "{short}");

    let target = session
        .exec(
            &Request {
                mutant: session.catalog().mutants()[0].display_id.clone(),
                target: Some("no-such-target".to_owned()),
                ..Request::default()
            },
            &cancel,
        )
        .unwrap_err();
    assert!(target.to_string().contains("RM5004"), "{target}");
    session.close().expect("close");
}

#[test]
fn a_refused_candidate_keeps_the_compilers_own_words_and_costs_no_sibling() {
    let fixture = fixture("fixture-rejectable");
    let session = prepare(&fixture);
    let mut refused: Vec<&str> = session
        .rejections()
        .iter()
        .map(|rejection| rejection.rule.as_str())
        .collect();
    refused.sort_unstable();
    assert_eq!(
        refused,
        [
            "add-to-sub",
            "mul-to-div",
            "range-to-inclusive",
            "return-default"
        ],
        "mul-to-div is refused by a lint that only fires once code is \
         generated, which is why validation compiles the way the run runs"
    );
    assert!(
        session
            .rejections()
            .iter()
            .any(|rejection| rejection.diagnostic.contains("cannot subtract"))
    );
    assert_eq!(
        session.accepted().len() + session.rejections().len(),
        session.catalog().len()
    );
    assert!(!session.accepted().is_empty());
    session.close().expect("close");
}

#[test]
fn keeping_the_temporary_directories_preserves_them_and_says_which() {
    let fixture = fixture("fixture-simple");
    let workspace = Workspace::open(
        &fixture.root,
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp_root.clone(),
            env: std::env::vars_os().collect(),
            locked: true,
            offline: true,
            keep_temp: true,
            ..OpenOptions::default()
        },
        &Cancel::new(),
    )
    .expect("open");
    let dir = workspace.snapshot_dir().to_path_buf();
    let kept = workspace.close().expect("close");
    assert_eq!(kept.first(), Some(&dir));
    assert!(dir.join("tree/src/lib.rs").is_file(), "kept means kept");
    std::fs::remove_dir_all(&dir).expect("tidy");
}

#[test]
fn the_trace_says_what_every_phase_did() {
    use rust_mutants::trace::{MemorySink, Payload, Recorder, Sink};

    let fixture = fixture("fixture-rejectable");
    let recorder = Recorder::wall(Sink::Memory(MemorySink::unbounded()));
    let workspace = Workspace::open(
        &fixture.root,
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp_root.clone(),
            env: std::env::vars_os().collect(),
            locked: true,
            offline: true,
            trace: recorder.clone(),
            ..OpenOptions::default()
        },
        &Cancel::new(),
    )
    .expect("open");
    let session = workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                verify: false,
                ..PrepareOptions::default()
            },
            &Cancel::new(),
        )
        .expect("prepare");
    let mutant = session.catalog().mutants()[0].display_id.clone();
    let _result = session
        .exec(
            &Request {
                mutant,
                ..Request::default()
            },
            &Cancel::new(),
        )
        .expect("exec");
    recorder.run_end("ok", None);
    session.close().expect("close");

    let events = recorder.events();
    let types: Vec<&str> = events
        .iter()
        .map(|event| event.payload.type_name())
        .collect();
    for expected in [
        "run-start",
        "open",
        "snapshot",
        "discover-file",
        "instrument",
        "validate-round",
        "build",
        "mutant-exec",
        "exec",
        "run-end",
    ] {
        assert!(
            types.contains(&expected),
            "{expected} is missing from {types:?}"
        );
    }

    // Every validation round is readable, and the refusals name the mutants
    // and carry the compiler's own words.
    let rounds: Vec<(u32, bool, usize)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            Payload::ValidateRound { round } => {
                Some((round.round, round.success, round.attributed.len()))
            }
            _ => None,
        })
        .collect();
    assert!(rounds.len() >= 2, "{rounds:?}");
    assert_eq!(rounds.first().map(|round| round.1), Some(false));
    assert_eq!(rounds.last().map(|round| round.1), Some(true));
    let attributed: Vec<(u32, String)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            Payload::ValidateRound { round } => Some(round.attributed.clone()),
            _ => None,
        })
        .flatten()
        .map(|one| (one.index, one.said))
        .collect();
    assert_eq!(attributed.len(), 4, "{attributed:?}");
    assert!(
        attributed
            .iter()
            .any(|(_, said)| said.contains("cannot subtract")),
        "{attributed:?}"
    );

    // Instrumentation says it moved no line.
    for event in &events {
        if let Payload::Instrument { instrument } = &event.payload {
            assert_eq!(
                instrument.lines_before, instrument.lines_after,
                "{} moved a line",
                instrument.path
            );
            assert_eq!(instrument.module, "__rm");
        }
    }

    // The execution says what it established.
    let executed: Vec<(&str, &str)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            Payload::MutantExec { mutant } => {
                Some((mutant.outcome.as_str(), mutant.target.as_str()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(executed.len(), 1, "{executed:?}");
}
