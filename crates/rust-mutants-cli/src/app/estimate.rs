// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run would cost, counted in work first and guessed at in time last.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use rust_mutants::count::{Count, Mutants, Pairs, Targets, Tests};
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
        counted.cataloged = counted.cataloged.and(Count::new(1));
        counted.tests_whole = counted.tests_whole.and(Count::new(held));
        if !session.was_validated(mutant.index) {
            counted.unselected = counted.unselected.and(Count::new(1));
            continue;
        }
        debug_assert!(
            session.accepted().binary_search(&mutant.index).is_ok(),
            "a validated candidate is either accepted or rejected"
        );
        let at = session.position(mutant);
        let line = at.map_or(0, |one| one.line);
        if !filter.is_empty() && !filter.selects(mutant, line) {
            counted.unselected = counted.unselected.and(Count::new(1));
            continue;
        }
        let route = session.route(mutant);
        let reaching = u64::try_from(route.reaching().len()).unwrap_or(u64::MAX);
        let discharged = u64::try_from(route.discharged().len()).unwrap_or(u64::MAX);
        counted.discharged = counted.discharged.and(Count::new(discharged));
        counted.unreached = counted.unreached.and(Count::new(
            targets.saturating_sub(reaching).saturating_sub(discharged),
        ));
        if reaching == 0 {
            counted.nothing_to_ask = counted.nothing_to_ask.and(Count::new(1));
        } else {
            counted.selected = counted.selected.and(Count::new(1));
            counted.pairs = counted.pairs.and(Count::new(reaching));
            counted.duration = counted.duration.saturating_add(priced(session, &route));
            counted.tests = counted.tests.and(Count::new(
                u64::try_from(
                    route.started(|target| usize::try_from(session.tests_of(target)).unwrap_or(1)),
                )
                .unwrap_or(u64::MAX),
            ));
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
    text.push_str(&counted.said(Count::new(targets)));
    text
}

/// What a dry run counted, in pairs of one mutant and one target.
#[derive(Debug, Clone, Copy, Default)]
pub struct Estimated {
    /// Every mutant the catalog holds.
    pub cataloged: Count<Mutants>,
    /// The mutants a filter left out.
    pub unselected: Count<Mutants>,
    /// The mutants with at least one target to ask.
    pub selected: Count<Mutants>,
    /// The mutants no target would be asked about.
    pub nothing_to_ask: Count<Mutants>,
    /// The pairs a run would start at most, before one target answers for the rest.
    pub pairs: Count<Pairs>,
    /// The pairs the measurement removed.
    pub unreached: Count<Pairs>,
    /// The pairs a proof removed.
    pub discharged: Count<Pairs>,
    /// The tests a run would start, which is what the pairs are asked for.
    pub tests: Count<Tests>,
    /// The tests a run that asked every test of every target about every mutant would start.
    pub tests_whole: Count<Tests>,
    /// What those tests would take on this machine, target by target.
    pub duration: std::time::Duration,
}

impl Estimated {
    /// The estimate, counted in work first and guessed at in time last.
    #[must_use]
    pub fn said(&self, targets: Count<Targets>) -> String {
        let whole = self.cataloged.against(targets);
        let removed = whole.less(self.pairs);
        let share = removed.share_of(whole).unwrap_or_default() * 100.0;
        let tests_share = self
            .tests_whole
            .less(self.tests)
            .share_of(self.tests_whole)
            .unwrap_or_default()
            * 100.0;
        let seconds = self
            .duration
            .as_secs()
            .saturating_add(u64::from(self.duration.subsec_nanos() > 0));
        format!(
            "\nWOULD START  {} of {} ({} against {}); {share:.1}% removed\n\
             WHICH RUN    {} of {}; {tests_share:.1}% removed\n\
             REMOVED BY   unreached={} discharged={} of the {} the route removed\n\
             NEVER ASKED  unselected={} nothing-to-ask={} of the {} the catalog holds; a \
             mutant nobody asks about takes its targets' pairs with it\n\
             AT MOST      {} execute; one target that answers ends the rest\n\
             ROUGHLY      {}:{:02}:{:02} on this machine, being each target's own baseline \
             scaled by the tests its route names, which is a guess about the machine rather \
             than about the work\n",
            self.pairs.get(),
            whole.said(),
            self.cataloged.said(),
            targets.said(),
            self.tests.get(),
            self.tests_whole.said(),
            self.unreached.get(),
            self.discharged.get(),
            removed.said(),
            self.unselected.get(),
            self.nothing_to_ask.get(),
            self.cataloged.said(),
            self.selected.said(),
            seconds / 3600,
            seconds % 3600 / 60,
            seconds % 60,
        )
    }
}

/// What one route would take on this machine, priced from what this session timed.
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
