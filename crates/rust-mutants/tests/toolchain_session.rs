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
use rust_mutants::testkit::opening::opening;
use rust_mutants::workspace::{OpenOptions, Workspace};

fn open(fixture: &Fixture) -> Workspace {
    Workspace::open(
        fixture.root(),
        opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    )
    .expect("open")
}

fn prepare(fixture: &Fixture) -> Session {
    prepared(fixture, true)
}

/// A session that measures coverage, or one that measures nothing at all and so routes every mutant everywhere.
fn prepared(fixture: &Fixture, coverage: bool) -> Session {
    open(fixture)
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                coverage,
                branch_proofs: coverage,
                touch: coverage,
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

    assert_eq!(session.catalog().len(), 11);
    assert_eq!(session.accepted().len(), 11, "{:?}", session.rejections());
    assert!(session.rejections().is_empty());
    let skips: Vec<(&str, u32)> = session
        .skips()
        .iter()
        .map(|skip| (skip.reason.name(), skip.count))
        .collect();
    assert_eq!(skips, [("test-code", 17), ("test-only-file", 6)]);

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
    let session = prepared(&fixture, false);
    assert!(
        !session.touched().measured() && !session.reached().measured(),
        "nothing narrows this run, so what it walks is every target"
    );
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
            keep_temp: true,
            ..opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp())
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
            trace: recorder.clone(),
            ..opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp())
        },
        &Cancel::new(),
    )
    .expect("open");
    let session = workspace
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                verify: false,
                coverage: false,
                branch_proofs: false,
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
fn the_trace_of_a_covered_run_names_every_layer() {
    use rust_mutants::trace::{MemorySink, Payload, Recorder, Sink};

    let fixture = Fixture::copy("fixture-coverage");
    let recorder = Recorder::wall(Sink::Memory(MemorySink::unbounded()));
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            trace: recorder.clone(),
            ..opening(&mjutest_devkit::paths::cargo_binary(), fixture.temp())
        },
        &Cancel::new(),
    )
    .expect("open");
    let session = workspace
        .prepare(
            &PrepareOptions {
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
            "open", "prepare", "pristine", "discover", "plan", "witness", "coverage", "validate",
            "build", "verify",
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

#[test]
fn the_app_integration_test_finds_its_binary_where_cargo_put_it() {
    let fixture = Fixture::copy("fixture-workspace");
    let session = prepare(&fixture);
    let target = session
        .targets()
        .iter()
        .find(|target| target.id.contains("/test/"))
        .expect("the workspace has an integration test")
        .clone();
    let binaries: Vec<(String, std::path::PathBuf)> = target
        .cargo_env
        .iter()
        .filter_map(|(name, value)| {
            name.to_string_lossy()
                .strip_prefix("CARGO_BIN_EXE_")
                .map(|name| (name.to_owned(), std::path::PathBuf::from(value)))
        })
        .collect();
    assert!(
        !binaries.is_empty(),
        "an integration test of a package with a binary is told where the binary is: {:?}",
        target.cargo_env
    );
    for (name, path) in &binaries {
        assert!(
            path.is_file(),
            "{name} is at {}, which is where the build put it rather than where a profile name \
             and a target name would guess",
            path.display()
        );
    }
    session.close().expect("close");
}

#[test]
fn a_tree_that_checks_but_does_not_link_is_refused_as_a_tree_and_not_as_a_mutation() {
    let fixture = Fixture::copy("fixture-links-nowhere");
    let workspace = open(&fixture);
    let refused = workspace
        .prepare(&PrepareOptions::default(), &Cancel::new())
        .err()
        .map(|error| error.to_string())
        .expect("a tree that does not link is not a tree a run can measure");
    assert!(
        refused.contains("RM4001"),
        "a check answers whether this is a program, not whether it links. The round that links \
         it is what finds out, and with nothing live it can only be the tree: {refused}"
    );
    assert!(
        refused.contains("a_symbol_no_library_supplies"),
        "and the refusal is the linker's own words: {refused}"
    );
    assert!(
        !refused.contains("could not be isolated"),
        "the failure is not read as a mutation nobody could find: {refused}"
    );
}

#[test]
fn list_and_why_skipped_still_only_type_check() {
    let fixture = Fixture::copy("fixture-links-nowhere");
    let workspace = open(&fixture);
    let discovery =
        rust_mutants::session::preview(&workspace, &PrepareOptions::default(), &Cancel::new())
            .expect("a preview rules on nothing, so a tree that does not link is one it can read");
    assert!(
        !discovery.candidates.is_empty(),
        "the preview still finds the candidates it would have proposed"
    );
    workspace.close().expect("close");
}

#[test]
fn a_session_describes_itself_and_hands_out_its_sources() {
    let fixture = Fixture::copy("fixture-simple");
    let session = prepare(&fixture);
    let described = session.describe();

    assert_eq!(described.catalog, session.catalog().clone());
    assert_eq!(described.catalog_digest, session.catalog().digest());
    assert_eq!(described.workspace_digest, session.workspace_digest());
    assert!(!described.toolchain.is_empty());
    let ids: Vec<&str> = described
        .targets
        .iter()
        .map(|target| target.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "fixture-simple/lib/fixture_simple",
            "fixture-simple/test/parity",
            "fixture-simple/doc/fixture_simple"
        ]
    );
    assert!(described.targets.iter().all(|target| target.harness));

    let text = serde_json::to_string(&described).expect("a description serialises");
    let again: rust_mutants::session::Description =
        serde_json::from_str(&text).expect("and reads back");
    assert_eq!(again, described);

    let source = session.source("src/lib.rs").expect("the file as it was");
    assert!(
        std::str::from_utf8(source)
            .expect("utf-8")
            .contains("if a > b { a } else { b }"),
        "a report names the bytes an edit replaces, and showing somebody the edit needs the \
         file they would open rather than the rewrite the snapshot holds"
    );
    assert_eq!(session.source("src/nothing.rs"), None);
    session.close().expect("close");
}

#[test]
fn a_mutant_is_located_by_its_locator_and_a_moved_line_is_reported_not_guessed() {
    let fixture = Fixture::copy("fixture-simple");
    let session = prepare(&fixture);

    let locator = rust_mutants::session::Locator {
        path: "src/lib.rs".to_owned(),
        item: "max".to_owned(),
        rule: "gt-to-ge".to_owned(),
        original: ">".to_owned(),
        line: None,
        count: None,
    };
    let found = session.locate(&locator).expect("the one mutant it names");
    assert_eq!(found.candidate.rule.name, "gt-to-ge");
    assert_eq!(
        session.item_of(found.index),
        Some("max"),
        "a locator names the item a reader would name"
    );

    let several = rust_mutants::session::Locator {
        item: "max".to_owned(),
        rule: "return-default".to_owned(),
        original: "a".to_owned(),
        ..locator.clone()
    };
    assert!(
        session.locate(&several).is_ok(),
        "one branch of the returned if is one mutation"
    );

    let elsewhere = rust_mutants::session::Locator {
        item: "no_such_function".to_owned(),
        ..locator.clone()
    };
    assert!(
        matches!(
            session.locate(&elsewhere),
            Err(rust_mutants::session::LocateError::Nothing)
        ),
        "a locator that names nothing says so rather than guessing"
    );

    let moved = rust_mutants::session::Locator {
        line: Some(1),
        ..locator
    };
    assert!(
        session.locate(&moved).is_ok(),
        "the line is a hint that separates two mutations, never the thing that identifies one"
    );
    session.close().expect("the session closes");
}

#[test]
fn a_locator_that_states_a_count_names_that_many_and_refuses_any_other_number() {
    let fixture = Fixture::copy("fixture-simple");
    let session = prepare(&fixture);

    let one = rust_mutants::session::Locator {
        path: "src/lib.rs".to_owned(),
        item: "max".to_owned(),
        rule: "gt-to-ge".to_owned(),
        original: ">".to_owned(),
        line: None,
        count: None,
    };
    assert_eq!(
        session.locate_all(&one).map(|found| found.len()),
        Ok(1),
        "a locator that states no count names one mutation"
    );
    assert_eq!(
        session
            .locate_all(&rust_mutants::session::Locator {
                count: Some(1),
                ..one.clone()
            })
            .map(|found| found.len()),
        Ok(1),
        "a count of one is the same claim written out"
    );
    let Err(rust_mutants::session::LocateError::Counted {
        wanted,
        display_ids,
    }) = session.locate_all(&rust_mutants::session::Locator {
        count: Some(2),
        ..one
    })
    else {
        panic!("a claim written for two mutations is not a claim about this one");
    };
    assert_eq!(wanted, 2);
    assert_eq!(
        display_ids.len(),
        1,
        "the refusal says what the catalog holds instead: {display_ids:?}"
    );
    session.close().expect("the session closes");
}

#[test]
fn a_claim_written_for_several_mutations_stops_holding_when_one_of_them_is_killed() {
    let fixture = Fixture::copy("fixture-families");
    let session = prepare(&fixture);

    let mut grouped: std::collections::BTreeMap<(String, String, String, String), Vec<u32>> =
        std::collections::BTreeMap::new();
    for mutant in session.catalog().mutants() {
        let Some(item) = session.item_of(mutant.index) else {
            continue;
        };
        grouped
            .entry((
                mutant.candidate.path.clone(),
                item.to_owned(),
                mutant.candidate.rule.name.to_owned(),
                String::from_utf8_lossy(&mutant.candidate.original).into_owned(),
            ))
            .or_default()
            .push(mutant.index);
    }
    let ((path, item, rule, original), indices) = grouped
        .into_iter()
        .find(|(_, indices)| indices.len() > 1)
        .expect("this fixture has a locator that names more than one mutation");
    let locator = rust_mutants::session::Locator {
        path,
        item,
        rule,
        original,
        line: None,
        count: Some(u32::try_from(indices.len()).expect("a small catalog")),
    };
    let expectation = rust_mutants::run::Expectation {
        id: None,
        locator: Some(locator),
        reason: "the reason one reviewer wrote for all of them".to_owned(),
        outcome: Outcome::Survived,
    };
    let judged = |at: usize, outcome: Outcome| {
        let mutant = session
            .catalog()
            .by_index(indices[at])
            .expect("a mutant the catalog holds");
        rust_mutants::run::Judged {
            index: mutant.index,
            id: mutant.id.clone(),
            display_id: mutant.display_id.clone(),
            outcome,
            target: String::new(),
            exit_code: 0,
            duration: std::time::Duration::ZERO,
            tests_run: None,
            failed_tests: Vec::new(),
            signal: None,
            retried: false,
            expected: false,
            not_run_reason: None,
            route: None,
            measured: true,
            identical: None,
            source_run_id: None,
        }
    };

    let mut every: Vec<rust_mutants::run::Judged> = (0..indices.len())
        .map(|at| judged(at, Outcome::Survived))
        .collect();
    let held = rust_mutants::run::verify(&session, std::slice::from_ref(&expectation), &mut every);
    assert!(
        matches!(held[0].standing, rust_mutants::run::Standing::Met),
        "every mutation the claim names came to the outcome it declared: {:?}",
        held[0].standing
    );
    assert_eq!(
        u64::from(held[0].covered),
        u64::try_from(indices.len()).expect("a small catalog"),
        "the claim was resolved against every mutation its locator names"
    );
    assert!(
        every.iter().all(|one| one.expected),
        "a claim that held accounts for every mutation it names"
    );

    let mut one_killed = every.clone();
    for one in &mut one_killed {
        one.expected = false;
    }
    one_killed[indices.len() - 1].outcome = Outcome::Killed;
    let broken = rust_mutants::run::verify(
        &session,
        std::slice::from_ref(&expectation),
        &mut one_killed,
    );
    assert!(
        matches!(
            broken[0].standing,
            rust_mutants::run::Standing::Stale {
                actual: Outcome::Killed
            }
        ),
        "a test killed one of them, so the reason is not what covers it: {:?}",
        broken[0].standing
    );
    assert!(
        one_killed.iter().all(|one| !one.expected),
        "a claim that did not hold accounts for none of them, or the ones it still covers would \
         be exempted on the strength of a test that killed another"
    );
    session.close().expect("the session closes");
}
