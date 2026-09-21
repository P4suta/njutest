// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The review loop: one place at a time, with the answers supplied rather than typed.

use njutest_cli::presentation::review::{AboutAGap, AboutTheUnsettled, Answers, Reason, review};
use njutest_cli::presentation::{
    Blindness, Headline, Place, Spot, Standing, Terminal, Told, Unsettled,
};
use njutest_cli::report::Verdict;

fn spot(line: u32, standing: Standing, locator: &str) -> Spot {
    Spot {
        line,
        column: 5,
        was: ">".to_owned(),
        now: ">=".to_owned(),
        said: standing.word().to_owned(),
        standing,
        blind_in: Vec::new(),
        locator: locator.to_owned(),
    }
}

/// Where a run of a project that has said nothing about where it writes was kept.
fn kept(run: &str) -> String {
    format!(
        "{}/runs/{run}",
        njutest_cli::config::Config::default()
            .reports
            .directory
            .as_path()
            .display()
    )
}

fn told(spots: Vec<Spot>) -> Told {
    Told {
        headline: Headline {
            verdict: Verdict::Insufficient,
            cataloged: 3,
            killed: 1,
            survived: 1,
            unreached: 1,
            step_limit_reached: 0,
            waited: 0,
            duration_ms: 1000,
            kept: kept("one"),
        },
        places: vec![Place {
            item: "sign".to_owned(),
            path: "src/lib.rs".to_owned(),
            excerpt: vec![(8, "    if n > 0 {".to_owned())],
            instead: None,
            spots,
        }],
        diagnostics: Vec::new(),
        limitations: Vec::new(),
    }
}

/// A reviewer a test wrote down: what it says, and what it was shown.
struct Scripted {
    gaps: Vec<AboutAGap>,
    seen_gaps: usize,
    seen_unsettled: usize,
}

impl Scripted {
    const fn saying(gaps: Vec<AboutAGap>) -> Self {
        Self {
            gaps,
            seen_gaps: 0,
            seen_unsettled: 0,
        }
    }
}

impl Answers for Scripted {
    fn about_a_gap(&mut self, _spot: &Spot, _blindness: Blindness, _drawn: &str) -> AboutAGap {
        let said = self
            .gaps
            .get(self.seen_gaps)
            .cloned()
            .unwrap_or(AboutAGap::Leave);
        self.seen_gaps = self.seen_gaps.saturating_add(1);
        said
    }

    fn about_the_unsettled(
        &mut self,
        _spot: &Spot,
        _unsettled: Unsettled,
        _drawn: &str,
    ) -> AboutTheUnsettled {
        self.seen_unsettled = self.seen_unsettled.saturating_add(1);
        AboutTheUnsettled::Leave
    }
}

#[test]
fn nothing_the_run_failed_to_establish_can_be_accepted() {
    let told = told(vec![
        spot(
            8,
            Standing::Blind(Blindness::Ran),
            "src/lib.rs:sign:gt-to-ge@8",
        ),
        spot(
            9,
            Standing::Unsettled(Unsettled::Waited),
            "src/lib.rs:sign:lt-to-le@9",
        ),
    ]);
    let mut asking = Scripted::saying(vec![AboutAGap::Accept(
        Reason::of("covered elsewhere").expect("a reason"),
    )]);
    let reviewed = review(&told, Terminal::plain(80), &mut asking);
    assert_eq!(
        asking.seen_unsettled, 1,
        "a reviewer is still shown what the run could not settle — passing over it \
         silently is the run claiming it looked"
    );
    assert_eq!(
        reviewed.accepted.len(),
        1,
        "and the one thing accepted is the one thing the run established. An acceptance \
         says a reviewer looked at what a run found and decided it may stand; there is \
         nothing to have looked at where nothing was found, and `AboutTheUnsettled` has \
         no arm that could say otherwise: {reviewed:?}"
    );
    assert_eq!(
        reviewed
            .accepted
            .first()
            .map(|(named, _why)| named.as_str()),
        Some("src/lib.rs:sign:gt-to-ge@8")
    );
}

#[test]
fn a_reason_nobody_wrote_is_not_a_reason() {
    assert!(Reason::of("").is_none());
    assert!(Reason::of("   ").is_none(), "nor is one made of spaces");
    assert!(Reason::of("covered by the integration suite").is_some());
}

#[test]
fn stopping_keeps_what_was_decided_and_says_where_it_stopped() {
    let told = told(vec![
        spot(8, Standing::Blind(Blindness::Ran), "one"),
        spot(9, Standing::Blind(Blindness::Ran), "two"),
        spot(10, Standing::Blind(Blindness::Ran), "three"),
    ]);
    let mut asking = Scripted::saying(vec![
        AboutAGap::Accept(Reason::of("first").expect("a reason")),
        AboutAGap::Stop,
    ]);
    let reviewed = review(&told, Terminal::plain(80), &mut asking);
    assert_eq!(asking.seen_gaps, 2, "it stops at the one that said stop");
    assert_eq!(
        reviewed.accepted.len(),
        1,
        "what a reviewer decided before they stopped is what they decided, and throwing \
         it away would make leaving the loop cost them the work: {reviewed:?}"
    );
    assert_eq!(
        reviewed.stopped_at.as_deref(),
        Some("two"),
        "and it says where, so the next review starts there rather than at the top: \
         {reviewed:?}"
    );
}
