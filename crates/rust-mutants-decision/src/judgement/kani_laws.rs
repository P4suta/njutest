// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Account, Ending, Harness, Observed, judged};
use crate::evidence::{Detection, Doubt, Sealed};

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

fn symbolic_test_result() -> Account {
    let index = kani::any::<u8>();
    kani::assume(index < 3);
    match index {
        0 => Account::Passed,
        1 => Account::Failed,
        _ => Account::Other,
    }
}

fn symbolic_harness() -> Harness {
    let index = kani::any::<u8>();
    kani::assume(index < 3);
    match index {
        0 => Harness::Libtest(symbolic_test_result()),
        1 => Harness::Doctest,
        _ => Harness::ShouldPanic,
    }
}

fn symbolic_observed() -> Observed {
    Observed {
        ending: symbolic_ending(),
        harness: symbolic_harness(),
        beyond_control: kani::any(),
        matched: kani::any(),
    }
}

#[kani::proof]
fn a_pass_is_only_the_ending_its_harness_passes_by() {
    let observed = symbolic_observed();
    let passed = judged(observed) == Sealed::Passed;
    let passing = match observed.harness {
        Harness::Libtest(harness_report) => {
            observed.ending == Ending::Returned && harness_report == Account::Passed
        }
        Harness::Doctest => observed.ending == Ending::Returned,
        Harness::ShouldPanic => matches!(
            observed.ending,
            Ending::ExitedFailure | Ending::Panicked | Ending::Aborted | Ending::Trapped
        ),
    };
    kani::assert(
        passed == (passing && !observed.beyond_control),
        "njutest-law-assertion:pass-iff-its-harness-passes",
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

#[kani::proof]
fn a_bound_no_control_of_the_tests_own_set_is_never_a_detection() {
    let observed = symbolic_observed();
    kani::assume(!observed.beyond_control && !observed.matched);
    let said = judged(observed);
    kani::assert(
        !matches!(
            said,
            Sealed::Detected(Detection::FuelExceeded | Detection::MemoryExceeded)
        ),
        "njutest-law-assertion:unmatched-bound-never-detects",
    );
    let bounded = matches!(
        observed.ending,
        Ending::FuelExhausted | Ending::MemoryExhausted
    );
    if bounded {
        kani::assert(
            said == Sealed::Doubted(Doubt::Unmatched),
            "njutest-law-assertion:unmatched-bound-doubted",
        );
    }
    kani::cover!(bounded, "njutest-law-branch:bounded");
    kani::cover!(!bounded, "njutest-law-branch:unbounded");
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn a_doctest_that_should_panic_is_detected_only_by_not_failing() {
    let observed = Observed {
        ending: symbolic_ending(),
        harness: Harness::ShouldPanic,
        beyond_control: false,
        matched: true,
    };
    let detected = matches!(judged(observed), Sealed::Detected(_));
    kani::assert(
        detected == matches!(observed.ending, Ending::Returned | Ending::FuelExhausted),
        "njutest-law-assertion:should-panic-detected-iff-not-failing",
    );
    kani::cover!(detected, "njutest-law-branch:detected");
    kani::cover!(!detected, "njutest-law-branch:undetected");
    kani::cover!(true, "njutest-law-reached");
}
