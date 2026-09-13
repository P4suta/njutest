// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run would cost, counted in work first and guessed at in time last.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use rust_mutants::run;
use rust_mutants::session::{self, Route, Session};

/// What a run would cost, from what preparing established and before a mutant is executed.
#[must_use]
pub fn estimate(session: &Session, filter: &run::Filter) -> String {
    let targets = u64::try_from(session.targets().len()).unwrap_or(u64::MAX);
    let held: u64 = session
        .targets()
        .iter()
        .map(|target| u64::from(session.tests_of(&target.id)))
        .sum();
    let mut counted = Estimated::default();
    let mut text = String::new();
    let rejected: BTreeSet<u32> = session
        .rejections()
        .iter()
        .map(|rejection| rejection.index)
        .collect();
    for mutant in session.catalog().mutants() {
        if rejected.contains(&mutant.index) {
            continue;
        }
        counted.cataloged = counted.cataloged.saturating_add(1);
        counted.tests_whole = counted.tests_whole.saturating_add(held);
        if !session.was_validated(mutant.index) {
            counted.unselected = counted.unselected.saturating_add(1);
            continue;
        }
        debug_assert!(
            session.accepted().binary_search(&mutant.index).is_ok(),
            "a validated candidate is either accepted or rejected"
        );
        let at = session.position(mutant);
        let line = at.map_or(0, |one| one.line);
        if !filter.is_empty() && !filter.selects(mutant, line) {
            counted.unselected = counted.unselected.saturating_add(1);
            continue;
        }
        let route = session.route(mutant);
        let reaching = u64::try_from(route.reaching().len()).unwrap_or(u64::MAX);
        let discharged = u64::try_from(route.discharged().len()).unwrap_or(u64::MAX);
        counted.discharged = counted.discharged.saturating_add(discharged);
        counted.unreached = counted
            .unreached
            .saturating_add(targets.saturating_sub(reaching).saturating_sub(discharged));
        if reaching == 0 {
            counted.nothing_to_ask = counted.nothing_to_ask.saturating_add(1);
        } else {
            counted.selected = counted.selected.saturating_add(1);
            counted.pairs = counted.pairs.saturating_add(reaching);
            counted.duration = counted.duration.saturating_add(priced(session, &route));
            counted.tests = counted.tests.saturating_add(
                u64::try_from(
                    route.started(|target| usize::try_from(session.tests_of(target)).unwrap_or(1)),
                )
                .unwrap_or(u64::MAX),
            );
        }
        let written = writeln!(
            text,
            "#{:<5} {}  {:<22} {}:{}  {}  {} targets",
            mutant.index,
            mutant.display_id,
            mutant.candidate.rule.name,
            mutant.candidate.path,
            line,
            route.granularity(),
            reaching
        );
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    text.push_str(&counted.said(targets));
    text
}

/// What a dry run counted, in pairs of one mutant and one target.
#[derive(Debug, Clone, Copy, Default)]
pub struct Estimated {
    /// Every mutant the catalog holds.
    pub cataloged: u64,
    /// The mutants a filter left out.
    pub unselected: u64,
    /// The mutants with at least one target to ask.
    pub selected: u64,
    /// The mutants no target would be asked about.
    pub nothing_to_ask: u64,
    /// The pairs a run would start at most, before one target answers for the rest.
    pub pairs: u64,
    /// The pairs the measurement removed.
    pub unreached: u64,
    /// The pairs a proof removed.
    pub discharged: u64,
    /// The tests a run would start, which is what the pairs are asked for.
    pub tests: u64,
    /// The tests a run that asked every test of every target about every mutant would start.
    pub tests_whole: u64,
    /// What those tests would take on this machine, target by target.
    pub duration: std::time::Duration,
}

impl Estimated {
    /// The estimate, counted in work first and guessed at in time last.
    ///
    /// A count is the same on every machine; a duration is a guess about this
    /// one. The count is what a person decides by, so it comes first and the
    /// guess comes last, marked as one.
    #[must_use]
    pub fn said(&self, targets: u64) -> String {
        let whole = self.cataloged.saturating_mul(targets);
        let removed = whole.saturating_sub(self.pairs);
        let widened =
            |count: u64| u32::try_from(count).map_or_else(|_| f64::from(u32::MAX), f64::from);
        let share = if whole == 0 {
            0.0
        } else {
            widened(removed) / widened(whole) * 100.0
        };
        let seconds = self
            .duration
            .as_secs()
            .saturating_add(u64::from(self.duration.subsec_nanos() > 0));
        let tests_share = if self.tests_whole == 0 {
            0.0
        } else {
            widened(
                self.tests_whole
                    .saturating_sub(self.tests.min(self.tests_whole)),
            ) / widened(self.tests_whole)
                * 100.0
        };
        format!(
            "\nWOULD START  {} of {whole} pairs ({} mutants against {targets} targets); \
             {share:.1}% removed\n\
             WHICH RUN    {} of {} tests; {tests_share:.1}% removed\n\
             REMOVED BY   unreached={} discharged={} unselected={} nothing-to-ask={}\n\
             AT MOST      {} mutants execute; one target that answers ends the rest\n\
             ROUGHLY      {}:{:02}:{:02} on this machine, being each target's own baseline \
             scaled by the tests its route names, which is a guess about the machine rather \
             than about the work\n",
            self.pairs,
            self.cataloged,
            self.tests,
            self.tests_whole,
            self.unreached,
            self.discharged,
            self.unselected,
            self.nothing_to_ask,
            self.selected,
            seconds / 3600,
            seconds % 3600 / 60,
            seconds % 60,
        )
    }
}

/// What one route would take on this machine, priced from what this session timed.
///
/// A target this session never timed is priced at the slowest one it did,
/// which is the guess that errs toward too long.
fn priced(session: &Session, route: &Route) -> std::time::Duration {
    route.costing(|target| {
        session::Timing::new(
            session
                .baseline(target)
                .unwrap_or_else(|| session.slowest_baseline()),
            session.tests_of(target),
        )
    })
}
