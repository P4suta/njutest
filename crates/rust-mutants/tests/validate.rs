// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Validation: the compiler decides which mutants are real, one at a time, with its own words attached to every refusal.

#![expect(
    clippy::indexing_slicing,
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::{BTreeMap, BTreeSet};

use njutest_devkit::result::{
    ResultState::{Refused, Returned},
    result_state,
};
use rust_mutants::cargo::Message;
use rust_mutants::instrument::{Instrumenting, instrument_file};
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::testkit::compile::{
    ScriptedCompile, diagnostic_at, diagnostic_beside, diagnostic_noted,
};
use rust_mutants::trace::Recorder;
use rust_mutants::validate::{
    Attempt, Compile, ConstFnAt, Constness, ValidateError, ValidateOptions, Validated, Validating,
    attribute, validate, validate_selected,
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
        carriers: &[],
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: scripted.catalog().digest(),
        first_item: 0,
        watched: "/watched",
    });
    assert_eq!(result_state(&file), Returned, "instrument: {file:?}");
    let Ok(file) = file else { return };
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
    let attributed = attribute(&[file], &messages, &Constness::default());
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
        carriers: &[],
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: scripted.catalog().digest(),
        first_item: 0,
        watched: "/watched",
    });
    assert_eq!(result_state(&file), Returned, "instrument: {file:?}");
    let Ok(file) = file else { return };
    let branch = file.branches[0];
    let warning = diagnostic_at(
        "src/lib.rs",
        branch.span.start,
        branch.span.end,
        branch.index,
    );
    assert!(
        matches!(&warning, Message::CompilerMessage(_)),
        "a compiler message"
    );
    let Message::CompilerMessage(mut message) = warning else {
        return;
    };
    message.message.level = "warning".to_owned();
    let attributed = attribute(
        &[file],
        &[Message::CompilerMessage(message)],
        &Constness::default(),
    );
    assert!(attributed.condemned.is_empty());
    assert!(attributed.unattributed.is_empty());
}

#[test]
fn a_tree_that_compiles_is_accepted_whole_in_one_round() {
    let mut scripted = scripted(&[], &[]);
    let validated = run(&mut scripted);
    assert_eq!(
        result_state(&validated),
        Returned,
        "validate: {validated:?}"
    );
    let Ok(validated) = validated else { return };
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
    );
    assert_eq!(
        result_state(&validated),
        Returned,
        "validate the selected candidates: {validated:?}"
    );
    let Ok(validated) = validated else { return };

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
    );
    assert_eq!(
        result_state(&validated),
        Returned,
        "the build gate still runs: {validated:?}"
    );
    let Ok(validated) = validated else { return };

    assert!(validated.accepted.is_empty());
    assert!(validated.rejections.is_empty());
    assert_eq!(scripted.attempts(), [BTreeSet::new()]);
}

#[test]
fn an_attributable_error_condemns_one_mutant_and_costs_one_more_round() {
    let mut scripted = scripted(&[1, 4], &[]);
    let validated = run(&mut scripted);
    assert_eq!(
        result_state(&validated),
        Returned,
        "validate: {validated:?}"
    );
    let Ok(validated) = validated else { return };
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
    let mutant = scripted.catalog().by_index(1);
    assert!(mutant.is_some(), "mutant 1 exists");
    let Some(mutant) = mutant else { return };
    assert_eq!(rejection.id, mutant.id.as_str());
    assert_eq!(
        scripted.attempts(),
        [BTreeSet::new(), BTreeSet::from([1, 4])]
    );
}

