// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every limitation that can reach a report says what a reader can do about it.

use mjutest_cli::assure::run::limitation_detail;

const GENERIC: &str = "stated by a phase of the run";

#[test]
fn every_limitation_the_engine_can_state_has_a_sentence_of_its_own() {
    let mut said: std::collections::BTreeMap<String, &str> = std::collections::BTreeMap::new();
    for name in rust_mutants::limitation::ALL {
        let detail = limitation_detail(name);
        assert_ne!(
            detail, GENERIC,
            "{name} reaches a report with nothing a reader can act on. A layer that \
             names itself and says nothing more is not one a person can audit, which \
             is what ADR 0004 decision 4 asks of every layer"
        );
        assert!(
            !detail.trim().is_empty(),
            "{name} reaches a report with an empty sentence, which is not the generic \
             one and is less than it: a reader is told the name of something that went \
             unmeasured and nothing whatever about it"
        );
        if let Some(other) = said.insert(detail.clone(), name) {
            panic!(
                "{name} and {other} say the same sentence, so a reader told either one \
                 learns which name it is and not which thing happened: {detail}"
            );
        }
    }
}

#[test]
fn a_limitation_that_names_its_target_is_still_looked_up_by_what_it_is() {
    let about = limitation_detail(&format!(
        "{}:core/lib/core",
        rust_mutants::limitation::BASELINE_NOT_PASSING
    ));
    assert_eq!(
        about,
        limitation_detail(rust_mutants::limitation::BASELINE_NOT_PASSING),
        "which target could not be measured is what a reader acts on, and the sentence \
         is about the limitation rather than about the target"
    );
    assert_ne!(about, GENERIC);
}

#[test]
fn a_name_from_a_later_engine_still_says_it_came_from_a_phase() {
    assert_eq!(
        limitation_detail("a limitation no release of this runner has ever seen"),
        GENERIC,
        "an unknown name is still reported rather than dropped: a limitation a reader \
         cannot look up is better than one they never hear about"
    );
}
