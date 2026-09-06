// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Validation: the compiler decides which mutants are real, one at a time, with its own words attached to every refusal.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::BTreeSet;

use rust_mutants::cargo::Message;
use rust_mutants::instrument::instrument_file;
use rust_mutants::rule::Tier;
use rust_mutants::testkit::compile::{ScriptedCompile, diagnostic_at};
use rust_mutants::trace::Recorder;
use rust_mutants::validate::{
    Attempt, Compile, ValidateError, ValidateOptions, Validated, attribute, validate,
};

fn options() -> ValidateOptions {
    ValidateOptions::default()
}

fn scripted(attributable: &[u32], unattributable: &[u32]) -> ScriptedCompile {
    ScriptedCompile::from_source("src/lib.rs", SOURCE, Tier::All)
        .refusing(attributable, unattributable)
}

fn run(scripted: &mut ScriptedCompile) -> Result<Validated, ValidateError> {
    let catalog = scripted.catalog().clone();
    validate(&catalog, scripted, options(), &Recorder::disabled())
}

const SOURCE: &str = "pub fn f(a: i32, b: i32) -> i32 {\n    let c = a + b;\n    let d = a - b;\n    let e = a * b;\n    c + d + e\n}\n";

#[test]
fn an_error_inside_a_branch_belongs_to_that_mutant_and_one_outside_belongs_to_nobody() {
    let scripted = scripted(&[], &[]);
    let file = instrument_file(
        "src/lib.rs",
        scripted.source(),
        scripted.placements(),
        scripted.catalog().digest(),
    )
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
    let file = instrument_file(
        "src/lib.rs",
        scripted.source(),
        scripted.placements(),
        scripted.catalog().digest(),
    )
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
            })
        }
    }
    let scripted = scripted(&[], &[]);
    let error = validate(
        scripted.catalog(),
        &mut Broken,
        options(),
        &Recorder::disabled(),
    )
    .unwrap_err();
    assert!(
        matches!(error, ValidateError::NotMutantInduced { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("RM4001"), "{error}");
}
