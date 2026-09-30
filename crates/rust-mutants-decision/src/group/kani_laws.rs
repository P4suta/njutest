// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Delivered, Others, StopDecision, Stopped, agrees, decide_stop};

fn symbolic_delivered() -> Delivered {
    let index = kani::any::<u8>();
    kani::assume(index < 4);
    match index {
        0 => Delivered::Sent,
        1 => Delivered::Gone,
        2 => Delivered::Refused,
        _ => Delivered::Failed,
    }
}

fn symbolic_others() -> Others {
    let index = kani::any::<u8>();
    kani::assume(index < 3);
    match index {
        0 => Others::Nobody,
        1 => Others::Somebody,
        _ => Others::Unseen,
    }
}

fn symbolic_decision() -> StopDecision {
    let index = kani::any::<u8>();
    kani::assume(index < 3);
    match index {
        0 => StopDecision::Reached(Stopped::Group),
        1 => StopDecision::Reached(Stopped::LeaderOnly),
        _ => StopDecision::Failed,
    }
}

#[kani::proof]
fn a_stop_agrees_exactly_with_what_the_kernel_answered() {
    let group = symbolic_delivered();
    let leader = symbolic_delivered();
    let others = symbolic_others();
    let said = decide_stop(group, leader, others);
    let expected = if group == Delivered::Failed {
        StopDecision::Failed
    } else if group != Delivered::Refused
        || others == Others::Nobody && matches!(leader, Delivered::Sent | Delivered::Gone)
    {
        StopDecision::Reached(Stopped::Group)
    } else if matches!(leader, Delivered::Refused | Delivered::Failed) {
        StopDecision::Failed
    } else {
        StopDecision::Reached(Stopped::LeaderOnly)
    };
    kani::assert(
        said == expected,
        "njutest-law-assertion:stop-as-the-kernel-says",
    );
    let reported = symbolic_decision();
    kani::assert(
        agrees(group, leader, others, reported) == (reported == expected),
        "njutest-law-assertion:agrees-only-with-the-decision",
    );
    kani::cover!(
        said == StopDecision::Reached(Stopped::Group),
        "njutest-law-branch:group"
    );
    kani::cover!(
        said == StopDecision::Reached(Stopped::LeaderOnly),
        "njutest-law-branch:leader-only"
    );
    kani::cover!(said == StopDecision::Failed, "njutest-law-branch:failed");
    kani::cover!(true, "njutest-law-reached");
}
