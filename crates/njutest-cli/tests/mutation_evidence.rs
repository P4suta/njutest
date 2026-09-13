// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one run may believe of what an earlier one established about one mutation, and what it records for the next.

#![expect(
    clippy::expect_used,
    reason = "the helpers that build one run's evidence are not themselves tests, and a store that cannot be written is a setup failure to report by panicking"
)]

use std::collections::BTreeMap;

use njutest_cli::assure::mutation::{
    Consulted, Disposition, Evidence, MutationOptions, keep, reuse,
};
use njutest_cli::evidence::store::{self, Standing};
use rust_mutants::session::{Fallback, Reaches, Route};

/// Two targets a run's baseline saw pass, by the identity a route names and the name a reader reads.
fn evidence(root: &std::path::Path) -> Evidence {
    Evidence {
        root: root.to_path_buf(),
        run_id: "20260909T000000Z-000002".to_owned(),
        standing: Standing {
            passing: BTreeMap::from([
                ("id-one".to_owned(), "k1".repeat(16)),
                ("id-two".to_owned(), "k2".repeat(16)),
            ]),
        },
        names: BTreeMap::from([
            ("id-one".to_owned(), "core/lib/core".to_owned()),
            ("id-two".to_owned(), "core/test/wide".to_owned()),
        ]),
    }
}

const fn options(evidence: Option<Evidence>) -> MutationOptions {
    MutationOptions {
        accepted: std::collections::BTreeSet::new(),
        test_args: Vec::new(),
        evidence,
        jobs: 1,
        exclusive: false,
        shard: None,
    }
}

/// What one consultation of the store ended on: what it believed, or the word it refused with.
fn said(consulted: &Consulted) -> String {
    match consulted {
        Consulted::NotKept => "not-kept".to_owned(),
        Consulted::Believed { run_id, .. } => format!("believed {run_id}"),
        Consulted::Refused(refusal) => refusal.name().to_owned(),
        other => format!("a shape this test does not know: {other:?}"),
    }
}

fn reaching(names: &[&str]) -> Route {
    Route::All {
        reaching: names.iter().map(|name| (*name).to_owned()).collect(),
        fallback: Fallback::NotMeasured,
    }
}

#[test]
fn a_kill_an_earlier_run_recorded_is_read_back_under_the_name_a_reader_reads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let held = evidence(dir.path());
    store::write(
        dir.path(),
        &store::record(
            "m1",
            "20260909T000000Z-000001",
            store::Outcome::Killed {
                target: "id-one".to_owned(),
                key: "k1".repeat(16),
            },
        ),
    )
    .expect("an earlier run's record");

    let consulted = reuse(&options(Some(held)), &reaching(&["core/lib/core"]), "m1");
    let Consulted::Believed {
        disposition,
        run_id,
    } = consulted
    else {
        panic!("a kill this run may believe");
    };
    assert_eq!(
        disposition,
        Disposition::Killed {
            by: "core/lib/core".to_owned()
        },
        "a record names the target by identity and a report names it the way a person \
         does, so reading one back has to turn the first into the second: a report that \
         quoted the identity would name a test nobody can run"
    );
    assert_eq!(
        run_id, "20260909T000000Z-000001",
        "and says whose answer it is"
    );
}

#[test]
fn a_run_that_cannot_resolve_every_target_a_route_names_believes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let held = evidence(dir.path());
    store::write(
        dir.path(),
        &store::record(
            "m1",
            "20260909T000000Z-000001",
            store::Outcome::Survived {
                targets: BTreeMap::from([("id-one".to_owned(), "k1".repeat(16))]),
            },
        ),
    )
    .expect("an earlier run's record");

    assert_eq!(
        said(&reuse(
            &options(Some(held.clone())),
            &reaching(&["core/lib/core", "core/lib/newcomer"]),
            "m1",
        )),
        "target-unknown",
        "reuse is a claim about a set, and a set this run can only half resolve is one \
         it may neither believe nor record: the half it resolved is a smaller claim \
         wearing the same name, and believing it would report that nothing noticed a \
         mutation a target nobody looked up might have"
    );
    assert_eq!(
        said(&reuse(
            &options(Some(held.clone())),
            &reaching(&["core/lib/core", "core/test/wide"]),
            "m1",
        )),
        "target-entered",
        "and a survival recorded against one target is not a survival against two: the \
         target that entered since is one the earlier run never ran"
    );
    assert_eq!(
        said(&reuse(
            &options(Some(held)),
            &reaching(&["core/lib/core"]),
            "m1"
        )),
        "believed 20260909T000000Z-000001",
        "while the set the earlier run answered for is one this run may believe"
    );
}

