// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Scheduling state for continuing an interrupted verification: what is saved, what is refused, and what a restored target still reaches.

use std::collections::BTreeSet;
use std::path::PathBuf;

use njutest_cli::assure::mutation::{Disposition, inherited};
use njutest_cli::checkpoint::{
    CheckpointError, SCHEMA, SavedMutant, SavedTarget, State, clear, path_of, read, write,
};
use njutest_cli::coverage::Point;
use njutest_cli::report::TargetStatus;

fn target(id: &str) -> SavedTarget {
    SavedTarget {
        id: id.to_owned(),
        status: TargetStatus::Passed,
        duration_ms: 7,
        message: None,
        files: vec!["src/lib.rs".to_owned()],
    }
}

fn mutant(id: &str) -> SavedMutant {
    SavedMutant {
        id: id.to_owned(),
        disposition: "killed".to_owned(),
        killed_by: Some("demo/lib/demo tests::works".to_owned()),
        duration_ms: 3,
    }
}

#[test]
fn what_an_interrupted_run_saved_is_what_the_next_one_reads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);
    assert!(
        read(dir.path(), &identity)
            .expect("nothing saved is not a failure")
            .is_none()
    );

    let mut state = State::new(&identity);
    assert!(state.is_empty());
    state.attempts = 1;
    state.record_target(target("t1"));
    state.record_mutant(mutant("m1"));
    assert!(!state.is_empty());
    let written = write(dir.path(), &state).expect("written");
    assert_eq!(written, path_of(dir.path(), &identity));

    let read_back = read(dir.path(), &identity)
        .expect("readable")
        .expect("what was saved is there");
    assert_eq!(read_back, state);
    assert_eq!(read_back.schema, SCHEMA);
    assert_eq!(read_back.target("t1").map(|one| one.duration_ms), Some(7));
    assert_eq!(
        read_back.mutant("m1").map(|one| one.disposition.as_str()),
        Some("killed")
    );
    assert!(read_back.target("t2").is_none());
}

#[test]
fn one_identity_owns_one_checkpoint_and_never_answers_for_another() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);
    let other = "b".repeat(64);
    let mut state = State::new(&identity);
    state.record_target(target("t1"));
    write(dir.path(), &state).expect("written");
    assert!(
        read(dir.path(), &other).expect("readable").is_none(),
        "different inputs are a different question"
    );

    std::fs::create_dir_all(path_of(dir.path(), &other).parent().expect("a directory"))
        .expect("mkdir");
    std::fs::copy(path_of(dir.path(), &identity), path_of(dir.path(), &other)).expect("copy");
    let error = read(dir.path(), &other).expect_err("state about other inputs");
    assert!(matches!(error, CheckpointError::Corrupt { .. }), "{error}");
    assert!(error.to_string().contains("NJ8004"), "{error}");
}

#[test]
fn state_that_is_not_the_state_it_claims_to_be_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);
    let path = path_of(dir.path(), &identity);
    std::fs::create_dir_all(path.parent().expect("a directory")).expect("mkdir");

    std::fs::write(&path, "{ not state").expect("write");
    assert!(
        read(dir.path(), &identity).is_err(),
        "a document that does not parse"
    );

    std::fs::write(
        &path,
        format!(
            "{{\"schema\":\"something-else\",\"identity\":\"{identity}\",\"attempts\":1,\
             \"targets\":[],\"mutants\":[]}}"
        ),
    )
    .expect("write");
    let error = read(dir.path(), &identity).expect_err("a document of another shape");
    assert!(error.to_string().contains("something-else"), "{error}");

    std::fs::write(
        &path,
        format!(
            "{{\"schema\":\"{SCHEMA}\",\"identity\":\"{identity}\",\"attempts\":1,\
             \"targets\":[],\"mutants\":[],\"extra\":true}}"
        ),
    )
    .expect("write");
    assert!(
        read(dir.path(), &identity).is_err(),
        "a field this release does not understand is a state it cannot continue from"
    );
}

