// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the seams a run watched say the system does, and who is holding each sentence up.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking"
)]
use njutest_cli::config::Contract;
use njutest_cli::report::spec::{page, spoken};
use njutest_cli::report::{BuildReport, Report, RunKind, SeamDecision, SeamRecord};

/// A report of a run that watched seams and nothing else.
fn watched(seams: Vec<SeamRecord>) -> Report {
    let mut source = BuildReport::new("source-one", RunKind::Full, Contract::StandardV1);
    "workspace".clone_into(&mut source.repository.root_name);
    source.repository.workspace_digest = "b".repeat(64);
    source.repository.configuration_digest = "c".repeat(64);
    "rustc 1.98.0".clone_into(&mut source.toolchain.rustc);
    source.scope.configured_builds = vec![njutest_cli::config::DEFAULT_CONFIGURATION.to_owned()];
    "2026-01-01T00:00:00Z".clone_into(&mut source.timing.started);
    "2026-01-01T00:00:00Z".clone_into(&mut source.timing.finished);
    source
        .limitations
        .push(njutest_cli::report::Limitation::new(
            "git-metadata-unavailable",
            "the seam fixture is not a git repository",
        ));
    source.seams = seams;
    let measurements = njutest_cli::report::across::BuildMeasurements::checked(vec![(
        njutest_cli::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        source,
    )])
    .expect("one checked build measurement");
    let final_run = rust_mutants::id::RunId::try_from("watched").expect("a canonical run id");
    let latticed = njutest_cli::report::across::configured(&final_run, &measurements)
        .expect("one checked complete lattice");
    let njutest_cli::report::LatticedDocument::Complete(latticed) = latticed else {
        panic!("the whole-catalog seam fixture cannot be a shard");
    };
    latticed
        .complete_without_models()
        .expect("standard-v1 needs no model completion")
}

/// One question about `POST /orders`, decided as `decision` says.
fn question(seq: u64, rule: &str, decision: SeamDecision) -> SeamRecord {
    SeamRecord {
        id: format!("{seq}{rule}"),
        capability: "payments".to_owned(),
        seq,
        asked: "POST /orders".to_owned(),
        answered: Some(201),
        rule: njutest_cli::wire::rule::Rule::parse(rule)
            .unwrap_or(njutest_cli::wire::rule::Rule::DropConnection),
        decision,
    }
}

#[test]
fn every_exchange_the_run_watched_is_one_sentence_however_many_questions_it_licensed() {
    let report = watched(vec![
        question(
            0,
            "drop-connection",
            SeamDecision::Tests {
                noticed_by: "pkg/test/orders".to_owned(),
            },
        ),
        question(0, "status-server-error", SeamDecision::Unnoticed),
        question(
            0,
            "truncate-response",
            SeamDecision::Proved {
                proof: "no-body-to-cut".to_owned(),
            },
        ),
    ]);
    let sentences = spoken(&report).expect("the sentences derive");
    assert_eq!(
        sentences.len(),
        1,
        "the sentence is about what the system did, and one round trip is one thing \
         it did however many ways a run found to perturb it"
    );
}

#[test]
fn a_sentence_nothing_would_notice_changing_says_so_rather_than_saying_nothing() {
    let report = watched(vec![question(
        0,
        "status-server-error",
        SeamDecision::Unnoticed,
    )]);
    let said = spoken(&report)
        .expect("the sentences derive")
        .first()
        .map(njutest_cli::report::spec::Sentence::worded)
        .expect("a sentence");
    assert!(
        said.contains("POST /orders") && said.contains("nobody holds this up"),
        "this is the sentence the whole layer exists to be able to say to a reviewer, \
         and a page that only listed what the system does would be a page nobody \
         needed: {said}"
    );
}

#[test]
fn a_question_a_proof_discharged_leaves_the_sentence_neither_held_up_nor_wanting() {
    let report = watched(vec![question(
        0,
        "truncate-response",
        SeamDecision::Proved {
            proof: "no-body-to-cut".to_owned(),
        },
    )]);
    let sentence = spoken(&report)
        .expect("the sentences derive")
        .into_iter()
        .next()
        .expect("a sentence");
    assert!(!sentence.is_guarded(), "no test held it up");
    assert_eq!(
        (sentence.unguarded, sentence.unasked),
        (0, 0),
        "nobody could have noticed it, so counting it against the tests would ask \
         them for something no test can give"
    );
}

#[test]
fn a_question_the_run_could_not_put_is_counted_apart_from_one_nothing_noticed() {
    let report = watched(vec![
        question(0, "status-server-error", SeamDecision::Unnoticed),
        question(0, "stale-response", SeamDecision::Unreached),
    ]);
    let sentence = spoken(&report)
        .expect("the sentences derive")
        .into_iter()
        .next()
        .expect("a sentence");
    assert_eq!(
        (sentence.unguarded, sentence.unasked),
        (1, 1),
        "a gap the tests could close and a question nobody asked are two different \
         things to do about it, and a page that added them up would tell a reader to \
         write a test for something no run put to them"
    );
    let said = sentence.worded();
    assert!(
        said.contains("nothing noticed") && said.contains("could not put"),
        "{said}"
    );
}

#[test]
fn one_target_that_noticed_several_questions_is_named_once() {
    let report = watched(vec![
        question(
            0,
            "drop-connection",
            SeamDecision::Tests {
                noticed_by: "pkg/test/orders".to_owned(),
            },
        ),
        question(
            0,
            "delay-response",
            SeamDecision::Tests {
                noticed_by: "pkg/test/orders".to_owned(),
            },
        ),
    ]);
    let sentence = spoken(&report)
        .expect("the sentences derive")
        .into_iter()
        .next()
        .expect("a sentence");
    assert_eq!(sentence.guarded_by, vec!["pkg/test/orders".to_owned()]);
}

#[test]
fn a_run_that_watched_no_seam_says_it_watched_none_rather_than_printing_a_blank_page() {
    let said = page(&watched(Vec::new())).expect("the page derives");
    assert!(
        said.contains("watched no seam"),
        "an empty page reads as a system that does nothing, which is the one thing \
         it cannot mean: {said}"
    );
}

#[test]
fn the_page_says_how_many_of_the_sentences_nothing_would_notice_changing() {
    let report = watched(vec![
        question(0, "status-server-error", SeamDecision::Unnoticed),
        question(
            1,
            "drop-connection",
            SeamDecision::Tests {
                noticed_by: "pkg/test/orders".to_owned(),
            },
        ),
    ]);
    let said = page(&report).expect("the page derives");
    assert!(
        said.contains("2 observed, 1 that nothing would notice changing."),
        "the number a reviewer acts on is the second one: {said}"
    );
}

#[test]
fn an_exchange_nothing_was_asked_about_is_not_counted_among_the_ones_nothing_noticed() {
    let report = watched(vec![question(0, "stale-response", SeamDecision::Unreached)]);
    let said = page(&report).expect("the page derives");
    assert!(
        said.contains("0 that nothing would notice changing")
            && said.contains("1 this run established nothing about"),
        "the run put no question anybody could answer, so the tests were never given \
         the chance. Counting it among the gaps tells a reviewer the suite is blind \
         where the run established nothing at all, which is the one reading this page \
         must never invite: {said}"
    );
}
