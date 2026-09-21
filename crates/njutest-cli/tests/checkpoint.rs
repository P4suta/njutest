// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#![expect(
    clippy::disallowed_methods,
    reason = "a test asserts what the filesystem says by asking it directly"
)]

//! Scheduling state for continuing an interrupted verification: what is saved, what is refused, and what a restored target still reaches.

use std::collections::BTreeSet;
use std::path::PathBuf;

use njutest_cli::assure::mutation::{Disposition, inherited};
use njutest_cli::checkpoint::{
    CheckpointError, SCHEMA, SavedDisposition, SavedMutant, SavedTarget, State, clear, path_of,
    read, write,
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
        disposition: SavedDisposition::Killed {
            by: "demo/lib/demo tests::works".to_owned(),
        },
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
    let mutant_id = "b".repeat(64);
    assert!(state.is_empty());
    state.attempts = 1;
    state.record_target(target("t1"));
    state.record_mutant(mutant(&mutant_id));
    assert!(!state.is_empty());
    let written = write(dir.path(), &state).expect("written");
    assert_eq!(
        written,
        path_of(dir.path(), &identity).expect("canonical identity")
    );

    let read_back = read(dir.path(), &identity)
        .expect("readable")
        .expect("what was saved is there");
    assert_eq!(read_back, state);
    assert_eq!(read_back.schema, SCHEMA);
    assert_eq!(read_back.target("t1").map(|one| one.duration_ms), Some(7));
    assert!(matches!(
        read_back.mutant(&mutant_id).map(|one| &one.disposition),
        Some(SavedDisposition::Killed { .. })
    ));
    assert!(read_back.target("t2").is_none());
}

#[test]
fn one_identity_owns_one_checkpoint_and_never_answers_for_another() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);
    let other = "b".repeat(64);
    let mut state = State::new(&identity);
    state.attempts = 1;
    state.record_target(target("t1"));
    write(dir.path(), &state).expect("written");
    assert!(
        read(dir.path(), &other).expect("readable").is_none(),
        "different inputs are a different question"
    );

    std::fs::create_dir_all(
        path_of(dir.path(), &other)
            .expect("canonical identity")
            .parent()
            .expect("a directory"),
    )
    .expect("mkdir");
    std::fs::copy(
        path_of(dir.path(), &identity).expect("canonical identity"),
        path_of(dir.path(), &other).expect("canonical identity"),
    )
    .expect("copy");
    let error = read(dir.path(), &other).expect_err("state about other inputs");
    assert!(matches!(error, CheckpointError::Corrupt { .. }), "{error}");
    assert!(error.to_string().contains("NJ8004"), "{error}");
}

#[test]
fn state_that_is_not_the_state_it_claims_to_be_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);
    let path = path_of(dir.path(), &identity).expect("canonical identity");
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
    state.attempts = 1;
    state.record_target(target("t1"));
    write(dir.path(), &state).expect("written");
    clear(dir.path(), &identity).expect("cleared");
    assert!(read(dir.path(), &identity).expect("readable").is_none());
    clear(dir.path(), &identity).expect("absence is already clear");
}

#[test]
fn only_a_claim_the_next_run_can_inherit_is_saved() {
    let mut state = State::new(&"a".repeat(64));
    state.record_mutant(mutant("m1"));
    assert_eq!(
        state.mutants.len(),
        1,
        "the checkpoint API accepts the one closed fact a successor can inherit"
    );
    assert_eq!(
        serde_json::to_value(state.mutants.first().expect("one saved kill"))
            .expect("a saved kill")
            .get("disposition")
            .and_then(|disposition| disposition.get("kind"))
            .and_then(serde_json::Value::as_str),
        Some("killed")
    );
}

