// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading one verified run: what a target's row is named, what a run states it could not do, and which line of a failure a person acts on.

use std::path::PathBuf;

use njutest_cli::assure::baseline::{
    failure, limitations, named, refused, target_of, unmeasurable,
};
use njutest_cli::limitation::PROC_MACRO_EXPANSION_NOT_MEASURED;
use njutest_cli::targets::UnitKind;
use rust_mutants::execute::{TargetKind, TestTarget};
use rust_mutants::limitation::{CUSTOM_HARNESS, DOCTESTS_NONE, DOCTESTS_ROUTED_BY_FILE};

fn target(id: &str, kind: TargetKind, states: &[&str]) -> TestTarget {
    let mut target = TestTarget::new(
        id.split('/').next().unwrap_or_default(),
        kind,
        id.rsplit('/').next().unwrap_or_default(),
        PathBuf::from("/tmp/one"),
        PathBuf::from("/tmp"),
    );
    target.limitations = states.iter().map(|one| (*one).to_owned()).collect();
    target
}

#[test]
fn a_library_that_documents_nothing_is_not_a_library_whose_examples_were_routed_coarsely() {
    let nothing = target(
        "pkg/doc/pkg",
        TargetKind::Doc,
        &[DOCTESTS_NONE, DOCTESTS_ROUTED_BY_FILE],
    );

    assert!(
        unmeasurable(&nothing),
        "a library with no examples ran nothing and states nothing: a row for it would \
         raise a finding about documentation nobody wrote"
    );
    assert!(
        !unmeasurable(&target("pkg/test/one", TargetKind::Test, &[CUSTOM_HARNESS])),
        "and every other target does state what it could not do"
    );
}

#[test]
fn what_a_run_could_not_do_is_said_once_by_whoever_could_not_do_it() {
    let targets = [
        target(
            "pkg/doc/pkg",
            TargetKind::Doc,
            &[DOCTESTS_NONE, "documentation-was-not-measured"],
        ),
        target("pkg/test/one", TargetKind::Test, &[CUSTOM_HARNESS]),
        target("pkg/test/two", TargetKind::Test, &[CUSTOM_HARNESS]),
    ];

    let stated = limitations(&targets, &["coverage-was-not-taken".to_owned()]);

    assert_eq!(
        stated,
        vec![
            "coverage-was-not-taken".to_owned(),
            CUSTOM_HARNESS.to_owned()
        ],
        "the two targets that brought their own harness state one limitation between \
         them, the run states its own, and the library that documents nothing states \
         nothing at all: a limitation named twice reads as two things a run could not do"
    );
}

#[test]
fn a_proc_macro_in_the_workspace_is_a_limitation_of_the_run_and_not_of_a_target() {
    let without = [target("pkg/test/one", TargetKind::Test, &[])];
    let with = [
        target("pkg/test/one", TargetKind::Test, &[]),
        target("derive/proc-macro/derive", TargetKind::ProcMacro, &[]),
    ];

    assert!(
        !limitations(&without, &[]).contains(&PROC_MACRO_EXPANSION_NOT_MEASURED.to_owned()),
        "a workspace with no macro of its own expands nothing this run did not measure"
    );
    assert_eq!(
        limitations(&with, &[]),
        vec![PROC_MACRO_EXPANSION_NOT_MEASURED.to_owned()],
        "and one macro crate is enough: what it expands is decided during the build, so \
         no target of any package carries it, and a run that said nothing would let a \
         reader take the score as covering code that was never mutated"
    );
}

#[test]
fn a_target_the_engine_built_is_named_by_what_cargo_said_about_it() {
    let built = target("pkg/test/one", TargetKind::Test, &[CUSTOM_HARNESS]);

    let row = target_of(&built).expect("valid engine target identity");

    assert_eq!(row.package, "pkg");
    assert_eq!(row.unit, UnitKind::Test);
    assert_eq!(row.unit_name, "one");
    assert!(
        !row.ignored,
        "a target is a binary and not a test inside one, and libtest skips tests rather \
         than binaries: a row that said the whole target was ignored would say a run \
         chose to leave it out"
    );
}

#[test]
fn a_target_the_session_dropped_is_named_by_its_identity_alone() {
    let row = named("pkg/test/one").expect("valid engine target identity");

    assert_eq!(row.package, "pkg");
    assert_eq!(row.unit, UnitKind::Test);
    assert_eq!(row.unit_name, "one");
    assert!(
        !row.ignored,
        "the target was not skipped; its own tests did not pass, which is the finding"
    );
    assert!(
        row.executable.as_os_str().is_empty(),
        "and there is nothing to start it with, because this one is never started"
    );
}

#[test]
fn an_identity_whose_name_holds_a_slash_keeps_the_whole_name() {
    let row = named("pkg/test/nested/one").expect("valid engine target identity");

    assert_eq!(
        row.unit_name, "nested/one",
        "the identity is a package, a kind, and a name, and the name is everything after \
         the second slash: cutting it at the third would file the target under half of \
         its own name"
    );
    assert_eq!(row.package, "pkg");
    assert_eq!(row.unit, UnitKind::Test);
}

#[test]
fn an_identity_naming_a_kind_this_does_not_know_is_refused() {
    assert!(
        named("pkg/fixture/one").is_err(),
        "an unknown kind cannot be silently relabelled as a binary"
    );
}

#[test]
fn the_line_a_reader_acts_on_is_the_test_that_failed_and_not_the_log_before_it() {
    let output = "   Compiling pkg v0.1.0\n\
                  warning: this is what the build FAILED\n\
                  error: something the compiler said\n\
                  test one::two ... FAILED\n";

    assert_eq!(
        failure(output).as_deref(),
        Some("test one::two ... FAILED"),
        "a person looking at a failing target needs the test that failed: a line that \
         merely ends in the word is not one, and the log a build printed before it is \
         not one either"
    );
}

#[test]
fn a_failure_with_no_failing_test_falls_back_to_the_error_and_then_to_anything_at_all() {
    let compiled = "   Compiling pkg v0.1.0\nerror[E0432]: unresolved import\n";
    let neither = "\n   the target said this and stopped\nand then this\n";

    assert_eq!(
        failure(compiled).as_deref(),
        Some("error[E0432]: unresolved import"),
        "the first line of a build log is which crate was compiled, which is true and \
         not what somebody looking at a failure needs"
    );
    assert_eq!(
        failure(neither).as_deref(),
        Some("the target said this and stopped"),
        "and a target that names neither is quoted from its first line, blank ones \
         skipped"
    );
    assert_eq!(
        failure("   \n\n").as_deref(),
        None,
        "a target that said nothing has no line to quote, and inventing one would put \
         words in its mouth"
    );
}

#[test]
fn a_refusal_that_is_not_about_the_tree_stays_an_error() {
    assert!(
        refused(&njutest_cli::error::RunnerError::Interrupted).is_none(),
        "a run somebody stopped has reached no answer about the workspace, and handing \
         back an empty baseline would report a tree nobody measured as one with nothing \
         in it"
    );
}
