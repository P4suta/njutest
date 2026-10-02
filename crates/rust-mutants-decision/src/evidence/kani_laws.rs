// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Detection, Doubt, Execution, Found, Reason, Sealability, Sealed, Standing, standing};

const LONGEST: usize = 3;

fn symbolic_sealability() -> Sealability {
    let index = kani::any::<u8>();
    kani::assume(index < 4);
    match index {
        0 => Sealability::Answerable,
        1 => Sealability::GuardAbsent,
        2 => Sealability::TestAbsent,
        _ => Sealability::ReachDiffers,
    }
}

fn symbolic_execution() -> Execution {
    let index = kani::any::<u8>();
    kani::assume(index < 14);
    match index {
        0 => Execution::Native,
        1 => Execution::Sealed(Sealed::Passed),
        2 => Execution::Sealed(Sealed::SetAside),
        3 => Execution::Sealed(Sealed::Detected(Detection::Panicked)),
        4 => Execution::Sealed(Sealed::Detected(Detection::Failed)),
        5 => Execution::Sealed(Sealed::Detected(Detection::Trapped)),
        6 => Execution::Sealed(Sealed::Detected(Detection::FuelExceeded)),
        7 => Execution::Sealed(Sealed::Detected(Detection::MemoryExceeded)),
        8 => Execution::Sealed(Sealed::Detected(Detection::Declined)),
        9 => Execution::Sealed(Sealed::Doubted(Doubt::ExitedEarly)),
        10 => Execution::Sealed(Sealed::Doubted(Doubt::StackOverflow)),
        11 => Execution::Sealed(Sealed::Doubted(Doubt::Refused)),
        12 => Execution::Sealed(Sealed::Doubted(Doubt::Unaccounted)),
        _ => Execution::Sealed(Sealed::Doubted(Doubt::Unmatched)),
    }
}

fn symbolic_executions() -> ([Execution; LONGEST], usize) {
    let executions = [
        symbolic_execution(),
        symbolic_execution(),
        symbolic_execution(),
    ];
    let length = kani::any::<usize>();
    kani::assume(length <= LONGEST);
    (executions, length)
}

const fn detected(execution: Execution) -> Option<Detection> {
    match execution {
        Execution::Sealed(Sealed::Detected(how)) => Some(how),
        Execution::Sealed(Sealed::Passed | Sealed::Doubted(_) | Sealed::SetAside)
        | Execution::Native => None,
    }
}

const fn class(standing: Standing) -> u8 {
    match standing {
        Standing::Established(verdict) => match verdict.found() {
            Found::Killed { .. } => 0,
            Found::Survived => 1,
            Found::Unreached => 2,
        },
        Standing::Unproven(_) => 3,
    }
}