#[test]
fn an_unattributable_error_is_isolated_by_bisection() {
    let mut scripted = scripted(&[], &[3]);
    let validated = run(&mut scripted);
    assert_eq!(
        result_state(&validated),
        Returned,
        "validate: {validated:?}"
    );
    let Ok(validated) = validated else { return };
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
        fn attempt(
            &mut self,
            _condemned: &BTreeSet<u32>,
            _constness: &Constness,
        ) -> Result<Attempt, ValidateError> {
            Ok(Attempt {
                files: Vec::new(),
                messages: vec![
                    diagnostic_at("src/lib.rs", 0, 1, 0),
                    Message::BuildFinished(rust_mutants::cargo::Finished::new(false)),
                ],
                completion: rust_mutants::cargo::Completion::Refused,
                written: 0,
            })
        }
    }
    let scripted = scripted(&[], &[]);
    let error = validate(
        scripted.catalog(),
        &mut Broken,
        &validating(&Cancel::new(), &Recorder::disabled()),
    );
    assert_eq!(
        result_state(&error),
        Refused,
        "the pristine failure is refused: {error:?}"
    );
    let Err(error) = error else { return };
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
    );
    assert_eq!(
        result_state(&error),
        Refused,
        "a cancelled validation does not finish: {error:?}"
    );
    let Err(error) = error else { return };
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
    );
    assert_eq!(
        result_state(&error),
        Refused,
        "a cancelled validation does not finish: {error:?}"
    );
    let Err(error) = error else { return };
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
        carriers: &[],
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: scripted.catalog().digest(),
        first_item: 0,
        watched: "/watched",
    });
    assert_eq!(result_state(&file), Returned, "instrument: {file:?}");
    let Ok(file) = file else { return };
    let branch = file.branches[1];
    let attributed = attribute(
        &[file],
        &[diagnostic_beside(
            "src/lib.rs",
            branch.span.start,
            branch.span.end,
            branch.index,
        )],
        &Constness::default(),
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
        carriers: &[],
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: scripted.catalog().digest(),
        first_item: 0,
        watched: "/watched",
    });
    assert_eq!(result_state(&file), Returned, "instrument: {file:?}");
    let Ok(file) = file else { return };
    let branch = file.branches[2];
    let attributed = attribute(
        &[file],
        &[diagnostic_noted(
            "src/lib.rs",
            branch.span.start,
            branch.span.end,
            branch.index,
        )],
        &Constness::default(),
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
        carriers: &[],
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: scripted.catalog().digest(),
        first_item: 0,
        watched: "/watched",
    });
    assert_eq!(result_state(&file), Returned, "instrument: {file:?}");
    let Ok(file) = file else { return };
    let attributed = attribute(
        &[file],
        &[diagnostic_beside("src/lib.rs", 0, 1, 999)],
        &Constness::default(),
    );
    assert!(
        attributed.condemned.is_empty(),
        "reading more spans widens what can be attributed, never what is guessed"
    );
    assert_eq!(attributed.unattributed.len(), 1);
}

#[test]
fn each_isolated_offender_is_compiled_alone_once_to_capture_its_own_diagnostic() {
    let mut scripted = scripted(&[], &[1, 3]);
    let validated = run(&mut scripted);
    assert_eq!(
        result_state(&validated),
        Returned,
        "validated: {validated:?}"
    );
    let Ok(validated) = validated else { return };
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
    let validated = run(&mut scripted);
    assert_eq!(
        result_state(&validated),
        Returned,
        "validated: {validated:?}"
    );
    let Ok(validated) = validated else { return };
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
        carriers: &[],
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: scripted.catalog().digest(),
        first_item: 0,
        watched: "/watched",
    });
    assert_eq!(result_state(&file), Returned, "instrument: {file:?}");
    let Ok(file) = file else { return };
    let branch = file.branches[0];
    let at = |index: u32| diagnostic_at("src/lib.rs", branch.span.start, branch.span.end, index);
    let warning = at(branch.index);
    assert!(
        matches!(&warning, Message::CompilerMessage(_)),
        "a compiler message"
    );
    let Message::CompilerMessage(mut warning) = warning else {
        return;
    };
    warning.message.level = "warning".to_owned();

    let alone = attribute(
        std::slice::from_ref(&file),
        &[at(branch.index)],
        &Constness::default(),
    );
    assert_eq!(
        alone.condemned.len(),
        1,
        "the error on its own condemns the mutant it names"
    );

    let after = attribute(
        &[file],
        &[
            Message::BuildFinished(rust_mutants::cargo::Finished::new(false)),
            Message::CompilerMessage(warning),
            at(branch.index),
        ],
        &Constness::default(),
    );
    assert_eq!(
        after.condemned, alone.condemned,
        "and a message that is not the compiler's, and a warning, are each passed over rather \
         than ending the reading: a build that stopped at the first of them would accept every \
         mutant the compiler refused after it"
    );
}

/// Two `const fn`s of one name in two `impl`s, both holding guards, and a third that calls one and keeps its `const`.
const TWINS: &str = "pub struct A;\nimpl A {\n    pub const fn make(n: u8) -> u8 {\n        n + 1\n    }\n}\npub struct B;\nimpl B {\n    pub const fn make(n: u8) -> u8 {\n        n - 1\n    }\n}\npub const fn keeps(n: u8) -> u8 {\n    A::make(n)\n}\n";