#[test]
fn state_that_carries_a_disposition_a_run_cannot_inherit_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);
    let path = path_of(dir.path(), &identity).expect("canonical identity");
    std::fs::create_dir_all(path.parent().expect("a directory")).expect("mkdir");
    std::fs::write(
        &path,
        format!(
            "{{\"schema\":\"{SCHEMA}\",\"identity\":\"{identity}\",\"attempts\":1,\
             \"targets\":[],\"mutants\":[{{\"id\":\"m1\",\
             \"disposition\":{{\"kind\":\"survived\"}},\"duration_ms\":1}}]}}"
        ),
    )
    .expect("write");
    let error = read(dir.path(), &identity).expect_err("a claim no run made");
    assert!(error.to_string().contains("survived"), "{error}");
}

#[test]
fn a_resumed_run_carries_only_kills_and_reads_nothing_else_as_one() {
    let killed = SavedMutant {
        id: "m1".to_owned(),
        disposition: SavedDisposition::Killed {
            by: "pkg/lib/pkg one".to_owned(),
        },
        duration_ms: 5,
    };

    assert_eq!(
        inherited(&killed),
        Disposition::Killed {
            by: "pkg/lib/pkg one".to_owned()
        },
        "a test noticed the mutation on this tree, and that stays true however the next \
         run routes: re-running it would spend the time to learn what is already known"
    );
}

#[test]
fn legacy_bound_outcomes_are_outside_the_current_checkpoint_layout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);
    let legacy = dir.path().join(&identity).join("checkpoint-v1.json");
    std::fs::create_dir_all(legacy.parent().expect("a directory")).expect("mkdir");
    std::fs::write(
        legacy,
        format!(
            "{{\"schema\":\"njutest-assurance-checkpoint-v1\",\"identity\":\"{identity}\",\
             \"attempts\":1,\"targets\":[],\"mutants\":[\
             {{\"id\":\"m1\",\"disposition\":\"runaway\",\"killed_by\":\"one\",\"duration_ms\":1}},\
             {{\"id\":\"m2\",\"disposition\":\"timed_out\",\"killed_by\":\"one\",\"duration_ms\":1}}]}}"
        ),
    )
    .expect("legacy checkpoint");

    assert!(
        read(dir.path(), &identity)
            .expect("current layout")
            .is_none(),
        "v1 bound outcomes carried no matched control, so the v2 reader never opens \
         that file and judges both mutations again"
    );
}

#[test]
fn a_current_kill_without_a_target_is_not_a_checkpoint() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);
    let path = path_of(dir.path(), &identity).expect("canonical identity");
    std::fs::create_dir_all(path.parent().expect("a directory")).expect("mkdir");
    std::fs::write(
        &path,
        format!(
            "{{\"schema\":\"{SCHEMA}\",\"identity\":\"{identity}\",\"attempts\":1,\
             \"targets\":[],\"mutants\":[{{\"id\":\"m1\",\
             \"disposition\":{{\"kind\":\"killed\",\"by\":null}},\"duration_ms\":1}}]}}"
        ),
    )
    .expect("forged checkpoint");
    assert!(
        read(dir.path(), &identity).is_err(),
        "the v2 union cannot represent a kill without the target that noticed"
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

    let path = path_of(dir.path(), &identity).expect("canonical identity");
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
    let mut state = State::new(&identity);
    state.attempts = 1;

    let occupied = dir.path().join("occupied");
    std::fs::write(&occupied, "not a directory").expect("a file where a root goes");
    let wanted = path_of(&occupied, &identity)
        .expect("canonical identity")
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
    let inner = path_of(&blocked, &identity).expect("canonical identity");
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
    let mut state = State::new(&identity);
    state.attempts = 1;
    let path = write(dir.path(), &state).expect("a checkpoint");
    let directory = path.parent().expect("a directory").to_path_buf();

    clear(dir.path(), &identity).expect("a completed run clears its checkpoint");

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

#[test]
fn a_stored_checkpoint_requires_an_attempt_and_unique_canonical_ids() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);
    let empty = State::new(&identity);
    let error = write(dir.path(), &empty).expect_err("zero attempts wrote no state");
    assert!(error.to_string().contains("zero attempts"), "{error}");

    let path = path_of(dir.path(), &identity).expect("canonical identity");
    std::fs::create_dir_all(path.parent().expect("identity directory")).expect("mkdir");
    for (member, entries) in [
        (
            "targets",
            "[{\"id\":\"same\",\"status\":\"passed\",\"duration_ms\":1,\"message\":null,\"files\":[]},{\"id\":\"same\",\"status\":\"passed\",\"duration_ms\":2,\"message\":null,\"files\":[]}]",
        ),
        (
            "mutants",
            "[{\"id\":\"same\",\"disposition\":{\"kind\":\"killed\",\"by\":\"t\"},\"duration_ms\":1},{\"id\":\"same\",\"disposition\":{\"kind\":\"killed\",\"by\":\"t\"},\"duration_ms\":2}]",
        ),
    ] {
        let (targets, mutants) = if member == "targets" {
            (entries, "[]")
        } else {
            ("[]", entries)
        };
        std::fs::write(
            &path,
            format!(
                "{{\"schema\":\"{SCHEMA}\",\"identity\":\"{identity}\",\"attempts\":1,\
                 \"targets\":{targets},\"mutants\":{mutants}}}"
            ),
        )
        .expect("forged checkpoint");
        let error = read(dir.path(), &identity).expect_err("duplicate identity");
        assert!(
            error.to_string().contains("not strictly increasing"),
            "{member}: {error}"
        );
    }
}

