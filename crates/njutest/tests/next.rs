// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What `njutest next` offers: one behaviour at a time, cheapest first, and only a test the run checked.

use njutest::next::gaps;
use njutest::presentation::{Blindness, Headline, Place, Spot, Standing, Told, Unsettled};
use njutest::report::{CandidateRecord, Verdict};

fn spot(line: u32, standing: Standing, mutant: &str) -> Spot {
    Spot {
        line,
        column: 5,
        was: ">".to_owned(),
        now: ">=".to_owned(),
        said: standing.word().to_owned(),
        standing,
        blind_in: Vec::new(),
        locator: format!("src/lib.rs:item:gt-to-ge@{line}"),
        mutant: mutant.to_owned(),
    }
}

fn place(path: &str, item: &str, spots: Vec<Spot>) -> Place {
    Place {
        item: item.to_owned(),
        path: path.to_owned(),
        excerpt: Vec::new(),
        instead: None,
        spots,
    }
}

fn told(places: Vec<Place>) -> Told {
    Told {
        headline: Headline {
            verdict: Verdict::Insufficient,
            cataloged: 9,
            killed: 0,
            survived: 9,
            unreached: 0,
            step_limit_reached: 0,
            waited: 0,
            duration_ms: 1,
            kept: "runs/one".to_owned(),
        },
        places,
        diagnostics: Vec::new(),
        limitations: Vec::new(),
    }
}

fn candidate(mutant: &str, digest: &str, accepted: bool) -> CandidateRecord {
    CandidateRecord {
        finding: mutant.to_owned(),
        mutant: mutant.to_owned(),
        kind: "patch".to_owned(),
        path: "tests/pins.rs".to_owned(),
        digest: digest.to_owned(),
        preimage: None,
        stability_runs: 3,
        kill_runs: 2,
        accepted,
        why: None,
    }
}

const RAN: Standing = Standing::Blind(Blindness::Ran);

#[test]
fn one_behaviour_is_the_mutations_of_one_item_no_test_noticed() {
    let shown = told(vec![place(
        "src/lib.rs",
        "sign",
        vec![
            spot(8, RAN, "aaaa"),
            spot(9, Standing::Blind(Blindness::Never), "bbbb"),
            spot(10, Standing::Unsettled(Unsettled::Waited), "cccc"),
        ],
    )]);
    let found = gaps(&shown, &[]);
    let [gap] = found.as_slice() else {
        panic!("one item is one behaviour to pin: {found:?}");
    };
    assert_eq!(
        gap.mutants,
        ["aaaa", "bbbb"],
        "a mutation the run established nothing about is a gap in the run, and no test closes it"
    );
    assert_eq!(
        gap.offer, None,
        "nothing was offered, so nothing is claimed"
    );
}

#[test]
fn a_gap_a_checked_test_closes_comes_before_a_larger_one_nothing_was_offered_for() {
    let shown = told(vec![
        place(
            "src/big.rs",
            "wide",
            vec![spot(1, RAN, "w1"), spot(2, RAN, "w2"), spot(3, RAN, "w3")],
        ),
        place("src/small.rs", "narrow", vec![spot(1, RAN, "n1")]),
    ]);
    let offered = [candidate("n1", "d1", true)];
    let found = gaps(&shown, &offered);
    let order: Vec<&str> = found.iter().map(|gap| gap.place.item.as_str()).collect();
    assert_eq!(
        order,
        ["narrow", "wide"],
        "the cheapest thing to do is the one whose test is already written and checked"
    );
    let unoffered: Vec<&str> = found
        .iter()
        .filter(|gap| gap.offer.is_none())
        .map(|gap| gap.place.item.as_str())
        .collect();
    assert_eq!(unoffered, ["wide"]);
}

#[test]
fn among_gaps_nothing_was_offered_for_the_one_with_more_in_it_comes_first() {
    let shown = told(vec![
        place("src/a.rs", "one", vec![spot(1, RAN, "a1")]),
        place(
            "src/b.rs",
            "two",
            vec![spot(1, RAN, "b1"), spot(2, RAN, "b2")],
        ),
    ]);
    let order: Vec<String> = gaps(&shown, &[])
        .iter()
        .map(|gap| gap.place.item.clone())
        .collect();
    assert_eq!(order, ["two", "one"]);
}

#[test]
fn the_offer_is_the_content_that_closes_the_most_and_claims_only_what_was_checked() {
    let shown = told(vec![place(
        "src/lib.rs",
        "sign",
        vec![spot(1, RAN, "m1"), spot(2, RAN, "m2"), spot(3, RAN, "m3")],
    )]);
    let offered = [
        candidate("m1", "narrow", true),
        candidate("m1", "wide", true),
        candidate("m2", "wide", true),
        candidate("m3", "wide", false),
        candidate("elsewhere", "wide", true),
    ];
    let found = gaps(&shown, &offered);
    let [gap] = found.as_slice() else {
        panic!("{found:?}");
    };
    let Some(offer) = &gap.offer else {
        panic!("two candidates held up for this gap: {gap:?}");
    };
    assert_eq!(
        offer.digest(),
        "wide",
        "the content that closes more is offered"
    );
    let closes: Vec<&str> = offer.closes().map(|one| one.mutant.as_str()).collect();
    assert_eq!(
        closes,
        ["m1", "m2"],
        "a record that did not hold up is no evidence, and one about another place is not about this one"
    );
    assert_eq!(offer.held(), (3, 2));
}