#[test]
fn a_restored_target_keeps_reaching_its_whole_file_and_narrows_nothing() {
    let restored = target("t1");
    let coverage: BTreeSet<_> = restored.coverage();
    assert_eq!(coverage.len(), 1);
    let file = PathBuf::from("src/lib.rs");
    for line in [1u32, 40, u32::MAX - 1] {
        assert!(
            coverage
                .iter()
                .any(|block| block.contains(&file, Point { line, column: 1 })),
            "a target restored from a checkpoint carries no regions to narrow with"
        );
    }
    assert!(
        !coverage
            .iter()
            .any(|block| block
                .contains(&PathBuf::from("src/other.rs"), Point { line: 1, column: 1 })),
        "it reaches the files it reached and no others"
    );
    let whole = coverage.iter().next().expect("one block");
    assert_eq!(
        whole.start,
        Point { line: 0, column: 0 },
        "and it begins before the first thing in the file: a block that starts at the \
         first line leaves whatever is above it outside, and what a restored target \
         narrows with is nothing at all"
    );
    assert_eq!(
        whole.end,
        Point {
            line: u32::MAX,
            column: u32::MAX,
        },
        "and ends after the last"
    );
}

#[test]
fn recording_the_same_thing_twice_replaces_it_rather_than_repeating_it() {
    let mut state = State::new(&"a".repeat(64));
    state.record_target(target("t2"));
    state.record_target(target("t1"));
    let mut again = target("t1");
    again.status = TargetStatus::Failed;
    state.record_target(again);
    assert_eq!(state.targets.len(), 2);
    assert_eq!(
        state.target("t1").map(|one| one.status),
        Some(TargetStatus::Failed)
    );
    let ids: Vec<&str> = state.targets.iter().map(|one| one.id.as_str()).collect();
    assert_eq!(ids, ["t1", "t2"], "the order is a function of the ids");

    state.record_mutant(mutant("m1"));
    state.record_mutant(mutant("m1"));
    assert_eq!(state.mutants.len(), 1);
}

#[test]
fn a_run_that_finished_has_nothing_to_continue_from() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);
    let mut state = State::new(&identity);
    state.record_target(target("t1"));
    write(dir.path(), &state).expect("written");
    clear(dir.path(), &identity);
    assert!(read(dir.path(), &identity).expect("readable").is_none());
    clear(dir.path(), &identity);
}

#[test]
fn only_a_claim_the_next_run_can_inherit_is_saved() {
    let mut state = State::new(&"a".repeat(64));
    for disposition in ["killed", "timed_out"] {
        let mut one = mutant(disposition);
        one.disposition = disposition.to_owned();
        state.record_mutant(one);
    }
    assert_eq!(state.mutants.len(), 2);

    for disposition in [
        "survived",
        "unreached",
        "errored",
        "unconfirmed",
        "rejected",
    ] {
        let mut one = mutant(disposition);
        one.disposition = disposition.to_owned();
        state.record_mutant(one);
    }
    assert_eq!(
        state.mutants.len(),
        2,
        "a disposition that depends on how the run routed is re-derived, not inherited"
    );
}

#[test]
fn state_that_carries_a_disposition_a_run_cannot_inherit_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);
    let path = path_of(dir.path(), &identity);
    std::fs::create_dir_all(path.parent().expect("a directory")).expect("mkdir");
    std::fs::write(
        &path,
        format!(
            "{{\"schema\":\"{SCHEMA}\",\"identity\":\"{identity}\",\"attempts\":1,\
             \"targets\":[],\"mutants\":[{{\"id\":\"m1\",\"disposition\":\"survived\",\
             \"killed_by\":null,\"duration_ms\":1}}]}}"
        ),
    )
    .expect("write");
    let error = read(dir.path(), &identity).expect_err("a claim no run made");
    assert!(error.to_string().contains("survived"), "{error}");
}

#[test]
fn a_resumed_run_carries_the_two_facts_a_checkpoint_may_hold_and_reads_nothing_else_as_one() {
    let killed = SavedMutant {
        id: "m1".to_owned(),
        disposition: "killed".to_owned(),
        killed_by: Some("pkg/lib/pkg one".to_owned()),
        duration_ms: 5,
    };
    let ran_away = SavedMutant {
        disposition: "runaway".to_owned(),
        ..killed.clone()
    };
    let waited = SavedMutant {
        disposition: "waited".to_owned(),
        ..killed.clone()
    };
    let older = SavedMutant {
        disposition: "timed_out".to_owned(),
        ..killed.clone()
    };

    assert_eq!(
        inherited(&killed),
        Some(Disposition::Killed {
            by: "pkg/lib/pkg one".to_owned()
        }),
        "a test noticed the mutation on this tree, and that stays true however the next \
         run routes: re-running it would spend the time to learn what is already known"
    );
    assert_eq!(
        inherited(&ran_away),
        Some(Disposition::Runaway {
            on: "pkg/lib/pkg one".to_owned()
        }),
        "a mutation that stopped the program terminating is the same kind of fact about the \
         same tree as a kill, which is why both are saved. A bound expiring is not: that is \
         a fact about the machine that measured, and the next machine is not that one"
    );
    assert_eq!(
        inherited(&waited),
        None,
        "a bound expiring is a fact about the machine that watched, so inheriting it would \
         hand the next machine a measurement it never made"
    );
    assert_eq!(
        inherited(&older),
        None,
        "a checkpoint an older release wrote spells a name that meant two things, and \
         reading it as either is guessing which one the machine that wrote it saw"
    );

    for disposition in [
        "survived",
        "unreached",
        "errored",
        "unconfirmed",
        "rejected",
    ] {
        let other = SavedMutant {
            disposition: disposition.to_owned(),
            ..killed.clone()
        };
        assert_eq!(
            inherited(&other),
            None,
            "{disposition} depends on how the run routed, and a resumed run routes at \
             file granularity, so carrying it would make the report say a claim this \
             run never made"
        );
    }
}

