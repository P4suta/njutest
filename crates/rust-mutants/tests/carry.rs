// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The carry rule: an answer is believed on another tree exactly when every premise of ADR 0041 holds.

use std::collections::BTreeSet;

use rust_mutants::carry::{
    Body, Carried, Entered, Execution, Locus, MalformedCarried, Now, Planned, Refusal, SCHEMA,
    Sealing, believe, key,
};
use rust_mutants::outcomes::{CacheOutcome, Keyed};
use rust_mutants::touch::{Completeness, ItemRef};

const TARGET: &str = "pkg/test/lib";
const OTHER: &str = "pkg/test/other";
const SKELETON: &str = "skeleton-0";

fn item(ordinal: u32) -> ItemRef {
    ItemRef {
        package: "pkg".to_owned(),
        path: "src/lib.rs".to_owned(),
        ordinal,
    }
}

fn entered(ordinal: u32, digest: &str) -> Entered {
    Entered {
        item: item(ordinal),
        body_digest: digest.to_owned(),
    }
}

fn execution(target: &str, detected: bool, completeness: Completeness) -> Execution {
    Execution {
        target: target.to_owned(),
        filter: None,
        skeleton: SKELETON.to_owned(),
        entered: BTreeSet::from([entered(0, "body-0")]),
        completeness,
        detected,
    }
}

fn keyed(closure: &str) -> Keyed {
    Keyed {
        closure: closure.to_owned(),
        manifests: "manifests".to_owned(),
        toolchain: "toolchain".to_owned(),
        engine: "engine".to_owned(),
        args: Vec::new(),
        timeout: "auto".to_owned(),
        steps: 0,
        build: Vec::new(),
    }
}

fn locus() -> Locus {
    Locus {
        item: item(0),
        body_digest: "body-0".to_owned(),
        start: 4,
        end: 5,
        replacement: "-".to_owned(),
        rule: "arith@1".to_owned(),
    }
}

fn record(outcome: CacheOutcome, executions: Vec<Execution>) -> Carried {
    let target = executions
        .last()
        .map(|one| one.target.clone())
        .unwrap_or_default();
    Carried {
        schema: SCHEMA.to_owned(),
        locus: locus(),
        keyed: keyed("closure-0"),
        outcome,
        target,
        tests_run: Some(1),
        failed_tests: Vec::new(),
        run_id: "run-0".to_owned(),
        executions,
    }
}

fn now() -> Now {
    let mut now = Now::default();
    for target in [TARGET, OTHER] {
        now.skeletons.insert(target.to_owned(), SKELETON.to_owned());
        now.held.insert(target.to_owned());
    }
    for (ordinal, digest) in [(0, "body-0"), (1, "body-1")] {
        now.items.insert(
            item(ordinal),
            Body {
                digest: digest.to_owned(),
                sealing: Sealing::Sealed,
            },
        );
    }
    now
}

fn plan(targets: &[&str]) -> Vec<Planned> {
    targets
        .iter()
        .map(|target| Planned {
            target: (*target).to_owned(),
            filter: None,
        })
        .collect()
}

fn kill() -> Carried {
    record(
        CacheOutcome::Killed,
        vec![
            execution(OTHER, false, Completeness::Whole),
            execution(TARGET, true, Completeness::Whole),
        ],
    )
}

fn survival() -> Carried {
    record(
        CacheOutcome::Survived,
        vec![
            execution(OTHER, false, Completeness::Whole),
            execution(TARGET, false, Completeness::Whole),
        ],
    )
}

#[test]
fn an_answer_carries_across_an_edit_to_a_body_no_execution_entered() {
    let mut edited = now();
    if let Some(body) = edited.items.get_mut(&item(1)) {
        body.digest = "body-1-edited".to_owned();
    }
    assert_eq!(believe(&kill(), &edited, &plan(&[OTHER, TARGET])), Ok(()));
    assert_eq!(
        believe(&survival(), &edited, &plan(&[OTHER, TARGET])),
        Ok(())
    );
}

#[test]
fn an_edit_to_a_body_an_execution_entered_refuses_it() {
    let mut edited = now();
    if let Some(body) = edited.items.get_mut(&item(0)) {
        body.digest = "body-0-edited".to_owned();
    }
    assert_eq!(
        believe(&kill(), &edited, &plan(&[OTHER, TARGET])),
        Err(Refusal::ItemChanged)
    );
    let mut gone = now();
    gone.items.remove(&item(0));
    assert_eq!(
        believe(&survival(), &gone, &plan(&[OTHER, TARGET])),
        Err(Refusal::ItemChanged),
        "an entered item that is no longer there is a changed one"
    );
}

#[test]
fn an_entered_body_that_is_no_longer_sealed_refuses_it() {
    let mut unsealed = now();
    if let Some(body) = unsealed.items.get_mut(&item(0)) {
        body.sealing = Sealing::Unsealed;
    }
    assert_eq!(
        believe(&kill(), &unsealed, &plan(&[OTHER, TARGET])),
        Err(Refusal::Unsealed)
    );
}

#[test]
fn a_changed_skeleton_refuses_it() {
    let mut changed = now();
    changed
        .skeletons
        .insert(TARGET.to_owned(), "skeleton-1".to_owned());
    assert_eq!(
        believe(&kill(), &changed, &plan(&[OTHER, TARGET])),
        Err(Refusal::SkeletonChanged)
    );
    let mut missing = now();
    missing.skeletons.remove(OTHER);
    assert_eq!(
        believe(&survival(), &missing, &plan(&[OTHER, TARGET])),
        Err(Refusal::SkeletonChanged),
        "a target with no skeleton now has not kept the one it had"
    );
}

