// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Validation: the compiler decides which mutants are real, one at a time, with its own words attached to every refusal.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::{BTreeMap, BTreeSet};

use rust_mutants::cargo::Message;
use rust_mutants::instrument::{Instrumenting, instrument_file};
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::testkit::compile::{
    ScriptedCompile, diagnostic_at, diagnostic_beside, diagnostic_noted,
};
use rust_mutants::trace::Recorder;
use rust_mutants::validate::{
    Attempt, Compile, ValidateError, ValidateOptions, Validated, Validating, attribute, validate,
    validate_selected,
};

fn options() -> ValidateOptions {
    ValidateOptions::default()
}

fn validating<'a>(cancel: &'a Cancel, trace: &'a Recorder) -> Validating<'a> {
    Validating {
        options: options(),
        cancel,
        trace,
    }
}

fn scripted(attributable: &[u32], unattributable: &[u32]) -> ScriptedCompile {
    ScriptedCompile::from_source("src/lib.rs", SOURCE, Tier::All)
        .refusing(attributable, unattributable)
}

fn run(scripted: &mut ScriptedCompile) -> Result<Validated, ValidateError> {
    let catalog = scripted.catalog().clone();
    validate(
        &catalog,
        scripted,
        &validating(&Cancel::new(), &Recorder::disabled()),
    )
}

const SOURCE: &str = "pub fn f(a: i32, b: i32) -> i32 {\n    let c = a + b;\n    let d = a - b;\n    let e = a * b;\n    c + d + e\n}\n";

#[test]
fn an_error_inside_a_branch_belongs_to_that_mutant_and_one_outside_belongs_to_nobody() {
    let scripted = scripted(&[], &[]);
    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: scripted.source(),
        placements: scripted.placements(),
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: scripted.catalog().digest(),
    })
    .expect("instrument");
    let branch = file.branches[2];
    let messages = vec![
        diagnostic_at(
            "src/lib.rs",
            branch.span.start,
            branch.span.end,
            branch.index,
        ),
        diagnostic_at("src/lib.rs", 0, 1, 999),
        diagnostic_at("src/other.rs", branch.span.start, branch.span.end, 7),
    ];
    let attributed = attribute(&[file], &messages);
    assert_eq!(attributed.condemned, BTreeSet::from([branch.index]));
    assert_eq!(
        attributed.unattributed.len(),
        2,
        "{:?}",
        attributed.unattributed
    );
    assert!(
        attributed.diagnostics[&branch.index].contains("does not compile"),
        "the compiler's own words are kept"
    );
}

#[test]
fn a_warning_is_not_a_rejection() {
    let scripted = scripted(&[], &[]);
    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: scripted.source(),
        placements: scripted.placements(),
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: scripted.catalog().digest(),
    })
    .expect("instrument");
    let branch = file.branches[0];
    let warning = diagnostic_at(
        "src/lib.rs",
        branch.span.start,
        branch.span.end,
        branch.index,
    );
    let Message::CompilerMessage(mut message) = warning else {
        panic!("a compiler message");
    };
    message.message.level = "warning".to_owned();
    let attributed = attribute(&[file], &[Message::CompilerMessage(message)]);
    assert!(attributed.condemned.is_empty());
    assert!(attributed.unattributed.is_empty());
}

#[test]
fn a_tree_that_compiles_is_accepted_whole_in_one_round() {
    let mut scripted = scripted(&[], &[]);
    let validated = run(&mut scripted).expect("validate");
    assert_eq!(validated.rounds, 1);
    assert!(validated.rejections.is_empty());
    assert_eq!(
        validated.accepted.len(),
        scripted.catalog().len(),
        "every mutant is accepted"
    );
    assert_eq!(scripted.attempts(), [BTreeSet::new()]);
}

#[test]
fn a_selected_validation_claims_only_the_indices_it_was_asked_about() {
    let mut scripted = scripted(&[1], &[]);
    let catalog = scripted.catalog().clone();
    let selected = BTreeSet::from([1, 3]);
    let validated = validate_selected(
        &catalog,
        &selected,
        &mut scripted,
        &validating(&Cancel::new(), &Recorder::disabled()),
    )
    .expect("validate the selected candidates");

    assert_eq!(validated.accepted, [3]);
    assert_eq!(
        validated
            .rejections
            .iter()
            .map(|rejection| rejection.index)
            .collect::<Vec<_>>(),
        [1]
    );
    assert!(
        validated
            .accepted
            .iter()
            .chain(
                validated
                    .rejections
                    .iter()
                    .map(|rejection| &rejection.index)
            )
            .all(|index| selected.contains(index)),
        "validation must make no claim about a candidate outside the requested selection"
    );
}

