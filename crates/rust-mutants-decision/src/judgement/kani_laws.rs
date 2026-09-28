// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Account, Ending, Observed, judged};
use crate::evidence::{Doubt, Sealed};

fn symbolic_ending() -> Ending {
    let index = kani::any::<u8>();
    kani::assume(index < 10);
    match index {
        0 => Ending::Returned,
        1 => Ending::ExitedZero,
        2 => Ending::ExitedFailure,
        3 => Ending::ExitedOther,
        4 => Ending::Panicked,
        5 => Ending::Aborted,
        6 => Ending::Trapped,
        7 => Ending::StackOverflow,
        8 => Ending::FuelExhausted,
        _ => Ending::MemoryExhausted,
    }
}

fn symbolic_account() -> Account {
    let index = kani::any::<u8>();
    kani::assume(index < 3);
    match index {
        0 => Account::Passed,
        1 => Account::Failed,
        _ => Account::Other,
    }
}

fn symbolic_observed() -> Observed {
    Observed {
        ending: symbolic_ending(),
        account: symbolic_account(),
        beyond_control: kani::any(),
    }
}

#[kani::proof]
fn a_pass_is_only_a_returned_instance_its_harness_accounted_for() {
    let observed = symbolic_observed();
    let passed = judged(observed) == Sealed::Passed;
    kani::assert(
        passed
            == (observed.ending == Ending::Returned
                && observed.account == Account::Passed
                && !observed.beyond_control),
        "njutest-law-assertion:pass-iff-returned-and-accounted",
    );
    kani::cover!(passed, "njutest-law-branch:passed");
    kani::cover!(!passed, "njutest-law-branch:not-passed");
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn a_refusal_beyond_the_control_is_never_a_verdict() {
    let observed = symbolic_observed();
    kani::assume(observed.beyond_control);
    kani::assert(
        judged(observed) == Sealed::Doubted(Doubt::Refused),
        "njutest-law-assertion:beyond-control-refused",
    );
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn an_ending_no_two_hosts_decide_alike_is_never_a_verdict() {
    let observed = symbolic_observed();
    kani::assume(!observed.beyond_control);
    let said = judged(observed);
    let undecidable = matches!(
        observed.ending,
        Ending::StackOverflow | Ending::ExitedZero | Ending::ExitedOther
    );
    if undecidable {
        kani::assert(
            matches!(said, Sealed::Doubted(_)),
            "njutest-law-assertion:undecidable-ending-doubted",
        );
    }
    kani::cover!(undecidable, "njutest-law-branch:undecidable");
    kani::cover!(!undecidable, "njutest-law-branch:decidable");
    kani::cover!(true, "njutest-law-reached");
}