#[test]
fn a_candidate_that_did_not_hold_up_is_never_an_offer() {
    let shown = told(vec![place("src/lib.rs", "sign", vec![spot(1, RAN, "m1")])]);
    let refused = [candidate("m1", "d", false)];
    let found = gaps(&shown, &refused);
    assert!(
        found.iter().all(|gap| gap.offer.is_none()),
        "a test that failed its checks is not one to hand anybody: {found:?}"
    );
}

/// Somebody deciding, as a test wrote them down: what they say to each offer and each gap, and what they were shown.
struct Scripted {
    offers: Vec<njutest::next::OnOffer>,
    gaps: Vec<njutest::next::OnGap>,
    shown: Vec<String>,
}

impl njutest::next::Deciding for Scripted {
    fn about_an_offer(&mut self, said: &str) -> njutest::next::OnOffer {
        self.shown.push(said.to_owned());
        if self.offers.is_empty() {
            njutest::next::OnOffer::Stop
        } else {
            self.offers.remove(0)
        }
    }

    fn about_a_gap(&mut self, said: &str) -> njutest::next::OnGap {
        self.shown.push(said.to_owned());
        if self.gaps.is_empty() {
            njutest::next::OnGap::Stop
        } else {
            self.gaps.remove(0)
        }
    }
}

fn two_places() -> Told {
    told(vec![
        place(
            "src/lib.rs",
            "sign",
            vec![spot(8, RAN, "m1"), spot(9, RAN, "m2"), spot(10, RAN, "m3")],
        ),
        place("src/other.rs", "other", vec![spot(3, RAN, "o1")]),
    ])
}

#[test]
fn an_offer_says_what_it_closes_what_it_leaves_and_how_it_was_checked() {
    let shown = two_places();
    let offered = [
        candidate("m1", "d".repeat(64).as_str(), true),
        candidate("m2", "d".repeat(64).as_str(), true),
    ];
    let found = gaps(&shown, &offered);
    let Some(first) = found.first() else {
        panic!("two places are two gaps");
    };
    let said = njutest::next::said(first);
    assert_eq!(
        said,
        format!(
            "the cheapest thing you can do closes 2 of the 3 gaps in `sign` (src/lib.rs).\n  \
             a checked test that does it: tests/pins.rs ({})\n  \
             it passed on this tree 3 times, and failed under each of those 2 mutations 2 times.\n  \
             1 other mutation of `sign` stays open after it.",
            "d".repeat(12)
        )
    );
}

#[test]
fn a_gap_nothing_was_offered_for_says_so_and_how_to_be_offered_one() {
    let shown = two_places();
    let found = gaps(&shown, &[]);
    assert_eq!(
        njutest::next::said(found.first().expect("two places are two gaps")),
        "3 mutations of `sign` (src/lib.rs) no test noticed, at lines 8, 9, 10.\n  \
         no checked test was offered for them; with `[generation]` configured, `njutest verify` \
         asks for one."
    );
}

#[test]
fn only_what_was_taken_is_written_and_stopping_leaves_the_rest_open() {
    let mut shown = two_places();
    shown
        .places
        .push(place("src/third.rs", "third", vec![spot(5, RAN, "t1")]));
    let offered = [candidate("m1", "d1", true)];
    let found = gaps(&shown, &offered);
    let mut deciding = Scripted {
        offers: vec![njutest::next::OnOffer::Take],
        gaps: vec![njutest::next::OnGap::Stop],
        shown: Vec::new(),
    };
    let walked = njutest::next::walk(&found, &mut deciding);
    let taken: Vec<&str> = walked
        .taken
        .iter()
        .map(njutest::next::Offer::digest)
        .collect();
    assert_eq!(
        taken,
        ["d1"],
        "what was taken is what is written, and nothing else"
    );
    let written = [njutest::next::Became::Written];
    assert_eq!(
        njutest::next::open(&found, &walked.taken, &written),
        4,
        "the two mutations the offer does not close, the one stopped at, and the one never reached \
         are still open"
    );
    assert!(walked.stopped);
    assert_eq!(
        deciding.shown.len(),
        2,
        "one thing at a time, and nothing after a stop"
    );
}

#[test]
fn passing_on_every_offer_writes_nothing() {
    let shown = two_places();
    let offered = [candidate("m1", "d1", true), candidate("o1", "d2", true)];
    let found = gaps(&shown, &offered);
    let mut deciding = Scripted {
        offers: vec![njutest::next::OnOffer::Next, njutest::next::OnOffer::Next],
        gaps: Vec::new(),
        shown: Vec::new(),
    };
    let walked = njutest::next::walk(&found, &mut deciding);
    assert!(walked.taken.is_empty());
    assert_eq!(
        njutest::next::open(&found, &walked.taken, &[]),
        4,
        "every mutation of every gap is still open"
    );
    assert!(!walked.stopped, "going through everything is not stopping");
}

#[test]
fn a_taken_test_that_did_not_hold_up_again_closes_nothing() {
    let shown = two_places();
    let offered = [candidate("m1", "d1", true)];
    let found = gaps(&shown, &offered);
    let mut deciding = Scripted {
        offers: vec![njutest::next::OnOffer::Take],
        gaps: vec![njutest::next::OnGap::Next],
        shown: Vec::new(),
    };
    let walked = njutest::next::walk(&found, &mut deciding);
    let all = njutest::next::open(&found, &[], &[]);
    assert_eq!(
        njutest::next::open(&found, &walked.taken, &[njutest::next::Became::Refused]),
        all,
        "a take refused when it was checked again wrote nothing and closed nothing"
    );
    assert_eq!(
        njutest::next::open(&found, &walked.taken, &[njutest::next::Became::Already]),
        njutest::next::open(&found, &walked.taken, &[njutest::next::Became::Written]),
        "a test already on disk that holds up closes what a written one does"
    );
}