#[test]
fn an_empty_selected_validation_still_compiles_the_pristine_tree_once() {
    let mut scripted = scripted(&[], &[]);
    let catalog = scripted.catalog().clone();
    let validated = validate_selected(
        &catalog,
        &BTreeSet::new(),
        &mut scripted,
        &validating(&Cancel::new(), &Recorder::disabled()),
    )
    .expect("the build gate still runs");

    assert!(validated.accepted.is_empty());
    assert!(validated.rejections.is_empty());
    assert_eq!(scripted.attempts(), [BTreeSet::new()]);
}

#[test]
fn an_attributable_error_condemns_one_mutant_and_costs_one_more_round() {
    let mut scripted = scripted(&[1, 4], &[]);
    let validated = run(&mut scripted).expect("validate");
    assert_eq!(validated.rounds, 2, "both are attributed in the same round");
    let rejected: Vec<u32> = validated
        .rejections
        .iter()
        .map(|rejection| rejection.index)
        .collect();
    assert_eq!(rejected, [1, 4]);
    assert!(!validated.accepted.contains(&1) && !validated.accepted.contains(&4));
    assert_eq!(
        validated.accepted.len(),
        scripted.catalog().len() - 2,
        "a refusal never costs a sibling"
    );
    let rejection = &validated.rejections[0];
    assert_eq!(rejection.code.as_deref(), Some("E0999"));
    assert!(rejection.diagnostic.contains("mutant 1 does not compile"));
    assert_eq!(rejection.path, "src/lib.rs");
    assert_eq!(
        rejection.id,
        scripted.catalog().by_index(1).expect("mutant").id
    );
    assert_eq!(
        scripted.attempts(),
        [BTreeSet::new(), BTreeSet::from([1, 4])]
    );
}

#[test]
fn an_unattributable_error_is_isolated_by_bisection() {
    let mut scripted = scripted(&[], &[3]);
    let validated = run(&mut scripted).expect("validate");
    let rejected: Vec<u32> = validated
        .rejections
        .iter()
        .map(|rejection| rejection.index)
        .collect();
    assert_eq!(rejected, [3]);
    assert!(validated.bisections > 0, "isolation was needed");
    assert!(
        scripted.attempts().len() > 2,
        "bisection costs compilations: {:?}",
        scripted.attempts()
    );
    assert!(!validated.accepted.contains(&3));
    assert_eq!(validated.accepted.len(), scripted.catalog().len() - 1);
}

#[test]
fn a_pristine_tree_that_does_not_compile_is_not_the_mutants_fault() {
    struct Broken;
    impl Compile for Broken {
        fn attempt(&mut self, _condemned: &BTreeSet<u32>) -> Result<Attempt, ValidateError> {
            Ok(Attempt {
                files: Vec::new(),
                messages: vec![
                    diagnostic_at("src/lib.rs", 0, 1, 0),
                    Message::BuildFinished { success: false },
                ],
                success: false,
                written: 0,
            })
        }
    }
    let scripted = scripted(&[], &[]);
    let error = validate(
        scripted.catalog(),
        &mut Broken,
        &validating(&Cancel::new(), &Recorder::disabled()),
    )
    .unwrap_err();
    assert!(
        matches!(error, ValidateError::NotMutantInduced { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("RM4001"), "{error}");
}

#[test]
fn a_cancelled_round_ends_validation_with_the_cancellation_code() {
    let cancel = Cancel::new();
    let mut scripted = ScriptedCompile::from_source("src/lib.rs", SOURCE, Tier::All)
        .refusing(&[0], &[])
        .cancelling_at(2, &cancel);
    let catalog = scripted.catalog().clone();
    let error = validate(
        &catalog,
        &mut scripted,
        &validating(&cancel, &Recorder::disabled()),
    )
    .expect_err("a cancelled validation does not finish");
    assert_eq!(error.code().code, "RM0001");
    assert_eq!(
        scripted.attempts().len(),
        2,
        "the round that was cancelled is the last one, and no bisection follows it"
    );
}

#[test]
fn a_cancelled_compilation_condemns_nobody() {
    let cancel = Cancel::new();
    let mut scripted = ScriptedCompile::from_source("src/lib.rs", SOURCE, Tier::All)
        .refusing(&[], &[0])
        .cancelling_at(1, &cancel);
    let catalog = scripted.catalog().clone();
    let error = validate(
        &catalog,
        &mut scripted,
        &validating(&cancel, &Recorder::disabled()),
    )
    .expect_err("a cancelled validation does not finish");
    assert!(
        matches!(error, ValidateError::Cancelled),
        "a build nobody waited for is not a build that failed, and reading it as one condemns \
         mutants the compiler never refused: {error:?}"
    );
}

#[test]
fn a_diagnostic_whose_primary_span_is_elsewhere_is_attributed_through_its_secondary_span() {
    let scripted = scripted(&[], &[]);
    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: scripted.source(),
        placements: scripted.placements(),
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: scripted.catalog().digest(),
    })
    .expect("instrument");
    let branch = file.branches[1];
    let attributed = attribute(
        &[file],
        &[diagnostic_beside(
            "src/lib.rs",
            branch.span.start,
            branch.span.end,
            branch.index,
        )],
    );
    assert_eq!(
        attributed.condemned,
        BTreeSet::from([branch.index]),
        "the compiler points at the place it decided, which for a type error is often the \
         definition; the edit it is about is named by another span of the same message"
    );
    assert!(attributed.unattributed.is_empty());
}

