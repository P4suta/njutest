// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What an earlier run established about one mutant, and every condition under which this run may believe it.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]
use std::collections::BTreeMap;

use njutest::evidence::store::{
    Answer, Outcome, Refusal, SCHEMA, Standing, StoreError, path_of, read, record, write,
};
use rust_mutants::id::HexDigest;

fn mutant(number: u8) -> HexDigest {
    HexDigest::try_from(format!("{number:064x}")).expect("a canonical mutant id")
}

fn reaching(targets: &[&str]) -> Vec<String> {
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
        before: Vec::new(),
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
    let one = record(mutant(1), "run-1", killed("t1", "k1"));
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
    let one = record(mutant(1), "run-1", survived(&[("t1", "k1"), ("t2", "k2")]));
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
        read(dir.path(), &mutant(1))
            .expect("a miss is not a failure")
            .is_none()
    );

    let one = record(mutant(1), "run-1", killed("t1", "k1"));
    let written = write(dir.path(), &one).expect("written");
    assert_eq!(written, path_of(dir.path(), &mutant(1)));
    assert_eq!(read(dir.path(), &mutant(1)).expect("readable"), Some(one));

    let contradicted = record(mutant(1), "run-2", survived(&[("t1", "k1")]));
    write(dir.path(), &contradicted).expect("written");
    assert_eq!(
        read(dir.path(), &mutant(1)).expect("readable"),
        Some(contradicted),
        "a stale record is removed by being contradicted, and nothing else removes one"
    );
}

#[test]
fn a_record_that_is_not_about_the_mutant_it_is_filed_under_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), &record(mutant(1), "run-1", killed("t1", "k1"))).expect("written");
    std::fs::copy(
        path_of(dir.path(), &mutant(1)),
        path_of(dir.path(), &mutant(2)),
    )
    .expect("copy");
    let error = read(dir.path(), &mutant(2)).expect_err("a record about another mutant");
    assert!(matches!(error, StoreError::Corrupt { .. }), "{error}");
    assert!(error.to_string().contains("NJ8004"), "{error}");

    std::fs::write(path_of(dir.path(), &mutant(3)), "{ not a record").expect("write");
    read(dir.path(), &mutant(3)).expect_err("a document that does not parse");
}

#[test]
fn a_record_says_what_shape_it_is_and_what_established_it() {
    let one = record(mutant(1), "run-1", killed("t1", "k1"));
    assert_eq!(one.schema, SCHEMA);
    assert_eq!(one.run_id, "run-1");
    let text = serde_json::to_string(&one).expect("renders");
    assert!(text.contains("\"kind\":\"killed\""), "{text}");
    let back: njutest::evidence::store::Record =
        njutest_devkit::strictjson::decode_str(&text).expect("reads back");
    assert_eq!(back, one);

    let other = record(
        mutant(2),
        "run-1",
        Outcome::Survived {
            targets: BTreeMap::new(),
        },
    );
    let text = serde_json::to_string(&other).expect("renders");
    assert!(text.contains("\"kind\":\"survived\""), "{text}");
}

#[test]
fn a_record_that_says_it_is_another_shape_is_refused_rather_than_read_as_this_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), &record(mutant(1), "run-1", killed("t1", "k1"))).expect("written");
    let path = path_of(dir.path(), &mutant(1));
    let text = std::fs::read_to_string(&path).expect("the record");
    std::fs::write(&path, text.replace(SCHEMA, "njutest-mutation-evidence-v9"))
        .expect("a record of a shape this release does not know");

    let error = read(dir.path(), &mutant(1)).expect_err("a record of another shape");
    assert!(matches!(error, StoreError::Corrupt { .. }), "{error}");
    assert!(
        error.to_string().contains("njutest-mutation-evidence-v9"),
        "a release that read a later shape's record as its own would believe a claim \
         made under rules it does not have, so it says which shape it found: {error}"
    );
}

