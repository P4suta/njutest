// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The public API, end to end: open a read-only tree, prepare it, and run mutants against the build that preparation produced.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::Path;

use njutest_devkit::fixture::Fixture;
use njutest_devkit::result::{ResultState::Refused, result_state};
use rust_mutants::outcome::Outcome;
use rust_mutants::rule::Tier;
use rust_mutants::run::Filter;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{Observing, PrepareOptions, Request, Session};
use rust_mutants::testkit::opening::opening;
use rust_mutants::trace::{MutantExecRecord, Payload, PhaseRecord, ValidateRoundRecord};
use rust_mutants::workspace::{OpenOptions, Workspace};

enum RelevantPayload<'a> {
    PhaseStart(&'a PhaseRecord),
    PhaseEnd(&'a PhaseRecord),
    ValidateRound(&'a ValidateRoundRecord),
    MutantExec(&'a MutantExecRecord),
    Other,
}

const fn relevant_payload(payload: &Payload) -> RelevantPayload<'_> {
    match payload {
        Payload::PhaseStart { phase } => RelevantPayload::PhaseStart(phase),
        Payload::PhaseEnd { phase } => RelevantPayload::PhaseEnd(phase),
        Payload::ValidateRound { round } => RelevantPayload::ValidateRound(round),
        Payload::MutantExec { mutant } => RelevantPayload::MutantExec(mutant),
        Payload::RunStart { .. }
        | Payload::Open { .. }
        | Payload::Snapshot { .. }
        | Payload::Exec { .. }
        | Payload::DiscoverFile { .. }
        | Payload::Instrument { .. }
        | Payload::Bisect { .. }
        | Payload::Build { .. }
        | Payload::Verify { .. }
        | Payload::Touch { .. }
        | Payload::PerturbedControl { .. }
        | Payload::Witness { .. }
        | Payload::SkipClaim { .. }
        | Payload::Kept { .. }
        | Payload::Route { .. }
        | Payload::Cache { .. }
        | Payload::Select { .. }
        | Payload::Identical { .. }
        | Payload::Evidence { .. }
        | Payload::Note { .. }
        | Payload::RunEnd { .. } => RelevantPayload::Other,
    }
}

fn open(fixture: &Fixture) -> Workspace {
    Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
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
                    .to_str()
                    .expect("fixture paths are exact UTF-8")
                    .to_owned(),
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
        fixture.root(),
        "the run answers about the directory it was given, in the spelling it was given"
    );
    assert!(
        std::fs::metadata(workspace.snapshot_root().join("src/lib.rs"))
            .expect("snapshot source metadata")
            .is_file()
    );
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
            .to_str()
            .expect("fixture paths are exact UTF-8")
            .starts_with("rust-mutants-target-")
    );
    assert!(workspace.swept().failures.is_empty());

    let dir = workspace.snapshot_dir().to_path_buf();
    assert!(workspace.close().expect("close").is_empty());
    assert!(
        matches!(
            std::fs::symlink_metadata(&dir),
            Err(ref error) if error.kind() == std::io::ErrorKind::NotFound
        ),
        "the snapshot goes when the workspace does"
    );
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

    assert_eq!(session.catalog().len(), 13);
    assert_eq!(session.accepted().len(), 13, "{:?}", session.rejections());
    assert!(session.rejections().is_empty());
    let skips: Vec<(&str, u32)> = session
        .skips()
        .iter()
        .map(|skip| (skip.reason.name(), skip.count))
        .collect();
    assert_eq!(
        skips,
        [
            ("test-code", 22),
            ("test-only-file", 6),
            ("let-condition", 1)
        ]
    );

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
            std::fs::metadata(&target.executable)
                .expect("target executable metadata")
                .is_file(),
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
fn a_scoped_session_keeps_the_catalog_but_cannot_execute_an_unvalidated_candidate() {
    let fixture = Fixture::copy("fixture-simple");
    let session = open(&fixture)
        .prepare(
            &PrepareOptions {
                tier: Tier::All,
                verify: false,
                coverage: false,
                branch_proofs: false,
                touch: false,
                validation_filter: Some(Filter {
                    rules: vec!["gt-to-ge".to_owned()],
                    ..Filter::default()
                }),
                ..PrepareOptions::default()
            },
            &Cancel::new(),
        )
        .expect("prepare only the selected rule");

    assert_eq!(session.catalog().len(), 13, "identity remains global");
    assert_eq!(session.accepted().len(), 1, "validation follows the scope");
    let outside = session
        .catalog()
        .mutants()
        .iter()
        .find(|mutant| mutant.candidate.rule.name == "return-default")
        .expect("an unselected candidate");
    assert!(!session.was_validated(outside.index));

    for error in [
        session
            .exec(
                &Request::new(outside.display_id.to_string()),
                &Cancel::new(),
            )
            .expect_err("an unvalidated guard is absent from the build"),
        session
            .judge(
                &Request::new(outside.display_id.to_string()),
                &rust_mutants::run::Quiet::default(),
                &Cancel::new(),
            )
            .expect_err("judging has the same fail-closed boundary"),
    ] {
        let said = error.to_string();
        assert!(
            said.contains("RM5003")
                && said.contains("not in this instrumented build")
                && said.contains("unvalidated"),
            "the refusal distinguishes a catalog identity from an executable guard: {said}"
        );
    }
    session.close().expect("close");
}

#[test]
fn consecutive_scopes_cannot_reuse_a_stale_instrumented_binary() {
    let fixture = Fixture::copy("fixture-simple");
    let options = |filter: Filter| PrepareOptions {
        tier: Tier::All,
        verify: false,
        coverage: false,
        branch_proofs: false,
        touch: false,
        validation_filter: Some(filter),
        ..PrepareOptions::default()
    };

    let first = open(&fixture)
        .prepare(
            &options(Filter {
                rules: vec!["gt-to-ge".to_owned()],
                ..Filter::default()
            }),
            &Cancel::new(),
        )
        .expect("prepare scope A");
    assert_eq!(first.accepted().len(), 1);
    first.close().expect("close A");

    let empty = open(&fixture)
        .prepare(
            &options(Filter {
                ids: Some(Vec::new()),
                ..Filter::default()
            }),
            &Cancel::new(),
        )
        .expect("prepare the empty scope");
    assert!(empty.accepted().is_empty());
    empty.close().expect("close empty");

    let last = open(&fixture)
        .prepare(
            &options(Filter {
                rules: vec!["return-default".to_owned()],
                ..Filter::default()
            }),
            &Cancel::new(),
        )
        .expect("prepare scope B");
    assert!(
        last.accepted().iter().all(|index| {
            last.catalog()
                .by_index(*index)
                .is_some_and(|mutant| mutant.candidate.rule.name == "return-default")
        }),
        "the last build contains only scope B"
    );
    let mutant = last
        .accepted()
        .first()
        .and_then(|index| last.catalog().by_index(*index))
        .expect("scope B has an executable candidate");
    let result = last
        .exec(&Request::new(mutant.display_id.to_string()), &Cancel::new())
        .expect("the scope-B guard is in the final binary");
    assert_eq!(result.outcome(), Outcome::Killed);
    last.close().expect("close B");
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
            .expect("the requested rule has a mutant")
            .display_id
            .to_string()
    };

    let killed = session
        .exec(&Request::new(by_rule("return-default")), &cancel)
        .expect("exec");
    assert_eq!(killed.outcome(), Outcome::Killed);
    assert_eq!(killed.target, "fixture-simple/lib/fixture_simple");
    assert!(killed.tests_run().unwrap_or_default() > 0);

    let survivor = session
        .exec(&Request::new(by_rule("gt-to-ge")), &cancel)
        .expect("exec");
    assert_eq!(survivor.outcome(), Outcome::Survived);
    assert_eq!(
        survivor.target, "fixture-simple/lib/fixture_simple",
        "every target ran; when they all say the same thing, the stable lexical representative \
         is independent of which result happened to arrive last"
    );

    let one = session
        .exec(
            &Request::new(by_rule("return-default"))
                .with_target("fixture-simple/lib/fixture_simple".to_owned())
                .test(Some("tests::max_picks_the_larger".to_owned())),
            &cancel,
        )
        .expect("exec");
    assert_eq!(one.outcome(), Outcome::Killed);
    assert_eq!(one.summary.expect("a summary").failed, 1);

    let nothing = session
        .exec(
            &Request::new(by_rule("return-default"))
                .with_target("parity".to_owned())
                .test(Some("no::such::test".to_owned())),
            &cancel,
        )
        .expect("exec");
    assert_eq!(nothing.outcome(), Outcome::Inconclusive);

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
        .expect_err("an unknown mutant is refused");
    assert!(unknown.to_string().contains("RM5003"), "{unknown}");

    let short = session
        .exec(&Request::new("a".to_owned()), &cancel)
        .expect_err("a short mutant identity is refused");
    assert!(short.to_string().contains("RM5003"), "{short}");

    let judged = session
        .judge(
            &Request::new("ffffffff".to_owned()),
            &rust_mutants::run::Quiet::default(),
            &cancel,
        )
        .expect_err("an unknown mutant cannot be judged");
    assert!(
        judged.to_string().contains("RM5003"),
        "every way of asking about a mutation refuses a name no mutation answers to, rather \
         than ending the run where it was asked: {judged}"
    );

    let target = session
        .exec(
            &Request::new(session.catalog().mutants()[0].display_id.to_string())
                .with_target("no-such-target".to_owned()),
            &cancel,
        )
        .expect_err("an unknown target is refused");
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
            ..opening(&njutest_devkit::paths::cargo_binary(), fixture.temp())
        },
        &Cancel::new(),
    )
    .expect("open");
    let dir = workspace.snapshot_dir().to_path_buf();
    let kept = workspace.close().expect("close");
    assert_eq!(kept.first(), Some(&dir));
    assert!(
        std::fs::metadata(dir.join("tree/src/lib.rs"))
            .expect("kept source metadata")
            .is_file(),
        "kept means kept"
    );
    std::fs::remove_dir_all(&dir).expect("tidy");
}