#[test]
fn a_target_whose_reach_did_not_hold_refuses_it() {
    let mut moved = now();
    moved.held.remove(TARGET);
    assert_eq!(
        believe(&kill(), &moved, &plan(&[OTHER, TARGET])),
        Err(Refusal::ReachMoved)
    );
}

#[test]
fn a_kill_needs_its_killer_to_have_named_what_it_entered_up_to_the_kill() {
    for (reach, expected) in [
        (Completeness::Whole, Ok(())),
        (Completeness::UpToFirstFailure, Ok(())),
        (Completeness::Cut, Err(Refusal::EntryIncomplete)),
    ] {
        let carried = record(
            CacheOutcome::Killed,
            vec![
                execution(OTHER, false, Completeness::Cut),
                execution(TARGET, true, reach),
            ],
        );
        assert_eq!(
            believe(&carried, &now(), &plan(&[OTHER, TARGET])),
            expected,
            "{reach:?}; the executions before the killer are not what the kill rests on"
        );
    }
}

#[test]
fn a_survival_needs_every_execution_to_have_named_everything_it_entered() {
    for reach in [Completeness::UpToFirstFailure, Completeness::Cut] {
        let carried = record(
            CacheOutcome::Survived,
            vec![
                execution(OTHER, false, Completeness::Whole),
                execution(TARGET, false, reach),
            ],
        );
        assert_eq!(
            believe(&carried, &now(), &plan(&[OTHER, TARGET])),
            Err(Refusal::EntryIncomplete),
            "{reach:?}"
        );
    }
}

#[test]
fn a_survival_answers_only_for_a_route_its_executions_cover() {
    assert_eq!(
        believe(&survival(), &now(), &plan(&[TARGET])),
        Ok(()),
        "a route that runs fewer targets than were recorded is covered"
    );
    assert_eq!(
        believe(
            &survival(),
            &now(),
            &plan(&[TARGET, OTHER, "pkg/test/third"])
        ),
        Err(Refusal::RouteGrew)
    );
}

#[test]
fn a_different_filter_refuses_it_and_a_narrower_one_is_not_enough() {
    let mut narrowed = plan(&[OTHER, TARGET]);
    if let Some(one) = narrowed.last_mut() {
        one.filter = Some(vec!["lib::works".to_owned()]);
    }
    assert_eq!(
        believe(&survival(), &now(), &narrowed),
        Err(Refusal::FilterDiffers)
    );
    assert_eq!(
        believe(&kill(), &now(), &narrowed),
        Err(Refusal::FilterDiffers)
    );
}

#[test]
fn a_kill_by_a_target_the_route_no_longer_runs_is_refused() {
    assert_eq!(
        believe(&kill(), &now(), &plan(&[OTHER])),
        Err(Refusal::FilterDiffers)
    );
}

#[test]
fn the_key_holds_the_locus_and_not_the_closure() {
    let base = key(&keyed("closure-0"), &locus());
    assert_eq!(
        base,
        key(&keyed("closure-1"), &locus()),
        "an edit anywhere changes the closure, and the locus key must survive it"
    );
    let loci: [fn(&mut Locus); 7] = [
        |one| one.item.package = "other".to_owned(),
        |one| one.item.path = "src/other.rs".to_owned(),
        |one| one.item.ordinal = 1,
        |one| one.body_digest = "body-9".to_owned(),
        |one| one.start = 5,
        |one| one.end = 6,
        |one| one.replacement = "*".to_owned(),
    ];
    for change in loci {
        let mut changed = locus();
        change(&mut changed);
        assert_ne!(base, key(&keyed("closure-0"), &changed), "{changed:?}");
    }
    let mut ruled = locus();
    ruled.rule = "arith@2".to_owned();
    assert_ne!(base, key(&keyed("closure-0"), &ruled));
    let keys: [fn(&mut Keyed); 7] = [
        |one| one.manifests = "m".to_owned(),
        |one| one.toolchain = "t".to_owned(),
        |one| one.engine = "e".to_owned(),
        |one| one.args = vec!["--x".to_owned()],
        |one| one.timeout = "5s".to_owned(),
        |one| one.steps = 9,
        |one| one.build = vec!["--release".to_owned()],
    ];
    for change in keys {
        let mut changed = keyed("closure-0");
        change(&mut changed);
        assert_ne!(base, key(&changed, &locus()), "{changed:?}");
    }
}

#[test]
fn a_record_whose_executions_do_not_end_as_its_outcome_says_is_malformed() {
    assert_eq!(kill().validate(), Ok(()));
    assert_eq!(survival().validate(), Ok(()));
    let mut unkilled = kill();
    unkilled.outcome = CacheOutcome::Survived;
    assert!(matches!(
        unkilled.validate(),
        Err(MalformedCarried::Outcome { .. })
    ));
    let mut empty = kill();
    empty.executions.clear();
    assert!(matches!(
        empty.validate(),
        Err(MalformedCarried::Outcome { .. })
    ));
    let mut elsewhere = kill();
    elsewhere.target = OTHER.to_owned();
    assert!(matches!(
        elsewhere.validate(),
        Err(MalformedCarried::Target { .. })
    ));
}

#[test]
fn every_refusal_has_its_own_word() {
    let words: BTreeSet<&str> = Refusal::ALL.iter().map(|one| one.name()).collect();
    assert_eq!(words.len(), Refusal::ALL.len(), "{words:?}");
}