#[test]
fn a_kill_a_checkpoint_cannot_attribute_is_one_a_resumed_run_judges_again() {
    let anonymous = SavedMutant {
        id: "m1".to_owned(),
        disposition: "killed".to_owned(),
        killed_by: None,
        duration_ms: 5,
    };

    assert_eq!(
        inherited(&anonymous),
        None,
        "the report a resumed run ends in has to say which test noticed each mutation, \
         and inheriting a kill with nobody's name on it would put a killed row in it \
         that names nobody"
    );
}

#[test]
fn a_fresh_state_has_tried_nothing_yet() {
    assert_eq!(
        State::new(&"a".repeat(64)).attempts,
        0,
        "the attempt count is what a resumed run adds to, and starting anywhere else \
         would make the first attempt look like a retry of one nobody made"
    );
}

#[test]
fn the_mutants_a_state_carries_are_in_one_order_however_they_were_recorded() {
    let mut state = State::new(&"a".repeat(64));
    for id in ["m3", "m1", "m2"] {
        state.record_mutant(mutant(id));
    }

    let ids: Vec<&str> = state.mutants.iter().map(|one| one.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["m1", "m2", "m3"],
        "a checkpoint is written and read back and compared with another; two runs that \
         judged the same mutants in a different order would write two different files \
         for one state"
    );
}

#[test]
fn a_checkpoint_that_is_not_there_is_not_an_answer_and_not_a_failure_either() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);

    assert!(
        matches!(read(dir.path(), &identity), Ok(None)),
        "no run has been interrupted here, so there is nothing to continue from"
    );

    let path = path_of(dir.path(), &identity);
    std::fs::create_dir_all(&path).expect("a directory where the file goes");
    let error = read(dir.path(), &identity).expect_err("a path that is not a file");
    assert!(
        matches!(error, CheckpointError::Unusable { .. }),
        "and a checkpoint this could not read for any other reason is not the same as \
         one that is not there: reading it as absent would start a run over and call it \
         a fresh one: {error}"
    );
}

#[test]
fn a_checkpoint_that_cannot_be_written_says_which_path_refused_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);
    let state = State::new(&identity);

    let occupied = dir.path().join("occupied");
    std::fs::write(&occupied, "not a directory").expect("a file where a root goes");
    let wanted = path_of(&occupied, &identity)
        .parent()
        .expect("a directory")
        .to_path_buf();
    let refused = write(&occupied, &state).expect_err("a root that is a file");
    assert!(
        matches!(&refused, CheckpointError::Unusable { path, .. } if path == &wanted),
        "the directory is what could not be made, and every later step fails for the \
         same reason and names something else, so the path is the only thing that says \
         which step this was: {refused}"
    );

    let blocked = dir.path().join("blocked");
    let inner = path_of(&blocked, &identity);
    std::fs::create_dir_all(&inner).expect("a directory where the checkpoint goes");
    let refused = write(&blocked, &state).expect_err("a checkpoint path that is a directory");
    assert!(
        matches!(&refused, CheckpointError::Unusable { path, .. } if path == &inner),
        "and one that was written and could not be moved into place names where it was \
         going: {refused}"
    );
}

#[test]
fn a_run_that_finished_leaves_nothing_of_its_checkpoint_behind() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);
    let path = write(dir.path(), &State::new(&identity)).expect("a checkpoint");
    let directory = path.parent().expect("a directory").to_path_buf();

    clear(dir.path(), &identity);

    assert!(
        !path.exists(),
        "the state a finished run cannot continue from"
    );
    assert!(
        !directory.exists(),
        "and the directory it was alone in: a tree that fills with empty directories is \
         one somebody eventually stops trusting to clean up after itself"
    );
}
