// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Invocations: what an input may be, and a digest every input moves.

use std::num::NonZeroU64;

use rust_mutants_sealed::{
    Arguments, Environment, EnvironmentFault, Invocation, PreopenFault, Preopens, SealedError,
    Snapshot, SnapshotBuilder,
};

use crate::common::{command, invocation, run, snapshot};

#[test]
fn an_argument_holding_nul_is_refused_by_its_place() {
    let refused = Arguments::new(vec!["program".to_owned(), "a\0b".to_owned()]);
    assert!(matches!(
        refused,
        Err(SealedError::ArgumentHoldsNul { index: 1 })
    ));
}

#[test]
fn an_environment_variable_a_guest_cannot_be_given_is_refused_by_what_is_wrong_with_it() {
    let cases = [
        (("", "value"), EnvironmentFault::EmptyName),
        (("A=B", "value"), EnvironmentFault::NameHoldsEquals),
        (("A\0", "value"), EnvironmentFault::HoldsNul),
        (("A", "va\0lue"), EnvironmentFault::HoldsNul),
    ];
    for ((name, value), fault) in cases {
        match Environment::new(vec![(name.to_owned(), value.to_owned())]) {
            Err(SealedError::EnvironmentVariable { fault: refused, .. }) => {
                assert_eq!(refused, fault);
            }
            other => panic!("{name:?}={value:?} was not refused: {other:?}"),
        }
    }
    let repeated = Environment::new(vec![
        ("A".to_owned(), "1".to_owned()),
        ("A".to_owned(), "2".to_owned()),
    ]);
    assert!(matches!(
        repeated,
        Err(SealedError::EnvironmentVariable {
            fault: EnvironmentFault::Repeated,
            ..
        })
    ));
}

#[test]
fn a_guest_path_that_cannot_be_preopened_is_refused_by_what_is_wrong_with_it() {
    let cases = [
        (vec![String::new()], PreopenFault::Empty),
        (vec!["/a\0".to_owned()], PreopenFault::HoldsNul),
        (
            vec!["/a".to_owned(), "/a".to_owned()],
            PreopenFault::Repeated,
        ),
    ];
    for (paths, fault) in cases {
        let preopens = paths
            .iter()
            .map(|path| (path.clone(), snapshot()))
            .collect();
        match Preopens::new(preopens) {
            Err(SealedError::Preopen { fault: refused, .. }) => assert_eq!(refused, fault),
            other => panic!("{paths:?} was not refused: {other:?}"),
        }
    }
}

#[test]
fn every_input_moves_the_invocation_digest() {
    let bytes = command(&[], "", "");
    let base = invocation();
    let digest = |asked: &Invocation| *run(&bytes, asked).invocation();
    let reference = digest(&base);
    assert_eq!(reference, digest(&base), "one invocation, one digest");
    let mut variants: Vec<Invocation> = Vec::new();
    let mut changed = base.clone();
    changed.arguments = Arguments::new(vec!["other".to_owned()]).expect("valid");
    variants.push(changed);
    let mut changed = base.clone();
    changed.environment = Environment::new(vec![("A".to_owned(), "1".to_owned())]).expect("valid");
    variants.push(changed);
    let mut changed = base.clone();
    changed.preopens = Preopens::new(vec![("/elsewhere".to_owned(), snapshot())]).expect("valid");
    variants.push(changed);
    let mut changed = base.clone();
    let other_tree = Snapshot::builder()
        .file("seen.txt", b"changed".to_vec())
        .and_then(SnapshotBuilder::build)
        .expect("valid");
    changed.preopens = Preopens::new(vec![("/sandbox".to_owned(), other_tree)]).expect("valid");
    variants.push(changed);
    let mut changed = base.clone();
    changed.seed = base.seed + 1;
    variants.push(changed);
    let mut changed = base.clone();
    changed.fuel = base.fuel + 1;
    variants.push(changed);
    for bump in 0..4 {
        let mut changed = base.clone();
        match bump {
            0 => changed.limits.memory += 65536,
            1 => changed.limits.stdout += 1,
            2 => changed.limits.stderr += 1,
            _ => changed.limits.overlay += 1,
        }
        variants.push(changed);
    }
    for bump in 0..3 {
        let mut changed = base.clone();
        match bump {
            0 => changed.clock.realtime_origin += 1,
            1 => changed.clock.monotonic_origin += 1,
            _ => changed.clock.nanos_per_fuel = NonZeroU64::new(2).expect("nonzero"),
        }
        variants.push(changed);
    }
    for variant in &variants {
        assert_ne!(digest(variant), reference, "{variant:?}");
    }
    let other_module = command(&["sched_yield"], "", "(drop (call $sched_yield))");
    assert_ne!(*run(&other_module, &base).invocation(), reference);
}
