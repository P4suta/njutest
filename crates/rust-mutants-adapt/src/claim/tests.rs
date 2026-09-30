// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{Named, Resolution};

fn named() -> Vec<String> {
    vec!["1a2b".to_owned(), "3c4d".to_owned()]
}

fn moved() -> Resolution {
    Resolution::named(Named {
        mutants: named(),
        moved: Some((12, 15)),
    })
}

#[test]
fn a_claim_comes_to_what_its_locator_answered() {
    assert_eq!(
        Resolution::named(Named {
            mutants: named(),
            moved: None
        }),
        Resolution::Names { mutants: named() },
        "a claim that names its mutations on its own line names them"
    );
    assert_eq!(
        moved(),
        Resolution::Moved {
            mutants: named(),
            from: 12,
            to: 15
        }
    );
    assert_eq!(
        Resolution::unnamed("no mutation is the one described".to_owned(), || true),
        Resolution::Uncompiled,
        "what a claim names in a file only another build reads is judged there"
    );
    assert_eq!(
        Resolution::unnamed("no mutation is the one described".to_owned(), || false),
        Resolution::Unmatched {
            why: "no mutation is the one described".to_owned()
        },
        "a claim that names nothing a build reads is unmatched, in the run's words"
    );
}

#[test]
fn only_a_claim_that_names_nothing_or_a_line_its_mutation_left_has_rotted() {
    for (resolution, rotted, uncompiled) in [
        (Resolution::Names { mutants: named() }, false, false),
        (moved(), true, false),
        (Resolution::Uncompiled, false, true),
        (
            Resolution::Unmatched {
                why: "nothing".to_owned(),
            },
            true,
            false,
        ),
    ] {
        assert_eq!(resolution.rotted(), rotted, "{resolution:?}");
        assert_eq!(resolution.uncompiled(), uncompiled, "{resolution:?}");
    }
}

#[test]
fn a_planted_resolution_that_forgives_a_moved_line_is_caught() {
    let planted = |resolution: &Resolution| match resolution {
        Resolution::Unmatched { .. } => true,
        Resolution::Names { .. } | Resolution::Moved { .. } | Resolution::Uncompiled => false,
    };
    assert_ne!(
        planted(&moved()),
        moved().rotted(),
        "the rule did not catch a claim whose line moved passed as sound"
    );
}