/// [`TWINS`] instrumented with every guard but those of `keeps`, which the tests treat as refused.
fn twins() -> (
    rust_mutants::instrument::FileOutput,
    BTreeSet<u32>,
    BTreeSet<u32>,
) {
    let scripted = ScriptedCompile::from_source("src/lib.rs", TWINS, Tier::All);
    let owned = |owner: &str| -> BTreeSet<u32> {
        scripted
            .placements()
            .iter()
            .filter(|placement| {
                placement.hint.const_fn.as_ref().is_some_and(|function| {
                    function.name == "make" && function.owner.as_deref() == Some(owner)
                })
            })
            .map(|placement| placement.index)
            .collect()
    };
    let (a, b) = (owned("A"), owned("B"));
    let kept: Vec<rust_mutants::instrument::Placement> = scripted
        .placements()
        .iter()
        .filter(|placement| a.contains(&placement.index) || b.contains(&placement.index))
        .cloned()
        .collect();
    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: scripted.source(),
        placements: &kept,
        carriers: &[],
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: scripted.catalog().digest(),
        first_item: 0,
        watched: "/watched",
    })
    .expect("instrument the twins");
    (file, a, b)
}

/// A refusal of a call the compiler would have to make before the program runs, as the pinned toolchain writes one: `said` is its message, `at` the call, and `defined` the definition its note points at, which it has for a free function and not for an associated one.
fn evaluation(said: &str, at: (u32, u32), defined: Option<(u32, u32)>) -> Message {
    let note = match defined {
        Some((start, end)) => format!(
            r#"{{"message":"function is not const","code":null,"level":"note","spans":[{{"file_name":"src/lib.rs","byte_start":{start},"byte_end":{end},"line_start":1,"line_end":1,"column_start":1,"column_end":2,"is_primary":true,"text":[],"label":null}}],"children":[],"rendered":null}}"#
        ),
        None => r#"{"message":"calls in constants are limited to constant functions, tuple structs and tuple variants","code":null,"level":"note","spans":[],"children":[],"rendered":null}"#.to_owned(),
    };
    let (start, end) = at;
    let json = format!(
        r#"{{"reason":"compiler-message","package_id":"p","manifest_path":"/w/Cargo.toml","target":{{"kind":["lib"],"crate_types":["lib"],"name":"demo","src_path":"/w/src/lib.rs","edition":"2024"}},"message":{{"message":"{said}","code":{{"code":"E0015","explanation":""}},"level":"error","spans":[{{"file_name":"src/lib.rs","byte_start":{start},"byte_end":{end},"line_start":1,"line_end":1,"column_start":1,"column_end":2,"is_primary":true,"text":[],"label":null}}],"children":[{note}],"rendered":"error[E0015]: {said}\n"}}}}"#
    );
    rust_mutants::cargo::parse_messages(json.as_bytes())
        .expect("the composed refusal parses")
        .into_iter()
        .next()
        .expect("one message")
}

/// Where the twin `make` of `owner` is.
fn make_of(file: &rust_mutants::instrument::FileOutput, owner: &str) -> ConstFnAt {
    let function = file
        .deconst
        .iter()
        .find(|function| function.owner.as_deref() == Some(owner))
        .expect("the twin is written without its const");
    ConstFnAt {
        path: "src/lib.rs".to_owned(),
        keyword: function.origin,
    }
}

#[test]
fn a_refused_evaluation_is_about_the_function_its_note_defines_wherever_it_points() {
    let (file, a, b) = twins();
    assert!(
        !a.is_empty() && !b.is_empty() && a.is_disjoint(&b),
        "each twin holds guards of its own"
    );
    let keyword = make_of(&file, "A");
    let defined = file
        .deconst
        .iter()
        .find(|function| function.owner.as_deref() == Some("A"))
        .expect("A's make")
        .keyword;
    let attributed = attribute(
        std::slice::from_ref(&file),
        &[evaluation(
            "cannot call non-const function `make` in constants",
            (0, 1),
            Some((defined.start, defined.end)),
        )],
        &Constness::default(),
    );
    assert_eq!(
        attributed.condemned, a,
        "the name alone is either twin, and the note's span is one of them: every mutant of that \
         one is condemned, and none of the other"
    );
    assert_eq!(
        attributed.evaluated, a,
        "for being evaluated before the program runs"
    );
    assert_eq!(
        attributed.pinned,
        BTreeSet::from([keyword]),
        "and the function keeps its const from now on"
    );
    assert!(attributed.unattributed.is_empty() && attributed.carried.is_empty());
}

