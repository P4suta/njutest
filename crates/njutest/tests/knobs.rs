// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a knob's records earn: a defect where a knob broke a target, a gap where it moved a target's reach, and a limitation where it was not put or compared nothing.

use std::collections::BTreeSet;

use njutest::report::drift::{Moved, Unmeasured};
use njutest::report::knobs::{
    Knob, KnobRecord, NotPut, Reach, Standing, Unsettled, found, limited,
};
use njutest::report::{Decided, FindingKind, MutantRecord, Outcome};
use njutest::testkit::reports::{routed, row};

const ZONED: &str = "pkg/test/zoned";
const OTHER: &str = "pkg/test/other";

fn record(target: &str, knob: Knob, standing: Standing) -> KnobRecord {
    KnobRecord {
        target: target.to_owned(),
        knob,
        standing,
    }
}

/// A survivor whose route put it to `reaching` and nothing else, and one nothing reached.
fn rows(reaching: &[&str]) -> Vec<MutantRecord> {
    let mut routed_survivor = row(
        0,
        ("src/lib.rs", "zone", 3),
        ("gt-to-ge", ">", ">="),
        Decided::Survived,
    );
    routed_survivor.routing = Some(routed(
        reaching,
        &[],
        &reaching
            .iter()
            .map(|one| (*one, Outcome::Survived))
            .collect::<Vec<_>>(),
    ));
    vec![
        routed_survivor,
        row(
            1,
            ("src/lib.rs", "zone", 4),
            ("gt-to-ge", ">", ">="),
            Decided::Unreached,
        ),
    ]
}

fn moved() -> Box<Reach> {
    let gained = Moved {
        gained: BTreeSet::from([0]),
        lost: BTreeSet::new(),
    };
    let nothing = || Moved {
        gained: BTreeSet::new(),
        lost: BTreeSet::new(),
    };
    Box::new(Reach {
        reached: gained,
        bodies: nothing(),
        infected: nothing(),
    })
}

#[test]
fn a_knob_that_broke_a_target_is_a_defect_naming_the_knob_and_the_tests_that_failed() {
    let knobs = [record(
        ZONED,
        Knob::Timezone,
        Standing::Broke {
            failed: vec!["the_zone_is_not_lord_howe".to_owned()],
        },
    )];
    let findings = found(&knobs, &rows(&[OTHER]));
    let [finding] = findings.as_slice() else {
        panic!("one finding for the one target the knob broke: {findings:?}");
    };
    assert_eq!(finding.kind, FindingKind::EnvironmentDependent);
    assert!(
        finding.kind.is_defect(),
        "a suite whose answer depends on the zone is wrong"
    );
    assert_eq!(finding.subject, ZONED);
    assert!(
        finding.detail.contains("TZ=Australia/Lord_Howe")
            && finding.detail.contains("the_zone_is_not_lord_howe"),
        "the finding says what was set and which tests failed, which is what a reader reruns: {}",
        finding.detail
    );
}

#[test]
fn a_knob_that_moved_only_the_reach_is_a_gap_counting_what_rests_on_the_target() {
    let knobs = [record(
        ZONED,
        Knob::Threads,
        Standing::Moved { reach: moved() },
    )];
    let findings = found(&knobs, &rows(&[OTHER]));
    let [finding] = findings.as_slice() else {
        panic!("one finding for the one target whose reach moved: {findings:?}");
    };
    assert_eq!(finding.kind, FindingKind::EnvironmentDependentReach);
    assert!(
        !finding.kind.is_defect(),
        "the suite passed; what is wrong is every proof read off its reach"
    );
    assert!(
        finding.detail.contains("--test-threads=1")
            && finding
                .detail
                .contains("1 mutation a proof removed its run of")
            && finding.detail.contains("1 mutation no test reached"),
        "the finding counts the dispositions resting on the target's baseline by the rule drift \
         counts with: {}",
        finding.detail
    );
    let routed_to_it = found(&knobs, &rows(&[ZONED]));
    assert!(
        routed_to_it.iter().all(|one| one
            .detail
            .contains("0 mutations a proof removed its run of")),
        "a survivor the route did put to the target rests on no removal: {routed_to_it:?}"
    );
}

#[test]
fn a_knob_that_held_finds_nothing() {
    let knobs = [
        record(ZONED, Knob::Columns, Standing::Stable),
        record(OTHER, Knob::Columns, Standing::Stable),
    ];
    assert!(found(&knobs, &rows(&[OTHER])).is_empty());
    assert!(limited(&knobs).is_empty());
}

#[test]
fn a_knob_asked_for_and_not_put_or_compared_is_a_limitation_that_says_why() {
    let knobs = [
        record(
            ZONED,
            Knob::Locale,
            Standing::NotPut {
                why: NotPut::LocaleMissing,
            },
        ),
        record(
            OTHER,
            Knob::Umask,
            Standing::Uncompared {
                why: Unmeasured::OtherTests,
            },
        ),
        record(
            OTHER,
            Knob::Threads,
            Standing::Unsettled {
                why: Unsettled::Waited,
            },
        ),
    ];
    assert!(
        found(&knobs, &rows(&[OTHER])).is_empty(),
        "a knob not put or not compared found nothing, which is not the same as finding that \
         nothing depends on it"
    );
    let limitations = limited(&knobs);
    let named: Vec<(&str, &str)> = limitations
        .iter()
        .map(|one| (one.name.as_str(), one.detail.as_str()))
        .collect();
    assert!(
        named.iter().any(|(name, detail)| *name == "knob-not-put"
            && detail.contains("locale")
            && detail.contains(ZONED)
            && detail.contains("not installed")),
        "{named:?}"
    );
    assert!(
        named
            .iter()
            .any(|(name, detail)| *name == "knob-not-compared"
                && detail.contains("umask")
                && detail.contains("threads")
                && detail.contains(OTHER)),
        "{named:?}"
    );
}
