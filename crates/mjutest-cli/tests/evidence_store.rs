// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What an earlier run established about one mutant, and every condition under which this run may believe it.

use std::collections::{BTreeMap, BTreeSet};

use mjutest_cli::evidence::store::{
    Outcome, Refusal, SCHEMA, Standing, StoreError, path_of, read, record, write,
};

fn reaching(targets: &[&str]) -> BTreeSet<String> {
    targets.iter().map(|one| (*one).to_owned()).collect()
}

fn standing(entries: &[(&str, &str)]) -> Standing {
    Standing {
        passing: entries
            .iter()
            .map(|(target, key)| ((*target).to_owned(), (*key).to_owned()))
            .collect(),
    }
}

fn killed(target: &str, key: &str) -> Outcome {
    Outcome::Killed {
        target: target.to_owned(),
        key: key.to_owned(),
    }
}

fn survived(entries: &[(&str, &str)]) -> Outcome {
    Outcome::Survived {
        targets: entries
            .iter()
            .map(|(target, key)| ((*target).to_owned(), (*key).to_owned()))
            .collect(),
    }
}

#[test]
fn a_kill_is_believed_when_the_target_that_noticed_still_reaches_it_and_still_behaves_the_same() {
    let one = record("m1", "run-1", killed("t1", "k1"));
    assert_eq!(
        one.believable(&reaching(&["t1", "t2"]), &standing(&[("t1", "k1")])),
        Ok(())
    );

    assert_eq!(
        one.believable(&reaching(&["t2"]), &standing(&[("t1", "k1")])),
        Err(Refusal::NotRouted {
            target: "t1".to_owned()
        }),
        "a target this run does not route to the mutant establishes nothing about it"
    );
    assert_eq!(
        one.believable(&reaching(&["t1"]), &standing(&[("t1", "different")])),
        Err(Refusal::KeyChanged {
            target: "t1".to_owned()
        }),
        "a target that does not behave the way it did did not do what was recorded"
    );
    assert_eq!(
        one.believable(&reaching(&["t1"]), &standing(&[])),
        Err(Refusal::NotPassing {
            target: "t1".to_owned()
        }),
        "a kill is believed only where this run saw the test pass on the original tree"
    );
}

#[test]
fn a_survival_is_believed_only_when_every_target_that_could_notice_is_one_that_did_not() {
    let one = record("m1", "run-1", survived(&[("t1", "k1"), ("t2", "k2")]));
    assert_eq!(
        one.believable(
            &reaching(&["t1", "t2"]),
            &standing(&[("t1", "k1"), ("t2", "k2")])
        ),
        Ok(())
    );
    assert_eq!(
        one.believable(&reaching(&["t1"]), &standing(&[("t1", "k1")])),
        Ok(()),
        "a reaching set smaller than the recorded one is still covered by it"
    );
    assert_eq!(
        one.believable(&reaching(&[]), &standing(&[])),
        Err(Refusal::NothingRouted),
        "a universal claim over an empty set is vacuously true, and believing one would \
         answer a question this run never asked"
    );

    assert_eq!(
        one.believable(
            &reaching(&["t1", "t3"]),
            &standing(&[("t1", "k1"), ("t3", "k3")])
        ),
        Err(Refusal::TargetEntered {
            target: "t3".to_owned()
        }),
        "a target that entered the reaching set is a test nothing was ever run against"
    );
    assert_eq!(
        one.believable(
            &reaching(&["t1", "t2"]),
            &standing(&[("t1", "k1"), ("t2", "changed")])
        ),
        Err(Refusal::KeyChanged {
            target: "t2".to_owned()
        })
    );
    assert_eq!(
        one.believable(&reaching(&["t1", "t2"]), &standing(&[("t1", "k1")])),
        Err(Refusal::NotPassing {
            target: "t2".to_owned()
        })
    );
}

#[test]
fn what_one_run_recorded_is_what_the_next_one_reads() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(
        read(dir.path(), "m1")
            .expect("a miss is not a failure")
            .is_none()
    );

    let one = record("m1", "run-1", killed("t1", "k1"));
    let written = write(dir.path(), &one).expect("written");
    assert_eq!(written, path_of(dir.path(), "m1"));
    assert_eq!(read(dir.path(), "m1").expect("readable"), Some(one));

    let contradicted = record("m1", "run-2", survived(&[("t1", "k1")]));
    write(dir.path(), &contradicted).expect("written");
    assert_eq!(
        read(dir.path(), "m1").expect("readable"),
        Some(contradicted),
        "a stale record is removed by being contradicted, and nothing else removes one"
    );
}

#[test]
fn a_record_that_is_not_about_the_mutant_it_is_filed_under_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), &record("m1", "run-1", killed("t1", "k1"))).expect("written");
    std::fs::copy(path_of(dir.path(), "m1"), path_of(dir.path(), "m2")).expect("copy");
    let error = read(dir.path(), "m2").expect_err("a record about another mutant");
    assert!(matches!(error, StoreError::Corrupt { .. }), "{error}");
    assert!(error.to_string().contains("MJ8004"), "{error}");

    std::fs::write(path_of(dir.path(), "m3"), "{ not a record").expect("write");
    read(dir.path(), "m3").expect_err("a document that does not parse");
}

#[test]
fn a_record_says_what_shape_it_is_and_what_established_it() {
    let one = record("m1", "run-1", killed("t1", "k1"));
    assert_eq!(one.schema, SCHEMA);
    assert_eq!(one.run_id, "run-1");
    let text = serde_json::to_string(&one).expect("renders");
    assert!(text.contains("\"kind\":\"killed\""), "{text}");
    let back: mjutest_cli::evidence::store::Record =
        serde_json::from_str(&text).expect("reads back");
    assert_eq!(back, one);

    let other = record(
        "m2",
        "run-1",
        Outcome::Survived {
            targets: BTreeMap::new(),
        },
    );
    let text = serde_json::to_string(&other).expect("renders");
    assert!(text.contains("\"kind\":\"survived\""), "{text}");
}
