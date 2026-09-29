// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Invocations: what an input may be, and a digest every input moves.

use std::num::NonZeroU64;

use rust_mutants_sealed::{
    Arguments, Environment, EnvironmentFault, Invocation, PreopenFault, Preopens, SealedDigest,
    SealedError, Snapshot, SnapshotBuilder, WorkingFault,
};

use crate::common::{command, invocation, run, runner, snapshot, tree, working};

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
        (vec![tree("", snapshot())], PreopenFault::Empty),
        (vec![tree("/a\0", snapshot())], PreopenFault::HoldsNul),
        (
            vec![tree("/a", snapshot()), tree("/a", snapshot())],
            PreopenFault::Repeated,
        ),
        (
            vec![tree("/a", snapshot()), tree("a/", snapshot())],
            PreopenFault::Repeated,
        ),
        (
            vec![tree("/", snapshot()), working("/", "")],
            PreopenFault::Repeated,
        ),
        (
            vec![
                tree("/a", snapshot()),
                working("/a", ""),
                tree(".", snapshot()),
            ],
            PreopenFault::Repeated,
        ),
        (
            vec![
                tree("/a", snapshot()),
                working("/a", ""),
                working("/a", "empty"),
            ],
            PreopenFault::Repeated,
        ),
    ];
    for (preopens, fault) in cases {
        let shown = format!("{preopens:?}");
        match Preopens::new(preopens) {
            Err(SealedError::Preopen { fault: refused, .. }) => {
                assert_eq!(refused, fault, "{shown}");
            }
            other => panic!("{shown} was not refused: {other:?}"),
        }
    }
}

#[test]
fn a_working_directory_its_tree_does_not_hold_is_refused_by_what_is_wrong_with_it() {
    let cases = [
        (vec![working("/a", "")], WorkingFault::NoTree),
        (
            vec![working("/a", ""), tree("/a", snapshot())],
            WorkingFault::NoTree,
        ),
        (
            vec![tree("/a", snapshot()), working("/b", "")],
            WorkingFault::NoTree,
        ),
        (
            vec![tree("/a", snapshot()), working("/a", "empty/")],
            WorkingFault::NotNames,
        ),
        (
            vec![tree("/a", snapshot()), working("/a", "../empty")],
            WorkingFault::NotNames,
        ),
        (
            vec![tree("/a", snapshot()), working("/a", "/empty")],
            WorkingFault::NotNames,
        ),
        (
            vec![tree("/a", snapshot()), working("/a", ".")],
            WorkingFault::NotNames,
        ),
        (
            vec![tree("/a", snapshot()), working("/a", "empty\0")],
            WorkingFault::NotNames,
        ),
        (
            vec![tree("/a", snapshot()), working("/a", "absent")],
            WorkingFault::NotADirectory,
        ),
        (
            vec![tree("/a", snapshot()), working("/a", "seen.txt")],
            WorkingFault::NotADirectory,
        ),
    ];
    for (preopens, fault) in cases {
        let shown = format!("{preopens:?}");
        match Preopens::new(preopens) {
            Err(SealedError::WorkingDirectory { fault: refused, .. }) => {
                assert_eq!(refused, fault, "{shown}");
            }
            other => panic!("{shown} was not refused: {other:?}"),
        }
    }
    for accepted in [
        vec![tree("/a", snapshot()), working("/a", "")],
        vec![tree("/a", snapshot()), working("/a", "empty")],
        vec![
            tree("/a", snapshot()),
            tree("/b", snapshot()),
            working("/b", "empty"),
        ],
    ] {
        let shown = format!("{accepted:?}");
        assert!(Preopens::new(accepted).is_ok(), "{shown}");
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
    let other_tree = Snapshot::builder()
        .file("seen.txt", b"changed".to_vec())
        .and_then(SnapshotBuilder::build)
        .expect("valid");
    for preopens in [
        vec![tree("/elsewhere", snapshot())],
        vec![tree("/sandbox", other_tree)],
        vec![tree("/sandbox", snapshot()), working("/sandbox", "")],
        vec![tree("/sandbox", snapshot()), working("/sandbox", "empty")],
        vec![tree(r"C:\sandbox", snapshot()), working(r"C:\sandbox", "")],
        vec![tree(r"c:\sandbox", snapshot()), working(r"c:\sandbox", "")],
        vec![tree("C:/sandbox", snapshot()), working("C:/sandbox", "")],
    ] {
        let mut changed = base.clone();
        changed.preopens = Preopens::new(preopens).expect("valid");
        variants.push(changed);
    }
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
    let distinct: std::collections::BTreeSet<SealedDigest> = variants.iter().map(digest).collect();
    assert_eq!(
        distinct.len(),
        variants.len(),
        "two inputs that differ, the working directory and the spelling of a tree's root among \
         them, share a digest"
    );
    let other_module = command(&["sched_yield"], "", "(drop (call $sched_yield))");
    assert_ne!(*run(&other_module, &base).invocation(), reference);
}

#[test]
fn a_module_is_named_by_the_digest_of_its_bytes_and_a_runner_by_its_configuration() {
    let bytes = command(&[], "", "");
    let one = runner();
    let other = runner();
    assert_eq!(
        one.configuration(),
        other.configuration(),
        "two runners on one machine are one configuration"
    );
    let module = one.prepare(&bytes).expect("a valid command");
    assert_eq!(*module.digest(), SealedDigest::of(&bytes));
    let other_bytes = command(&["sched_yield"], "", "(drop (call $sched_yield))");
    assert_ne!(*module.digest(), SealedDigest::of(&other_bytes));
    let spelled = module.digest().to_string();
    assert_eq!(spelled.len(), 64, "{spelled}");
    assert!(
        spelled
            .chars()
            .all(|character| matches!(character, '0'..='9' | 'a'..='f')),
        "{spelled} is lowercase hexadecimal"
    );
}
