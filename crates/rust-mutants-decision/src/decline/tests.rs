// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

extern crate std;

use std::vec;
use std::vec::Vec;

use super::{Concluded, Held, concluded, held};

type Decline = (&'static str, &'static str);

const SHARING: Decline = ("tests::a", "cannot share blocks");
const OTHER_WORDS: Decline = ("tests::a", "cannot share either");
const ADDING: Decline = ("tests::b", "cannot add");
const NETWORK: Decline = ("tests::c", "no network");

#[test]
fn only_a_decline_the_baseline_made_in_the_same_words_is_set_aside() {
    let baseline = [SHARING];
    assert_eq!(held(&[SHARING], &baseline), Held::SetAside);
    assert_eq!(held::<Decline>(&[], &baseline), Held::SetAside);
    assert_eq!(
        held(&[ADDING], &baseline),
        Held::Detected { by: &ADDING },
        "a test that measured in the baseline and declined under the mutation was changed by it"
    );
    assert_eq!(
        held(&[OTHER_WORDS], &baseline),
        Held::Detected { by: &OTHER_WORDS },
        "other words are another decline"
    );
    assert_eq!(
        held(&[SHARING, ADDING, NETWORK], &baseline),
        Held::Detected { by: &ADDING },
        "the first decline the baseline did not make is the one named"
    );
}

fn sequences() -> Vec<Vec<Decline>> {
    let every = [SHARING, OTHER_WORDS, ADDING, NETWORK];
    let mut found = vec![Vec::new()];
    for one in every {
        found.push(vec![one]);
        for other in every {
            if other != one {
                found.push(vec![one, other]);
            }
        }
    }
    found
}

fn by_the_rules<'a>(
    believed: Option<&'a [Decline]>,
    baseline: &[Decline],
    passed: usize,
) -> Concluded<'a, Decline> {
    let Some(declined) = believed else {
        return Concluded::Errored;
    };
    let changed: Vec<&Decline> = declined
        .iter()
        .filter(|one| baseline.iter().all(|made| made != *one))
        .collect();
    if let Some(first) = changed.first() {
        Concluded::DeclinedUnderTheMutant { by: first }
    } else if declined.len() == passed && passed > 0 {
        Concluded::Declined
    } else {
        Concluded::Survived
    }
}

#[test]
fn every_survival_comes_to_what_the_rules_say_whatever_its_notice_and_baseline() {
    let sequences = sequences();
    let mut disagreeing = Vec::new();
    for baseline in &sequences {
        for declined in &sequences {
            for believed in [None, Some(declined.as_slice())] {
                for passed in 0..=3 {
                    let said = concluded(believed, baseline, passed);
                    if said != by_the_rules(believed, baseline, passed) {
                        disagreeing.push((believed, baseline.clone(), passed, said));
                    }
                }
            }
        }
    }
    assert!(
        disagreeing.is_empty(),
        "{} survivals come to something the rules do not say, the first {:?}",
        disagreeing.len(),
        disagreeing.first()
    );
}

#[test]
fn a_process_every_test_of_which_declined_as_its_baseline_did_measured_nothing() {
    let baseline = [SHARING, ADDING];
    assert_eq!(
        concluded(Some(&[SHARING, ADDING]), &baseline, 2),
        Concluded::Declined
    );
    assert_eq!(
        concluded(Some(&[SHARING]), &baseline, 2),
        Concluded::Survived,
        "a test that passed without declining measured, so the survival stands"
    );
    assert_eq!(
        concluded::<Decline>(Some(&[]), &baseline, 0),
        Concluded::Survived,
        "a process that declined nothing is a survival, however few tests it ran"
    );
    assert_eq!(
        concluded::<Decline>(None, &baseline, 2),
        Concluded::Errored,
        "a notice nobody can believe leaves no survival to believe"
    );
}

#[test]
fn a_planted_conclusion_that_sets_aside_a_decline_in_other_words_is_caught_by_the_rules() {
    let planted = |believed: Option<&'static [Decline]>, baseline: &[Decline], passed: usize| {
        let Some(declined) = believed else {
            return Concluded::Errored;
        };
        let tests_only = |one: &Decline| baseline.iter().any(|made| made.0 == one.0);
        match declined.iter().find(|one| !tests_only(one)) {
            Some(by) => Concluded::DeclinedUnderTheMutant { by },
            None if !declined.is_empty() && declined.len() == passed => Concluded::Declined,
            None => Concluded::Survived,
        }
    };
    let declined: &'static [Decline] = &[OTHER_WORDS];
    assert_ne!(
        planted(Some(declined), &[SHARING], 1),
        by_the_rules(Some(declined), &[SHARING], 1),
        "the rules did not catch a decline in other words set aside as the baseline's"
    );
}