#[test]
fn what_is_recorded_is_what_the_next_run_can_check_and_nothing_else() {
    let dir = tempfile::tempdir().expect("tempdir");
    let held = options(Some(evidence(dir.path())));

    keep(
        &held,
        "m1",
        &reaching(&["core/lib/core"]),
        &Disposition::Killed {
            by: "core/lib/nobody".to_owned(),
        },
    );
    assert!(
        store::read(dir.path(), "m1").expect("readable").is_none(),
        "a kill by a target this run's baseline never saw pass is one the next run has \
         no key to check, so it is not written: a record nobody can refuse is a record \
         nobody can believe either"
    );

    keep(
        &held,
        "m2",
        &reaching(&["core/lib/core", "core/lib/newcomer"]),
        &Disposition::Survived {
            route: reaching(&["core/lib/core", "core/lib/newcomer"]),
        },
    );
    assert!(
        store::read(dir.path(), "m2").expect("readable").is_none(),
        "and a survival over a set this run cannot resolve whole is not written either, \
         for the same reason it would not be believed"
    );

    for (mutant, disposition) in [
        (
            "m3",
            Disposition::TimedOut {
                on: "core/lib/core".to_owned(),
            },
        ),
        ("m4", Disposition::Unreached),
    ] {
        keep(&held, mutant, &reaching(&["core/lib/core"]), &disposition);
        assert!(
            store::read(dir.path(), mutant).expect("readable").is_none(),
            "and what a run says about itself rather than about the mutant is not \
             recorded at all: a mutation that ran out of this run's time is not one the \
             next run's tests cannot notice, and one this run routed nothing to is a \
             fact about the routing: {disposition:?}"
        );
    }

    recorded(dir.path(), &held);
}

/// What a run does record, once it can name every target and check every key.
fn recorded(root: &std::path::Path, held: &MutationOptions) {
    let dir = root;
    keep(
        held,
        "m5",
        &reaching(&["core/lib/core"]),
        &Disposition::Killed {
            by: "core/lib/core".to_owned(),
        },
    );
    keep(
        held,
        "m6",
        &reaching(&["core/lib/core", "core/test/wide"]),
        &Disposition::Survived {
            route: reaching(&["core/lib/core", "core/test/wide"]),
        },
    );
    let survival = store::read(dir, "m6")
        .expect("readable")
        .expect("a survival worth keeping");
    assert_eq!(
        survival.outcome,
        store::Outcome::Survived {
            targets: BTreeMap::from([
                ("id-one".to_owned(), "k1".repeat(16)),
                ("id-two".to_owned(), "k2".repeat(16)),
            ]),
        },
        "a survival is the universal claim, so what is recorded is every target it is \
         about with the key the next run checks each of them against: one key short and \
         the next run believes a claim over a set nobody answered for"
    );
    keep(
        held,
        "m7",
        &reaching(&[]),
        &Disposition::Survived {
            route: reaching(&[]),
        },
    );
    assert!(
        store::read(dir, "m7").expect("readable").is_none(),
        "and a survival over no targets at all is not a survival: nothing ran, so there \
         is nothing for the next run to believe"
    );
    let kept = store::read(dir, "m5")
        .expect("readable")
        .expect("a kill worth keeping");
    assert_eq!(
        kept.outcome,
        store::Outcome::Killed {
            target: "id-one".to_owned(),
            key: "k1".repeat(16),
        },
        "a kill this run can attribute is recorded by identity and by the behaviour key \
         the next run will check it against"
    );
    assert_eq!(kept.run_id, "20260909T000000Z-000002");
}

#[test]
fn a_record_this_run_cannot_read_is_not_a_run_with_no_record() {
    let dir = tempfile::tempdir().expect("tempdir");
    let held = evidence(dir.path());
    assert_eq!(
        said(&reuse(
            &options(Some(held.clone())),
            &reaching(&["core/lib/core"]),
            "nobody",
        )),
        "nothing-recorded",
        "a mutation no earlier run answered for is one this run establishes itself, and \
         says so: a store that was asked and had nothing is not a run that never asked"
    );

    std::fs::create_dir_all(store::path_of(dir.path(), "m1"))
        .expect("a directory where a record goes");
    assert_eq!(
        said(&reuse(
            &options(Some(held)),
            &reaching(&["core/lib/core"]),
            "m1"
        )),
        "unreadable",
        "and a record this run cannot read is one it does not believe rather than one \
         it stops for: the answer it could not read is re-established, which is the \
         direction a cache may be wrong in. It is not the same as no record either — \
         one is a cold store and the other is a store that has stopped working, and a \
         run that reports both as silence leaves nobody able to tell which they have"
    );
}

#[test]
fn a_run_with_nowhere_to_read_or_write_evidence_neither_believes_nor_records() {
    let dir = tempfile::tempdir().expect("tempdir");
    let none = options(None);
    assert_eq!(
        said(&reuse(&none, &reaching(&["core/lib/core"]), "m1")),
        "not-kept",
        "a run holding no evidence has nothing to believe, and nothing to refuse either: \
         a run that keeps no store and one whose store refuses everything do the same \
         work, and only this parts them"
    );
    keep(
        &none,
        "m1",
        &reaching(&["core/lib/core"]),
        &Disposition::Killed {
            by: "core/lib/core".to_owned(),
        },
    );
    assert!(
        !dir.path().join("mutants-v1").exists(),
        "and writes nowhere rather than choosing a place nobody asked for"
    );
}

