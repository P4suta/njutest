// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Answer, Observed, Wait, agrees, answer};

fn symbolic_wait() -> Wait {
    let index = kani::any::<u8>();
    kani::assume(index < 3);
    match index {
        0 => Wait::Exited,
        1 => Wait::Answered,
        _ => Wait::Other,
    }
}

#[kani::proof]
fn an_answer_is_decided_by_the_wait_and_the_named_failure_alone() {
    let wait = symbolic_wait();
    let named = kani::any::<bool>();
    let said = answer(wait, named);
    let ruled = match (wait, named) {
        (Wait::Exited | Wait::Answered, true) => Answer::Answered,
        (Wait::Answered, false) => Answer::Contradicted,
        (Wait::Exited, false) | (Wait::Other, _) => Answer::Unanswered,
    };
    kani::assert(
        said == ruled,
        "njutest-law-assertion:answer-as-the-wait-says",
    );
    kani::cover!(said == Answer::Answered, "njutest-law-branch:answered");
    kani::cover!(said == Answer::Unanswered, "njutest-law-branch:unanswered");
    kani::cover!(
        said == Answer::Contradicted,
        "njutest-law-branch:contradicted"
    );
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
fn a_reported_answer_agrees_exactly_where_the_observation_decides() {
    let observed = Observed {
        wait: symbolic_wait(),
        named_a_failure: kani::any::<bool>(),
        capture_failed: kani::any::<bool>(),
    };
    let decided = observed.named_a_failure
        && !observed.capture_failed
        && !matches!(observed.wait, Wait::Other);
    let contradicted = matches!(observed.wait, Wait::Answered) && !observed.named_a_failure;
    kani::assert(
        agrees(observed, true) == (decided && !contradicted),
        "njutest-law-assertion:true-agrees-only-with-decided",
    );
    kani::assert(
        agrees(observed, false) == (!decided && !contradicted),
        "njutest-law-assertion:false-agrees-only-with-undecided",
    );
    kani::cover!(decided && !contradicted, "njutest-law-branch:answered");
    kani::cover!(!decided && !contradicted, "njutest-law-branch:unanswered");
    kani::cover!(contradicted, "njutest-law-branch:contradicted");
    kani::cover!(true, "njutest-law-reached");
}