#[test]
fn a_record_that_cannot_be_read_or_written_is_a_refusal_and_never_a_miss() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(path_of(dir.path(), &mutant(1)))
        .expect("a directory where a record goes");

    let error = read(dir.path(), &mutant(1)).expect_err("a record that cannot be read");
    assert!(
        matches!(error, StoreError::Unusable { .. }),
        "a record this run could not read is not a run that has no record: reading it \
         as a miss makes an unreadable disk look like a cold cache, and the run \
         re-establishes what it could not read rather than saying it could not: {error}"
    );

    let refused = write(dir.path(), &record(mutant(1), "run-1", killed("t1", "k1")))
        .expect_err("a record that cannot be written");
    assert!(
        matches!(refused, StoreError::Unusable { .. }),
        "and one it could not write is not one it wrote: a later run reads what is \
         there, so a write nobody noticed failing is a run that will read an older \
         answer and believe this one established it: {refused}"
    );

    let blocked = dir.path().join("blocked");
    std::fs::write(&blocked, "a file where a store's directory goes").expect("the file");
    let refused = write(&blocked, &record(mutant(2), "run-1", killed("t1", "k1")))
        .expect_err("a store with nowhere to put its records");
    let directory = path_of(&blocked, &mutant(2))
        .parent()
        .expect("the directory a record goes in")
        .display()
        .to_string();
    assert!(
        refused.to_string().contains(&directory) && !refused.to_string().contains("m2.json"),
        "and a store whose directory cannot be made names the directory rather than the \
         file it would have held: carrying on to the write turns one refusal a person \
         can act on into a second one about a path that was never the problem. It said \
         {refused}, and the directory is {directory}"
    );
}

fn killed_after(target: &str, before: &[(&str, &str)]) -> Outcome {
    Outcome::Killed {
        target: target.to_owned(),
        key: "k2".to_owned(),
        before: before
            .iter()
            .map(|(asked, key)| Answer {
                target: (*asked).to_owned(),
                key: (*key).to_owned(),
                outcome: njutest::report::Outcome::Survived,
            })
            .collect(),
    }
}

#[test]
fn a_kill_is_believed_only_where_this_run_would_ask_exactly_the_targets_asked_before_it() {
    let both = standing(&[("t1", "k1"), ("t2", "k2")]);
    let one = record(mutant(1), "run-1", killed_after("t2", &[("t1", "k1")]));
    assert_eq!(
        one.believable(&reaching(&["t1", "t2"]), &both),
        Ok(()),
        "this run asks `t1` first, as the recording run did, so what `t1` answered is \
         what asking it again would say"
    );

    let unasked = record(mutant(2), "run-1", killed_after("t2", &[]));
    assert_eq!(
        unasked.believable(&reaching(&["t1", "t2"]), &both),
        Err(Refusal::TargetEntered {
            target: "t1".to_owned()
        }),
        "this run would ask `t1` before the one that noticed, and the record has no \
         answer from it, so reading it back would leave out an answer asking gives"
    );

    assert_eq!(
        one.believable(&reaching(&["t2"]), &standing(&[("t2", "k2")])),
        Err(Refusal::NotRouted {
            target: "t1".to_owned()
        }),
        "the record answers for `t1`, which this run no longer asks"
    );

    assert_eq!(
        one.believable(
            &reaching(&["t1", "t2"]),
            &standing(&[("t1", "moved"), ("t2", "k2")])
        ),
        Err(Refusal::KeyChanged {
            target: "t1".to_owned()
        }),
        "what `t1` answered is only this run's answer while `t1` behaves as it did"
    );
}

#[test]
fn a_kill_whose_earlier_answers_no_run_could_have_given_is_not_believed() {
    let three = standing(&[("t1", "k1"), ("t2", "k2"), ("t3", "k2")]);
    let reordered = record(
        mutant(3),
        "run-1",
        killed_after("t3", &[("t2", "k2"), ("t1", "k1")]),
    );
    assert!(
        matches!(
            reordered.believable(&reaching(&["t1", "t2", "t3"]), &three),
            Err(Refusal::Unreadable { .. })
        ),
        "a run asks in one order, so a record holding the same targets in another was not \
         written by one"
    );
    let noticed = record(
        mutant(4),
        "run-1",
        Outcome::Killed {
            target: "t2".to_owned(),
            key: "k2".to_owned(),
            before: vec![Answer {
                target: "t1".to_owned(),
                key: "k1".to_owned(),
                outcome: njutest::report::Outcome::Killed,
            }],
        },
    );
    assert!(
        matches!(
            noticed.believable(&reaching(&["t1", "t2"]), &three),
            Err(Refusal::Unreadable { .. })
        ),
        "a run stops at the first kill it confirms, so an earlier kill among the answers \
         before one is a record no run wrote"
    );
}