#[test]
fn the_trace_says_what_every_phase_did() {
    use rust_mutants::trace::{MemorySink, Recorder, Sink};

    let fixture = Fixture::copy("fixture-rejectable");
    let recorder = Recorder::wall(
        Sink::Memory(MemorySink::unbounded()),
        rust_mutants::testkit::trace::standalone_context(),
    );
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            trace: recorder.clone(),
            ..opening(&njutest_devkit::paths::cargo_binary(), fixture.temp())
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
    let mutant = session.catalog().mutants()[0].display_id.to_string();
    let result = session
        .exec(&Request::new(mutant), &Cancel::new())
        .expect("exec");
    drop(result);
    recorder
        .run_end(rust_mutants::trace::RunOutcome::Completed, None)
        .expect("trace closes");
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
        .filter_map(|event| match relevant_payload(&event.payload) {
            RelevantPayload::ValidateRound(round) => {
                Some((round.round, round.success, round.attributed.len()))
            }
            RelevantPayload::PhaseStart(_)
            | RelevantPayload::PhaseEnd(_)
            | RelevantPayload::MutantExec(_)
            | RelevantPayload::Other => None,
        })
        .collect();
    assert!(rounds.len() >= 2, "{rounds:?}");
    assert_eq!(rounds.first().map(|round| round.1), Some(false));
    assert_eq!(rounds.last().map(|round| round.1), Some(true));
    let attributed: Vec<(u32, String)> = events
        .iter()
        .filter_map(|event| match relevant_payload(&event.payload) {
            RelevantPayload::ValidateRound(round) => Some(round.attributed.clone()),
            RelevantPayload::PhaseStart(_)
            | RelevantPayload::PhaseEnd(_)
            | RelevantPayload::MutantExec(_)
            | RelevantPayload::Other => None,
        })
        .flat_map(IntoIterator::into_iter)
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
        .filter_map(|event| match relevant_payload(&event.payload) {
            RelevantPayload::MutantExec(mutant) => {
                Some((mutant.outcome.as_str(), mutant.target.as_str()))
            }
            RelevantPayload::PhaseStart(_)
            | RelevantPayload::PhaseEnd(_)
            | RelevantPayload::ValidateRound(_)
            | RelevantPayload::Other => None,
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
    let request = Request::new(mutant.to_string());

    let killed = session.exec(&request, &cancel).expect("exec");
    assert_eq!(
        killed.outcome(),
        Outcome::Killed,
        "the library and the binary hold no tests, and passing over them is what lets \
         the one target that does hold tests answer"
    );
    assert_eq!(killed.target, "fixture-subprocess/test/through_the_binary");

    let control = session
        .control(&request, &cancel, Observing::Nothing)
        .expect("control")
        .result;
    assert_eq!(
        control.outcome(),
        Outcome::Inconclusive,
        "survival is a universal claim over the selected targets: a sibling that ran no tests \
         cannot be erased by another target passing"
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
    use rust_mutants::trace::{MemorySink, Recorder, Sink};

    let fixture = Fixture::copy("fixture-coverage");
    let recorder = Recorder::wall(
        Sink::Memory(MemorySink::unbounded()),
        rust_mutants::testkit::trace::standalone_context(),
    );
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            trace: recorder.clone(),
            ..opening(&njutest_devkit::paths::cargo_binary(), fixture.temp())
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
    recorder
        .run_end(rust_mutants::trace::RunOutcome::Completed, None)
        .expect("trace closes");
    let events = recorder.events();

    let phases: Vec<(String, bool)> = events
        .iter()
        .filter_map(|event| match relevant_payload(&event.payload) {
            RelevantPayload::PhaseStart(phase) => Some((phase.name.clone(), false)),
            RelevantPayload::PhaseEnd(phase) => Some((phase.name.clone(), true)),
            RelevantPayload::ValidateRound(_)
            | RelevantPayload::MutantExec(_)
            | RelevantPayload::Other => None,
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
        !route.reaching().is_empty()
            || route.granularity() == rust_mutants::session::Granularity::Unreached,
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
            name.to_str()
                .expect("cargo environment names are exact UTF-8")
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
            std::fs::metadata(path)
                .expect("package binary metadata")
                .is_file(),
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
    let refused = workspace.prepare(&PrepareOptions::default(), &Cancel::new());
    assert_eq!(
        result_state(&refused),
        Refused,
        "a tree that does not link is not measurable"
    );
    let Err(refused) = refused else { return };
    let refused = refused.to_string();
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
        njutest_devkit::strictjson::decode_str(&text).expect("and reads back");
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
    let counted = session.locate_all(&rust_mutants::session::Locator {
        count: Some(2),
        ..one
    });
    assert!(
        matches!(
            &counted,
            Err(rust_mutants::session::LocateError::Counted { .. })
        ),
        "a claim written for two mutations is not a claim about this one: {counted:?}"
    );
    let Err(rust_mutants::session::LocateError::Counted {
        wanted,
        display_ids,
    }) = counted
    else {
        return;
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
                std::str::from_utf8(&mutant.candidate.original)
                    .expect("fixture source is exact UTF-8")
                    .to_owned(),
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
        under: rust_mutants::run::Where::default(),
    };
    let judged = |at: usize, outcome: Outcome| {
        let mutant = session
            .catalog()
            .by_index(indices[at])
            .expect("a mutant the catalog holds");
        rust_mutants::run::Judged {
            index: mutant.index,
            id: mutant.id.to_string(),
            display_id: mutant.display_id.to_string(),
            outcome,
            target: String::new(),
            exit_code: 0,
            start_failure: None,
            duration: std::time::Duration::ZERO,
            tests_run: None,
            failed_tests: Vec::new(),
            signal: None,
            retried: false,
            lingered: false,
            expected: false,
            not_run_reason: None,
            route: None,
            measured: true,
            identical: rust_mutants::run::CodegenIdentity::NotMeasured,
            source_run_id: None,
            step_notice: None,
        }
    };

    let mut every: Vec<rust_mutants::run::Judged> = (0..indices.len())
        .map(|at| judged(at, Outcome::Survived))
        .collect();
    let held = rust_mutants::run::verify(
        &session,
        std::slice::from_ref(&expectation),
        &mut every,
        rust_mutants::run::Scope::WHOLE,
    )
    .expect("the small expectation set is representable");
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
    assert_eq!(
        held[0].mutant.as_deref(),
        every.first().map(|one| one.id.as_str()),
        "a claim that held names the first of the mutations it covers, so two runs of one \
         catalog write the same report"
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
        rust_mutants::run::Scope::WHOLE,
    )
    .expect("the small expectation set is representable");
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

    let uncounted = rust_mutants::run::Expectation {
        locator: expectation
            .locator
            .clone()
            .map(|locator| rust_mutants::session::Locator {
                count: None,
                ..locator
            }),
        ..expectation
    };
    let mut again = every.clone();
    for one in &mut again {
        one.expected = false;
    }
    let unnamed = rust_mutants::run::verify(
        &session,
        std::slice::from_ref(&uncounted),
        &mut again,
        rust_mutants::run::Scope {
            shard: None,
            narrowed: false,
        },
    )
    .expect("the small expectation set is representable");
    assert!(
        matches!(
            unnamed[0].standing,
            rust_mutants::run::Standing::Unmatched { .. }
        ),
        "a locator that says nothing about how many it names is a claim about one, and resolving \
         it to one of several would exempt a mutation nobody wrote a reason for: {:?}",
        unnamed[0].standing
    );
    assert!(
        again.iter().all(|one| !one.expected),
        "and it accounts for none of them"
    );
    assert_eq!(
        unnamed[0].covered, 0,
        "a claim that named nothing was resolved against nothing"
    );

    session.close().expect("the session closes");
}

#[test]
fn the_packages_a_session_measures_are_each_named_once_and_in_one_order() {
    let fixture = Fixture::copy("fixture-workspace");
    let session = prepare(&fixture);

    let named = session.packages();
    let mut once: Vec<&String> = named.iter().collect();
    once.sort();
    once.dedup();
    assert_eq!(
        once.len(),
        named.len(),
        "a package is where a reader looks for a test to write, and one named as many times as \
         it holds mutants is a list nobody can read: {named:?}"
    );
    let mut ordered = named.clone();
    ordered.sort();
    assert_eq!(
        named, ordered,
        "and the order is the one two runs of this catalog agree on: {named:?}"
    );
    assert!(
        named.len() > 1,
        "this fixture is the one with more than one package in it: {named:?}"
    );
    session.close().expect("the session closes");
}

#[test]
fn a_mutant_in_a_file_the_session_holds_no_source_for_has_no_position() {
    let fixture = Fixture::copy("fixture-simple");
    let session = prepare(&fixture);

    let held = session
        .catalog()
        .mutants()
        .first()
        .expect("a mutant the catalog holds")
        .clone();
    assert!(
        session.position(&held).is_some(),
        "the mutant the catalog holds is in a file the session kept"
    );

    let mut elsewhere = held.clone();
    elsewhere.candidate.path = "src/never_copied.rs".to_owned();
    assert_eq!(
        session.position(&elsewhere),
        None,
        "a session that kept no source for a file cannot say where in it an edit is, and \
         answering anyway would put a line number on a file nobody has"
    );

    let mut past = held;
    past.candidate.span.start = u32::MAX;
    assert!(
        session.position(&past).is_some(),
        "an offset past the end of a file is still a place in it: the answer is the end rather \
         than nothing, because a caller asking where an edit is has one"
    );
    session.close().expect("the session closes");
}

#[test]
fn the_questions_a_prepared_session_answers_about_itself_are_answered() {
    let fixture = Fixture::copy("fixture-annotated");
    let session = prepare(&fixture);

    assert!(
        !session.claims().is_empty(),
        "this fixture carries the markers a reader writes beside the code, and a session that \
         answers with none reports every one of them as hiding nothing"
    );
    assert_eq!(
        session.workspace_digest().len(),
        64,
        "the tree a run says it measured is named by the digest of the tree it instrumented: {}",
        session.workspace_digest()
    );
    let first = session
        .catalog()
        .mutants()
        .first()
        .expect("a mutant the catalog holds");
    assert!(
        session
            .package_of(first.index)
            .is_some_and(|named| !named.is_empty()),
        "and a mutation belongs to the package a reader goes to for a test to write"
    );
    assert!(
        !session.packages().is_empty(),
        "which is one of the packages the session says it measures: {:?}",
        session.packages()
    );
    session.close().expect("the session closes");
}

#[test]
fn a_mutation_reaches_the_documentation_of_its_own_library_and_no_other() {
    let documented = Fixture::copy("fixture-doctest");
    let session = prepare(&documented);
    let doc: Vec<&str> = session
        .targets()
        .iter()
        .filter(|target| target.kind == rust_mutants::execute::TargetKind::Doc)
        .map(|target| target.id.as_str())
        .collect();
    assert_eq!(
        doc.len(),
        1,
        "this fixture documents its library with examples: {doc:?}"
    );
    let reaching_doc = session
        .catalog()
        .mutants()
        .iter()
        .filter(|mutant| {
            let route = session.route(mutant);
            doc.iter().any(|id| route.reaching().contains(id))
        })
        .count();
    assert!(
        reaching_doc > 0,
        "a documented example is compiled by rustdoc while cargo runs it, so no measurement \
         names it and every mutation of the library it is written in reaches it"
    );
    session.close().expect("the session closes");

    let undocumented = Fixture::copy("fixture-workspace");
    let session = prepare(&undocumented);
    let empty: Vec<&str> = session
        .targets()
        .iter()
        .filter(|target| {
            target.kind == rust_mutants::execute::TargetKind::Doc
                && target
                    .limitations
                    .iter()
                    .any(|one| one == rust_mutants::limitation::DOCTESTS_NONE)
        })
        .map(|target| target.id.as_str())
        .collect();
    assert_eq!(
        empty.len(),
        1,
        "and this one has a library that documents no example: {empty:?}"
    );
    for mutant in session.catalog().mutants() {
        let route = session.route(mutant);
        assert!(
            !empty.iter().any(|id| route.reaching().contains(id)),
            "which has nothing to run, so nothing is routed to it: {} reached {:?}",
            mutant.display_id,
            route.reaching()
        );
    }
    session.close().expect("the session closes");
}

/// How many targets the coverage phase says it measured, from the note it leaves.
fn targets_measured_under_coverage(skip_targets: Vec<String>) -> usize {
    use rust_mutants::trace::{MemorySink, Recorder, Sink};

    let fixture = Fixture::copy("fixture-coverage");
    let recorder = Recorder::wall(
        Sink::Memory(MemorySink::unbounded()),
        rust_mutants::testkit::trace::standalone_context(),
    );
    let workspace = Workspace::open(
        fixture.root(),
        OpenOptions {
            trace: recorder.clone(),
            ..opening(&njutest_devkit::paths::cargo_binary(), fixture.temp())
        },
        &Cancel::new(),
    )
    .expect("open");
    let session = workspace
        .prepare(
            &PrepareOptions {
                coverage: true,
                skip_targets,
                ..PrepareOptions::default()
            },
            &Cancel::new(),
        )
        .expect("prepare");
    session.close().expect("close");
    recorder
        .run_end(rust_mutants::trace::RunOutcome::Completed, None)
        .expect("trace closes");
    recorder
        .events()
        .iter()
        .find_map(|event| match &event.payload {
            Payload::Note { note } if note.kind == "coverage" => {
                match note.detail.split_whitespace().next() {
                    Some(count) => match count.parse::<usize>() {
                        Ok(measured) => Some(measured),
                        Err(_the_note_does_not_begin_with_a_count) => None,
                    },
                    None => None,
                }
            }
            _ => None,
        })
        .expect("the coverage phase says how many targets it measured")
}

#[test]
fn a_target_the_configuration_skips_is_not_started_by_the_coverage_measurement() {
    let every = targets_measured_under_coverage(Vec::new());
    assert!(
        every > 1,
        "the fixture has to have more than one target for skipping one to be visible: {every}"
    );
    let fewer = targets_measured_under_coverage(vec!["fixture-coverage/test/upper".to_owned()]);
    assert_eq!(
        fewer,
        every - 1,
        "`docs/limitations.md` says a target named in `[execution] skip_targets` is never \
         started. The reachability measurement builds its own target list from the same \
         build messages and runs before the skip list is applied, so a target somebody \
         took out ran anyway -- once, to the end, under coverage"
    );
}

#[test]
fn a_claim_on_a_file_no_unit_compiled_is_inapplicable_and_one_that_names_nothing_is_unmatched() {
    let fixture = Fixture::copy("fixture-uncompiled");
    let session = prepare(&fixture);
    let claim = |path: &str, item: &str, original: &str| rust_mutants::run::Expectation {
        id: None,
        locator: Some(rust_mutants::session::Locator {
            path: path.to_owned(),
            item: item.to_owned(),
            rule: "return-default".to_owned(),
            original: original.to_owned(),
            line: None,
            count: None,
        }),
        reason: "a number nothing reads".to_owned(),
        outcome: Outcome::Survived,
        under: rust_mutants::run::Where::default(),
    };
    let verified = rust_mutants::run::verify(
        &session,
        &[
            claim("src/elsewhere.rs", "seven", "7"),
            claim("src/elsewhere.rs", "eight", "8"),
            claim("src/nowhere.rs", "seven", "7"),
        ],
        &mut [],
        rust_mutants::run::Scope {
            shard: None,
            narrowed: false,
        },
    )
    .expect("three claims are representable");
    assert!(
        matches!(
            verified[0].standing,
            rust_mutants::run::Standing::Inapplicable {
                because: rust_mutants::run::Unheld::NotCompiled
            }
        ),
        "a claim on a module this build never compiles, as one gated to another platform, is not \
         judged here rather than unmatched: {:?}",
        verified[0].standing
    );
    for (at, what) in [
        (1, "a locator that names nothing in the file"),
        (2, "a file that is not there"),
    ] {
        assert!(
            matches!(
                verified[at].standing,
                rust_mutants::run::Standing::Unmatched { .. }
            ),
            "{what} is unmatched on every host, so a locator that rotted is never silent: {:?}",
            verified[at].standing
        );
    }
    session.close().expect("the session closes");
}

#[test]
fn a_claim_is_judged_only_where_the_facts_it_names_hold() {
    let fixture = Fixture::copy("fixture-uncompiled");
    let session = prepare(&fixture);
    let here = if cfg!(unix) { "unix" } else { "windows" };
    let elsewhere = if cfg!(unix) { "windows" } else { "unix" };
    let claim = |under: rust_mutants::run::Where| rust_mutants::run::Expectation {
        id: None,
        locator: Some(rust_mutants::session::Locator {
            path: "src/lib.rs".to_owned(),
            item: "max".to_owned(),
            rule: "condition-to-true".to_owned(),
            original: "a > b".to_owned(),
            line: None,
            count: None,
        }),
        reason: "a claim about one platform's behaviour".to_owned(),
        outcome: Outcome::Survived,
        under,
    };
    let cfg = |text: &str| rust_mutants::run::Where {
        cfg: Some(rust_mutants::facts::Predicate::parse(text).expect("a predicate")),
        env: std::collections::BTreeMap::new(),
    };
    let absent = rust_mutants::run::Where {
        cfg: None,
        env: std::collections::BTreeMap::from([(
            "RUST_MUTANTS_ADR_0042_NEVER_SET".to_owned(),
            "1".to_owned(),
        )]),
    };
    let verified = rust_mutants::run::verify(
        &session,
        &[claim(cfg(here)), claim(cfg(elsewhere)), claim(absent)],
        &mut [],
        rust_mutants::run::Scope {
            shard: None,
            narrowed: false,
        },
    )
    .expect("three claims are representable");
    assert!(
        !matches!(
            verified[0].standing,
            rust_mutants::run::Standing::Inapplicable { .. }
        ),
        "cfg({here}) holds of this host's target, so the claim is judged: {:?}",
        verified[0].standing
    );
    assert!(
        matches!(
            &verified[1].standing,
            rust_mutants::run::Standing::Inapplicable {
                because: rust_mutants::run::Unheld::Cfg { predicate }
            } if predicate == elsewhere
        ),
        "cfg({elsewhere}) does not hold here, so the claim is not judged here: {:?}",
        verified[1].standing
    );
    assert!(
        matches!(
            &verified[2].standing,
            rust_mutants::run::Standing::Inapplicable {
                because: rust_mutants::run::Unheld::Env { name, found: None, .. }
            } if name == "RUST_MUTANTS_ADR_0042_NEVER_SET"
        ),
        "a claim that holds under a variable the tests are not given is not judged: {:?}",
        verified[2].standing
    );
    session.close().expect("the session closes");
}