#[test]
fn clearing_surfaces_every_failure_other_than_absence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let identity = "a".repeat(64);
    let path = path_of(dir.path(), &identity).expect("canonical identity");
    std::fs::create_dir_all(&path).expect("directory in place of checkpoint");
    let error = clear(dir.path(), &identity).expect_err("a directory is not a file");
    assert!(matches!(error, CheckpointError::Unusable { .. }), "{error}");

    std::fs::remove_dir_all(path.parent().expect("identity directory")).expect("reset");
    let mut state = State::new(&identity);
    state.attempts = 1;
    write(dir.path(), &state).expect("checkpoint");
    let sibling = path
        .parent()
        .expect("identity directory")
        .join("unexpected");
    std::fs::write(&sibling, "occupied").expect("sibling");
    let error = clear(dir.path(), &identity).expect_err("nonempty identity directory");
    assert!(matches!(error, CheckpointError::Unusable { .. }), "{error}");
    assert!(!path.exists(), "the stale v2 checkpoint itself was removed");
}

#[test]
fn untrusted_identities_and_paths_are_rejected_before_filesystem_use() {
    let dir = tempfile::tempdir().expect("tempdir");
    let invalid_identities = vec![
        "../outside".to_owned(),
        "/absolute".to_owned(),
        "short".to_owned(),
        "A".repeat(64),
    ];
    for identity in &invalid_identities {
        assert!(
            read(dir.path(), identity).is_err(),
            "accepted checkpoint identity {identity:?}"
        );
        assert!(
            clear(dir.path(), identity).is_err(),
            "cleared through checkpoint identity {identity:?}"
        );
    }

    let identity = "a".repeat(64);
    let mut state = State::new(&identity);
    state.attempts = 1;
    state.record_mutant(mutant("../../outside"));
    assert!(
        write(dir.path(), &state).is_err(),
        "accepted a forged mutant id"
    );

    let malformed_files = [
        vec!["/absolute.rs".to_owned()],
        vec!["../outside.rs".to_owned()],
        vec!["src/lib.rs".to_owned(), "src/lib.rs".to_owned()],
        vec!["src/z.rs".to_owned(), "src/a.rs".to_owned()],
    ];
    for files in malformed_files {
        let mut state = State::new(&identity);
        state.attempts = 1;
        let mut saved = target("target");
        saved.files = files;
        state.record_target(saved);
        assert!(write(dir.path(), &state).is_err(), "accepted source paths");
    }

    let mut state = State::new(&identity);
    state.attempts = 1;
    state.record_target(target(""));
    assert!(
        write(dir.path(), &state).is_err(),
        "accepted an empty target id"
    );
}
