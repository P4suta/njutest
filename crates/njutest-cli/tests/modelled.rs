// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a model checker establishes, and that a run which stopped watching cannot be read as a fact about the code.

use std::time::Duration;

use njutest_cli::modelled::{Cutoff, Modelled, Unaskable, Undecided};

#[test]
fn a_run_that_stopped_watching_says_nothing_about_the_code() {
    let cut_off = Modelled::Undecided(Undecided::Cutoff(Cutoff {
        waited: Duration::from_secs(60),
    }));
    assert!(
        !cut_off.about_the_code(),
        "the run is telling a reader about itself. There is no fact about the code \
         here, not even a negative one, and a survivor rendered beside a proof that \
         was too hard would read as one that resisted proof"
    );

    for asked in [
        Unaskable::TooDeep { bound: 5 },
        Unaskable::NotArbitrary {
            argument: "order: Order".to_owned(),
        },
    ] {
        assert!(
            Modelled::Undecided(Undecided::Unaskable(asked)).about_the_code(),
            "these two were asked about the code and the code is why there is no \
             answer, which is a different thing for a reader to be told"
        );
    }
}

#[test]
fn only_a_question_about_the_code_offers_the_reader_something_to_change() {
    let too_deep = Unaskable::TooDeep { bound: 5 };
    assert!(
        too_deep.change().contains('5'),
        "the bound a reader would raise is the one the run used: {}",
        too_deep.change()
    );
    assert!(
        too_deep.available(),
        "the proof is there and this run did not reach it"
    );

    let unaskable = Unaskable::NotArbitrary {
        argument: "order: Order".to_owned(),
    };
    assert!(
        unaskable.change().contains("order: Order"),
        "and the argument that cannot be made symbolic is named: {}",
        unaskable.change()
    );
    assert!(
        !unaskable.available(),
        "no setting reaches an answer for this one, which is why the two are not \
         one case with a number in it"
    );
}

#[test]
fn what_a_checker_said_is_one_of_three_and_says_which() {
    assert_eq!(Modelled::Proved.name(), "proved");
    assert_eq!(Modelled::Noticed.name(), "noticed");
    assert_eq!(
        Modelled::Undecided(Undecided::Cutoff(Cutoff {
            waited: Duration::from_secs(1),
        }))
        .name(),
        "undecided"
    );
    assert!(
        Modelled::Proved.about_the_code() && Modelled::Noticed.about_the_code(),
        "a proof and a refutation are both facts about the code"
    );
}