#[test]
fn the_targets_a_route_answers_for_are_the_ones_it_names_whatever_shape_it_is() {
    let dir = tempfile::tempdir().expect("tempdir");
    let held = evidence(dir.path());
    assert_eq!(
        held.identities(&["core/test/wide", "core/lib/core"]),
        Ok(vec!["id-two".to_owned(), "id-one".to_owned()]),
        "the identities come back in the order the route named them, because a route is \
         a list a reader compares against the report's own"
    );
    assert_eq!(
        held.identities(&["core/lib/core", "core/lib/newcomer"]),
        Err(store::Refusal::TargetUnknown {
            target: "core/lib/newcomer".to_owned()
        }),
        "and one name that resolves to nothing makes the whole set unresolvable rather \
         than a shorter set, and names the one that did not resolve"
    );
    assert_eq!(held.identity("core/lib/core"), Some("id-one"));
    assert_eq!(held.identity("id-one"), None, "a name is not an identity");

    let block = Route::Block {
        reaching: vec![Reaches {
            target: "core/test/wide".to_owned(),
            tests: rust_mutants::session::Asked::Every,
        }],
        discharged: Vec::new(),
        fallback: None,
    };
    store::write(
        dir.path(),
        &store::record(
            "m1",
            "20260909T000000Z-000001",
            store::Outcome::Survived {
                targets: BTreeMap::from([("id-two".to_owned(), "k2".repeat(16))]),
            },
        ),
    )
    .expect("an earlier run's record");
    assert_eq!(
        said(&reuse(&options(Some(held)), &block, "m1")),
        "believed 20260909T000000Z-000001",
        "and a route that named its targets one way is answered for the same way as one \
         that named them another: what a record is about is the set, not the shape of \
         the route that produced it"
    );
}

/// One target a baseline measured, as the mutation phase holds it.
fn measured(name: &str) -> njutest_cli::assure::baseline::Measured {
    njutest_cli::assure::baseline::Measured {
        target: njutest_cli::targets::Target {
            id: format!("id-{name}"),
            package: "core".to_owned(),
            unit: njutest_cli::targets::UnitKind::Lib,
            unit_name: "core".to_owned(),
            path: name.to_owned(),
            ignored: false,
            executable: std::path::PathBuf::from("/nowhere"),
            cwd: std::path::PathBuf::from("/nowhere"),
            env: Vec::new(),
        },
        status: njutest_cli::report::TargetStatus::Passed,
        duration_ms: 1,
        tests: 1,
        message: None,
    }
}

#[test]
fn a_kill_is_confirmed_against_the_test_that_found_it_and_not_against_the_rest() {
    use njutest_cli::assure::mutation::{narrowed, request_for};

    let one = measured("cases::adds");
    let asked = request_for("m1", Some(&one), &["--nocapture".to_owned()]);
    assert_eq!(
        asked.target.as_deref(),
        Some("core/lib/core cases::adds"),
        "a request against a measured target names it the way a person does, which is \
         the unit and the test together"
    );
    assert_eq!(asked.args, ["--nocapture"]);
    let held = narrowed(asked.clone(), Some(&one), "core/lib/core");
    assert_eq!(
        (held.target, held.test, held.args),
        (asked.target.clone(), asked.test.clone(), asked.args),
        "a request that already named a target is the request the pair is put to: \
         narrowing it to whatever the harness reported would confirm a kill against a \
         target nobody routed to"
    );

    let suite = request_for("m1", None, &["--quiet".to_owned()]);
    assert_eq!(
        (
            suite.target.clone(),
            suite.mutant.clone(),
            suite.args.clone()
        ),
        (None, "m1".to_owned(), vec!["--quiet".to_owned()]),
        "a request with no proof of who could notice runs every target, and still names \
         the mutation and carries what the run was told to pass the harness"
    );
    let confirming = narrowed(suite.clone(), None, "core/lib/core");
    assert_eq!(
        confirming.target.as_deref(),
        Some("core/lib/core"),
        "when the suite answers, the pair is put to the target that answered: asking the \
         whole suite again puts both halves of the confirmation to every other target as \
         well, which is the cost of the answer multiplied by the size of the workspace"
    );
    assert_eq!(
        confirming.test, None,
        "and to the whole of that target, because which of its tests found it is a \
         question the harness answered and this one did not ask"
    );
    let untouched = narrowed(suite.clone(), None, "");
    assert_eq!(
        (untouched.target, untouched.test),
        (suite.target.clone(), suite.test),
        "while an execution that did not say who answered leaves the request alone: \
         narrowing to a target nobody named would confirm against nothing"
    );
}
