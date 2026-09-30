// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Concluded, Held, concluded, held};

const LONGEST: usize = 2;

fn symbolic_declines() -> ([u8; LONGEST], usize) {
    let declines = [kani::any::<u8>(), kani::any::<u8>()];
    kani::assume(declines.iter().all(|one| *one < 3));
    let length = kani::any::<usize>();
    kani::assume(length <= LONGEST);
    (declines, length)
}

fn first_the_baseline_did_not_make<'a>(declined: &'a [u8], baseline: &[u8]) -> Option<&'a u8> {
    for one in declined {
        let mut made = false;
        for other in baseline {
            made = made || other == one;
        }
        if !made {
            return Some(one);
        }
    }
    None
}

#[kani::proof]
#[kani::unwind(4)]
fn a_decline_is_held_to_the_baselines() {
    let (declined, declined_length) = symbolic_declines();
    let (baseline, baseline_length) = symbolic_declines();
    let (Some(declined), Some(baseline)) = (
        declined.get(..declined_length),
        baseline.get(..baseline_length),
    ) else {
        return;
    };
    let said = held(declined, baseline);
    let ruled = match first_the_baseline_did_not_make(declined, baseline) {
        Some(by) => Held::Detected { by },
        None => Held::SetAside,
    };
    kani::assert(
        said == ruled,
        "njutest-law-assertion:held-names-the-first-unmade",
    );
    kani::cover!(
        matches!(said, Held::Detected { .. }),
        "njutest-law-branch:detected"
    );
    kani::cover!(said == Held::SetAside, "njutest-law-branch:set-aside");
    kani::cover!(true, "njutest-law-reached");
}

#[kani::proof]
#[kani::unwind(4)]
fn a_survival_concludes_as_the_rules_say() {
    let (declined, declined_length) = symbolic_declines();
    let (baseline, baseline_length) = symbolic_declines();
    let (Some(declined), Some(baseline)) = (
        declined.get(..declined_length),
        baseline.get(..baseline_length),
    ) else {
        return;
    };
    let believed = kani::any::<bool>();
    let passed = kani::any::<usize>();
    let said = concluded(believed.then_some(declined), baseline, passed);
    let ruled = if !believed {
        Concluded::Errored
    } else if let Some(by) = first_the_baseline_did_not_make(declined, baseline) {
        Concluded::DeclinedUnderTheMutant { by }
    } else if declined.len() == passed && passed > 0 {
        Concluded::Declined
    } else {
        Concluded::Survived
    };
    kani::assert(
        said == ruled,
        "njutest-law-assertion:concluded-as-the-rules-say",
    );
    kani::cover!(said == Concluded::Errored, "njutest-law-branch:errored");
    kani::cover!(
        matches!(said, Concluded::DeclinedUnderTheMutant { .. }),
        "njutest-law-branch:declined-under-the-mutant"
    );
    kani::cover!(said == Concluded::Declined, "njutest-law-branch:declined");
    kani::cover!(said == Concluded::Survived, "njutest-law-branch:survived");
    kani::cover!(true, "njutest-law-reached");
}