#[kani::proof]
#[kani::unwind(4)]
fn native_executions_alone_never_establish_a_verdict() {
    let sealability = symbolic_sealability();
    let length = kani::any::<usize>();
    kani::assume(length >= 1 && length <= LONGEST);
    let executions = [Execution::Native; LONGEST];
    let said = standing(sealability, &executions[..length]);
    kani::assert(
        matches!(said, Standing::Unproven(doubts) if doubts.contains(Reason::Native)
            || sealability == Sealability::GuardAbsent),
        "njutest-law-assertion:native-alone-unproven",
    );
    kani::cover!(
        sealability == Sealability::Answerable,
        "njutest-law-branch:answerable"
    );
    kani::cover!(
        sealability == Sealability::GuardAbsent,
        "njutest-law-branch:guard-absent"
    );
    kani::cover!(
        sealability == Sealability::TestAbsent,
        "njutest-law-branch:test-absent"
    );
    kani::cover!(
        sealability == Sealability::ReachDiffers,
        "njutest-law-branch:reach-differs"
    );
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
#[kani::unwind(4)]
fn a_sealed_detection_establishes_the_first_kill() {
    let sealability = symbolic_sealability();
    let (executions, length) = symbolic_executions();
    let given = &executions[..length];
    let first = given
        .iter()
        .enumerate()
        .find_map(|(by, execution)| detected(*execution).map(|how| (by, how)));
    let said = standing(sealability, given);
    if sealability != Sealability::GuardAbsent
        && let Some((by, how)) = first
    {
        kani::assert(
            said == Standing::Established(super::Verdict(Found::Killed { by, how })),
            "njutest-law-assertion:first-sealed-detection-kills",
        );
    }
    kani::assert(
        !matches!(said, Standing::Established(verdict) if matches!(verdict.found(), Found::Killed { .. }))
            || (first.is_some() && sealability != Sealability::GuardAbsent),
        "njutest-law-assertion:a-kill-needs-a-sealed-detection",
    );
    kani::cover!(first.is_some(), "njutest-law-branch:detected");
    kani::cover!(first.is_none(), "njutest-law-branch:undetected");
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
#[kani::unwind(4)]
fn survival_is_universal_over_sealed_passes() {
    let sealability = symbolic_sealability();
    let (executions, length) = symbolic_executions();
    let given = &executions[..length];
    let every_sealed_pass_or_set_aside = given.iter().all(|execution| {
        matches!(
            execution,
            Execution::Sealed(Sealed::Passed | Sealed::SetAside)
        )
    });
    let one_passed = given
        .iter()
        .any(|execution| *execution == Execution::Sealed(Sealed::Passed));
    let said = standing(sealability, given);
    let survived =
        matches!(said, Standing::Established(verdict) if verdict.found() == Found::Survived);
    kani::assert(
        survived
            == (sealability == Sealability::Answerable
                && every_sealed_pass_or_set_aside
                && one_passed),
        "njutest-law-assertion:survival-iff-every-sealed-pass",
    );
    let unreached =
        matches!(said, Standing::Established(verdict) if verdict.found() == Found::Unreached);
    kani::assert(
        unreached == (sealability == Sealability::Answerable && length == 0),
        "njutest-law-assertion:unreached-iff-answerable-and-no-test",
    );
    kani::cover!(survived, "njutest-law-branch:survived");
    kani::cover!(unreached, "njutest-law-branch:unreached");
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
#[kani::unwind(4)]
fn a_mutant_every_test_of_which_declined_as_its_control_did_measured_nothing() {
    let sealability = symbolic_sealability();
    let length = kani::any::<usize>();
    kani::assume(length >= 1 && length <= LONGEST);
    let executions = [Execution::Sealed(Sealed::SetAside); LONGEST];
    let said = standing(sealability, &executions[..length]);
    kani::assert(
        sealability != Sealability::Answerable
            || said == Standing::Unproven(super::Doubts::of(Reason::Declined)),
        "njutest-law-assertion:all-set-aside-unproven-declined",
    );
    kani::assert(
        !matches!(said, Standing::Established(_)),
        "njutest-law-assertion:set-aside-alone-never-a-verdict",
    );
    kani::cover!(
        sealability == Sealability::Answerable,
        "njutest-law-branch:answerable"
    );
    kani::cover!(
        sealability != Sealability::Answerable,
        "njutest-law-branch:otherwise"
    );
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
#[kani::unwind(11)]
fn an_unproven_standing_names_a_reason() {
    let sealability = symbolic_sealability();
    let (executions, length) = symbolic_executions();
    let said = standing(sealability, &executions[..length]);
    if let Standing::Unproven(doubts) = said {
        kani::assert(
            Reason::ALL.iter().any(|reason| doubts.contains(*reason)),
            "njutest-law-assertion:unproven-names-a-reason",
        );
    }
    kani::cover!(
        matches!(said, Standing::Unproven(_)),
        "njutest-law-branch:unproven"
    );
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
#[kani::unwind(4)]
fn the_order_of_the_executions_does_not_change_the_class() {
    let sealability = symbolic_sealability();
    let (executions, length) = symbolic_executions();
    kani::assume(length >= 2);
    let mut swapped = executions;
    swapped.swap(0, length - 1);
    kani::assert(
        class(standing(sealability, &executions[..length]))
            == class(standing(sealability, &swapped[..length])),
        "njutest-law-assertion:class-order-independent",
    );
    kani::cover!(length == 2, "njutest-law-branch:two");
    kani::cover!(length == 3, "njutest-law-branch:three");
    kani::cover!(true, "njutest-law-reached");
}