#[test]
fn a_refused_evaluation_with_no_note_is_about_every_function_its_message_could_name() {
    let (file, a, b) = twins();
    let named = |said: &str| {
        attribute(
            std::slice::from_ref(&file),
            &[evaluation(said, (0, 1), None)],
            &Constness::default(),
        )
        .condemned
    };
    assert_eq!(
        named("cannot call non-const associated function `B::<u8>::make` in constants"),
        b,
        "an associated function is named by its type, generic arguments and all, and the type \
         tells the twins apart"
    );
    assert_eq!(
        named("cannot call non-const associated function `C::make` in constants"),
        a.union(&b).copied().collect::<BTreeSet<u32>>(),
        "a type the message names and no twin belongs to leaves the name, which could be either: \
         both are condemned, since a round that condemns too little is refused again and one that \
         guesses is a build nobody can trust"
    );
    assert!(
        named("cannot call non-const function `elsewhere` in constants").is_empty(),
        "a function the tree does not write without its const is none of this rule's business"
    );
}

#[test]
fn a_refused_call_inside_a_const_fn_that_keeps_its_const_for_want_of_a_guard_makes_it_a_carrier() {
    let (file, a, b) = twins();
    assert!(
        a.is_disjoint(&b),
        "the call names A's make, and B's guards are no part of it"
    );
    let caller = file
        .constant
        .iter()
        .find(|function| function.name == "keeps")
        .expect("keeps is written with its const");
    let inside = (caller.body.start + 2, caller.body.start + 3);
    let keeps = ConstFnAt {
        path: "src/lib.rs".to_owned(),
        keyword: caller.origin,
    };
    let said = "cannot call non-const associated function `A::make` in constant functions";
    let carried = attribute(
        std::slice::from_ref(&file),
        &[evaluation(said, inside, None)],
        &Constness::default(),
    );
    assert!(
        carried.condemned.is_empty() && carried.pinned.is_empty(),
        "nothing says keeps is evaluated before the program runs, so nothing is condemned: {carried:?}"
    );
    assert_eq!(
        carried.calls,
        BTreeSet::from([(keeps.clone(), make_of(&file, "A"))]),
        "keeps goes without its const next round, wherever A's make does"
    );
    assert_eq!(
        carried.carried.len(),
        1,
        "and the round says which error that answers"
    );
    let pinned = attribute(
        std::slice::from_ref(&file),
        &[evaluation(said, inside, None)],
        &Constness {
            pinned: BTreeSet::from([keeps]),
            calls: BTreeSet::new(),
        },
    );
    assert_eq!(
        pinned.condemned, a,
        "where the caller keeps its const because the compiler evaluates it, so is the callee, \
         and every mutant it holds is condemned"
    );
    assert!(pinned.calls.is_empty());
}

#[test]
fn a_carrier_is_every_caller_of_a_function_holding_a_guard_until_one_the_compiler_evaluates() {
    let at = |start: u32| ConstFnAt {
        path: "src/lib.rs".to_owned(),
        keyword: rust_mutants::span::Span {
            start,
            end: start + 5,
        },
    };
    let mut constness = Constness {
        pinned: BTreeSet::new(),
        calls: BTreeSet::from([(at(10), at(0)), (at(20), at(10)), (at(30), at(40))]),
    };
    let spans = |carriers: BTreeMap<String, Vec<rust_mutants::span::Span>>| -> Vec<u32> {
        let mut starts = Vec::new();
        for file in carriers.into_values() {
            starts.extend(file.iter().map(|span| span.start));
        }
        starts
    };
    let holding = BTreeSet::from([at(0)]);
    assert_eq!(
        spans(constness.carriers(&holding)),
        [10, 20],
        "a caller carries the guard, and so does its caller, and a call from a function holding \
         nothing to one holding nothing carries nothing"
    );
    assert!(
        constness.carriers(&BTreeSet::new()).is_empty(),
        "with no guard held, no function goes without its const: the tree the bisection starts \
         from is the pristine one"
    );
    constness.pinned.insert(at(20));
    assert_eq!(
        spans(constness.carriers(&holding)),
        [10],
        "a caller the compiler evaluates before the program runs keeps its const"
    );
}