#[test]
fn a_diagnostic_whose_edit_is_named_only_by_a_child_note_is_attributed_through_it() {
    let scripted = scripted(&[], &[]);
    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: scripted.source(),
        placements: scripted.placements(),
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: scripted.catalog().digest(),
    })
    .expect("instrument");
    let branch = file.branches[2];
    let attributed = attribute(
        &[file],
        &[diagnostic_noted(
            "src/lib.rs",
            branch.span.start,
            branch.span.end,
            branch.index,
        )],
    );
    assert_eq!(attributed.condemned, BTreeSet::from([branch.index]));
    assert!(attributed.unattributed.is_empty());
}

#[test]
fn a_diagnostic_that_names_no_branch_anywhere_still_belongs_to_nobody() {
    let scripted = scripted(&[], &[]);
    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: scripted.source(),
        placements: scripted.placements(),
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: scripted.catalog().digest(),
    })
    .expect("instrument");
    let attributed = attribute(&[file], &[diagnostic_beside("src/lib.rs", 0, 1, 999)]);
    assert!(
        attributed.condemned.is_empty(),
        "reading more spans widens what can be attributed, never what is guessed"
    );
    assert_eq!(attributed.unattributed.len(), 1);
}

#[test]
fn each_isolated_offender_is_compiled_alone_once_to_capture_its_own_diagnostic() {
    let mut scripted = scripted(&[], &[1, 3]);
    let validated = run(&mut scripted).expect("validated");
    let mut refused: Vec<&rust_mutants::validate::Rejection> =
        validated.rejections.iter().collect();
    refused.sort_by_key(|one| one.index);
    let indices: Vec<u32> = refused.iter().map(|one| one.index).collect();
    assert_eq!(indices, [1, 3]);
    for one in &refused {
        assert!(
            one.isolated,
            "bisection compiled it alone and it still failed, which is what isolated means"
        );
        assert!(
            one.diagnostic.contains(&format!("mutant {} ", one.index)),
            "an offender bisection named carries the compiler's words about itself, not a \
             sentence saying there were none: {}",
            one.diagnostic
        );
        assert_eq!(one.code.as_deref(), Some("E0999"));
    }
}

#[test]
fn an_interaction_of_two_mutants_condemns_the_pair_and_says_so() {
    let mut scripted =
        ScriptedCompile::from_source("src/lib.rs", SOURCE, Tier::All).interacting(&[1, 3]);
    let validated = run(&mut scripted).expect("validated");
    let mut indices: Vec<u32> = validated.rejections.iter().map(|one| one.index).collect();
    indices.sort_unstable();
    assert_eq!(
        indices,
        [1, 3],
        "neither half fails alone, so the pair is what the compiler refused"
    );
    for one in &validated.rejections {
        assert!(
            !one.isolated,
            "nothing was isolated: each of them compiles on its own"
        );
        let other = if one.index == 1 { "3" } else { "1" };
        assert!(
            one.diagnostic
                .contains(&format!("together with {other}, and each of them compiles")),
            "a mutant condemned for what it does with another is told which one: {}",
            one.diagnostic
        );
        assert!(
            one.diagnostic.contains("does not compile"),
            "and keeps the compiler's own words about the pair: {}",
            one.diagnostic
        );
    }
}

#[test]
fn a_message_before_an_error_does_not_stop_the_reading_of_the_rest() {
    let scripted = scripted(&[], &[]);
    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: scripted.source(),
        placements: scripted.placements(),
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: scripted.catalog().digest(),
    })
    .expect("instrument");
    let branch = file.branches[0];
    let at = |index: u32| diagnostic_at("src/lib.rs", branch.span.start, branch.span.end, index);
    let Message::CompilerMessage(mut warning) = at(branch.index) else {
        panic!("a compiler message");
    };
    warning.message.level = "warning".to_owned();

    let alone = attribute(std::slice::from_ref(&file), &[at(branch.index)]);
    assert_eq!(
        alone.condemned.len(),
        1,
        "the error on its own condemns the mutant it names"
    );

    let after = attribute(
        &[file],
        &[
            Message::BuildFinished { success: false },
            Message::CompilerMessage(warning),
            at(branch.index),
        ],
    );
    assert_eq!(
        after.condemned, alone.condemned,
        "and a message that is not the compiler's, and a warning, are each passed over rather \
         than ending the reading: a build that stopped at the first of them would accept every \
         mutant the compiler refused after it"
    );
}
