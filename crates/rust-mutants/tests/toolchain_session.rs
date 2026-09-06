// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The public API, end to end: open a read-only tree, prepare it, and run mutants against the build that preparation produced.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::Path;

use mjutest_devkit::fixture::Fixture;
use rust_mutants::outcome::Outcome;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Request, Session};
use rust_mutants::workspace::{OpenOptions, Workspace};

fn open(fixture: &Fixture) -> Workspace {
    Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp().to_path_buf(),
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

/// The digest of every file of a tree, so "the source was not touched" can be asserted rather than hoped.
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
    let fixture = Fixture::copy("fixture-simple");
    let before = fingerprint(fixture.root());
    let workspace = open(&fixture);

    assert_eq!(
        workspace.root(),
        fixture.root().canonicalize().expect("real")
    );
    assert!(workspace.snapshot_root().join("src/lib.rs").is_file());
    assert_ne!(workspace.snapshot_root(), fixture.root());
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
        fingerprint(fixture.root()),
        before,
        "the source tree is read-only"
    );
}

#[test]
fn preparing_catalogs_instruments_validates_and_builds() {
    let fixture = Fixture::copy("fixture-simple");
    let before = fingerprint(fixture.root());
    let session = prepare(&fixture);

    assert_eq!(session.catalog().len(), 6);
    assert_eq!(session.accepted().len(), 6, "{:?}", session.rejections());
    assert!(session.rejections().is_empty());
    let skips: Vec<(&str, u32)> = session
        .skips()
        .iter()
        .map(|skip| (skip.reason.name(), skip.count))
        .collect();
    assert_eq!(skips, [("test-code", 2), ("test-only-file", 2)]);

    let targets: Vec<&str> = session
        .targets()
        .iter()
        .map(|target| target.id.as_str())
        .collect();
    assert_eq!(
        targets,
        [
            "fixture-simple/lib/fixture_simple",
            "fixture-simple/test/parity",
            "fixture-simple/doc/fixture_simple"
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
        fingerprint(fixture.root()),
        before,
        "the source tree is read-only"
    );
    session.close().expect("close");
}

#[test]
fn a_mutant_runs_against_every_target_until_one_kills_it() {
    let fixture = Fixture::copy("fixture-simple");
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

    let killed = session
        .exec(&Request::new(by_rule("return-default")), &cancel)
        .expect("exec");
    assert_eq!(killed.outcome, Outcome::Killed);
    assert_eq!(killed.target, "fixture-simple/lib/fixture_simple");
    assert!(killed.tests_run.unwrap_or_default() > 0);

    let survivor = session
        .exec(&Request::new(by_rule("gt-to-ge")), &cancel)
        .expect("exec");
    assert_eq!(survivor.outcome, Outcome::Survived);
    assert_eq!(
        survivor.target, "fixture-simple/test/parity",
        "every target ran, and the last one had the last word"
    );

    let one = session
        .exec(
            &Request::new(by_rule("return-default"))
                .with_target("fixture-simple/lib/fixture_simple".to_owned())
                .test(Some("tests::max_picks_the_larger".to_owned())),
            &cancel,
        )
        .expect("exec");
    assert_eq!(one.outcome, Outcome::Killed);
    assert_eq!(one.summary.expect("a summary").failed, 1);

    let nothing = session
        .exec(
            &Request::new(by_rule("return-default"))
                .with_target("parity".to_owned())
                .test(Some("no::such::test".to_owned())),
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
    let fixture = Fixture::copy("fixture-simple");
    let session = prepare(&fixture);
    let cancel = Cancel::new();

    let unknown = session
        .exec(&Request::new("ffffffff".to_owned()), &cancel)
        .unwrap_err();
    assert!(unknown.to_string().contains("RM5003"), "{unknown}");

    let short = session
        .exec(&Request::new("a".to_owned()), &cancel)
        .unwrap_err();
    assert!(short.to_string().contains("RM5003"), "{short}");

    let target = session
        .exec(
            &Request::new(session.catalog().mutants()[0].display_id.clone())
                .with_target("no-such-target".to_owned()),
            &cancel,
        )
        .unwrap_err();
    assert!(target.to_string().contains("RM5004"), "{target}");
    session.close().expect("close");
}

#[test]
fn a_refused_candidate_keeps_the_compilers_own_words_and_costs_no_sibling() {
    let fixture = Fixture::copy("fixture-rejectable");
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
    let fixture = Fixture::copy("fixture-simple");
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp().to_path_buf(),
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

    let fixture = Fixture::copy("fixture-rejectable");
    let recorder = Recorder::wall(Sink::Memory(MemorySink::unbounded()));
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp().to_path_buf(),
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
        .exec(&Request::new(mutant), &Cancel::new())
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

    for event in &events {
        if let Payload::Instrument { instrument } = &event.payload {
            assert_eq!(
                instrument.lines_before, instrument.lines_after,
                "{} moved a line",
                instrument.path
            );
            assert!(
                instrument.module.starts_with("__rm_"),
                "every file's runtime module is named after its own path: {}",
                instrument.module
            );
        }
    }

    let executed: Vec<(&str, &str)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            Payload::MutantExec { mutant } => {
                Some((mutant.outcome.as_str(), mutant.target.as_str()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        executed
            .iter()
            .filter(|(_, target)| !target.contains("/doc/"))
            .count(),
        1,
        "a library that documents no example answers nothing, and a request that names no \
         target passes over it: {executed:?}"
    );
}

#[test]
fn a_target_with_no_tests_in_it_answers_neither_question() {
    let fixture = Fixture::copy("fixture-subprocess");
    let session = prepare(&fixture);
    let cancel = Cancel::new();
    let mutant = session
        .catalog()
        .mutants()
        .iter()
        .find(|mutant| mutant.candidate.rule.name == "negate-condition")
        .expect("a negate-condition mutant")
        .display_id
        .clone();
    let request = Request::new(mutant);

    let killed = session.exec(&request, &cancel).expect("exec");
    assert_eq!(
        killed.outcome,
        Outcome::Killed,
        "the library and the binary hold no tests, and passing over them is what lets \
         the one target that does hold tests answer"
    );
    assert_eq!(killed.target, "fixture-subprocess/test/through_the_binary");

    let control = session.control(&request, &cancel).expect("control");
    assert_eq!(
        control.outcome,
        Outcome::Survived,
        "the original passes, and a sibling target that ran nothing is not a reason to \
         say it did not"
    );
}

#[test]
fn a_dependency_s_documentation_is_not_this_run_s_to_measure() {
    let fixture = Fixture::copy("fixture-simple");
    let session = prepare(&fixture);

    let documentation: Vec<&str> = session
        .targets()
        .iter()
        .filter(|target| target.kind == rust_mutants::execute::TargetKind::Doc)
        .map(|target| target.package.as_str())
        .collect();

    assert_eq!(
        documentation,
        ["fixture-simple"],
        "the workspace's own members and never the whole resolved graph: asking cargo to \
         run a dependency's examples asks it to resolve that dependency's own \
         dev-dependencies, which a lock file for this workspace never pinned"
    );
}

#[test]
fn the_trace_of_a_probed_and_covered_run_names_every_layer() {
    use rust_mutants::trace::{MemorySink, Payload, Recorder, Sink};

    let fixture = Fixture::copy("fixture-coverage");
    let recorder = Recorder::wall(Sink::Memory(MemorySink::unbounded()));
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            temp_directory: fixture.temp().to_path_buf(),
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
                probe: true,
                coverage: true,
                ..PrepareOptions::default()
            },
            &Cancel::new(),
        )
        .expect("prepare");
    recorder.run_end("ok", None);
    let events = recorder.events();

    let phases: Vec<(String, bool)> = events
        .iter()
        .filter_map(|event| match &event.payload {
            Payload::PhaseStart { phase } => Some((phase.name.clone(), false)),
            Payload::PhaseEnd { phase } => Some((phase.name.clone(), true)),
            _ => None,
        })
        .collect();
    let began: Vec<&str> = phases
        .iter()
        .filter(|(_, ended)| !ended)
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(
        began,
        [
            "open", "prepare", "pristine", "discover", "plan", "probe", "witness", "coverage",
            "validate", "build", "verify",
        ],
        "every stage of a preparation is a phase a reader can time"
    );
    assert!(
        rust_mutants::trace::check(&events).is_empty(),
        "every phase that began ended: {:?}",
        rust_mutants::trace::check(&events)
    );
    for (name, ended) in &phases {
        if *ended {
            let timed = events.iter().any(|event| {
                matches!(&event.payload, Payload::PhaseEnd { phase }
                    if phase.name == *name && phase.duration_ms.is_some())
            });
            assert!(timed, "{name} ended without saying how long it took");
        }
    }

    let of = |name: &str| {
        events
            .iter()
            .filter(|event| event.payload.type_name() == name)
            .count()
    };
    assert!(of("verify") > 0, "a target was run with nothing active");
    assert!(
        of("probe-exec") > 0,
        "a target was run against the probe tree"
    );
    assert!(of("witness") > 0, "a claim was put to the compiler");
    assert!(of("discover-file") > 0);
    assert!(of("instrument") > 0);

    let mutant = session.catalog().at(0).expect("a mutant");
    let route = session.route(mutant);
    assert!(
        !route.reaching().is_empty() || route.granularity() == "unreached",
        "a route either names targets or says nothing reaches it: {route:?}"
    );
    session.close().expect("close");
}
