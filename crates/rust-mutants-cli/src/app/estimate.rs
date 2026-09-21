// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run would cost, counted in work first and guessed at in time last.

use std::collections::BTreeSet;

use rust_mutants::count::{Count, Mutants, Pairs, Targets, Tests, Unit};
use rust_mutants::run;
use rust_mutants::session::{self, Route, Session};

/// What a run would cost, from what preparing established and before a mutant is executed.
///
/// # Errors
/// Returns the first count or duration that cannot be represented exactly.
pub fn estimate(session: &Session, filter: &run::Filter) -> Result<String, crate::error::CliError> {
    let targets = count(session.targets().len(), "the dry-run target count")?;
    let held = held_tests(session)?;
    let mut counted = Estimated::default();
    let mut text = String::new();
    let rejected = rejected(session);
    for mutant in session.catalog().mutants() {
        if rejected.contains(&mutant.index) {
            continue;
        }
        add(&mut counted.cataloged, 1, "the cataloged-mutant count")?;
        add(&mut counted.tests_whole, held, "the all-tests work count")?;
        if !session.was_validated(mutant.index) {
            add(&mut counted.unselected, 1, "the unselected-mutant count")?;
            continue;
        }
        debug_assert!(
            session.accepted().binary_search(&mutant.index).is_ok(),
            "a validated candidate is either accepted or rejected"
        );
        let at = session.position(mutant);
        let line = at.map_or(0, |one| one.line);
        if !filter.is_empty() && !filter.selects(mutant, line) {
            add(&mut counted.unselected, 1, "the unselected-mutant count")?;
            continue;
        }
        let route = session.route(mutant);
        let reaching = count(route.reaching().len(), "a route's reaching-target count")?;
        let discharged = count(
            route.discharged().len(),
            "a route's discharged-target count",
        )?;
        add(
            &mut counted.discharged,
            discharged,
            "the discharged-pair count",
        )?;
        let not_reaching =
            targets
                .checked_sub(reaching)
                .ok_or(crate::error::CliError::ProjectionOverflow {
                    projection: "dry-run",
                    field: "a route with more reaching targets than the session",
                })?;
        let unreached = not_reaching.checked_sub(discharged).ok_or(
            crate::error::CliError::ProjectionOverflow {
                projection: "dry-run",
                field: "a route with more accounted targets than the session",
            },
        )?;
        add(
            &mut counted.unreached,
            unreached,
            "the unreached-pair count",
        )?;
        if reaching == 0 {
            add(
                &mut counted.nothing_to_ask,
                1,
                "the nothing-to-ask mutant count",
            )?;
        } else {
            add(&mut counted.selected, 1, "the selected-mutant count")?;
            add(&mut counted.pairs, reaching, "the selected-pair count")?;
            counted.duration = counted
                .duration
                .checked_add(priced(session, &route)?)
                .ok_or(crate::error::CliError::ProjectionOverflow {
                    projection: "dry-run",
                    field: "the projected execution duration",
                })?;
            add_count(
                &mut counted.tests,
                route.started(|target| session.tests_of(target))?,
                "the selected-test count",
            )?;
        }
        text.push_str(&route_line(mutant, line, &route, reaching));
    }
    text.push_str(&counted.said(Count::new(targets))?);
    Ok(text)
}

fn route_line(
    mutant: &rust_mutants::catalog::Mutant,
    line: u32,
    route: &Route,
    reaching: u64,
) -> String {
    format!(
        "#{:<5} {}  {:<22} {}:{}  {}  {} targets\n",
        mutant.index,
        mutant.display_id,
        mutant.candidate.rule.name,
        mutant.candidate.path,
        line,
        route.granularity().name(),
        reaching
    )
}

fn rejected(session: &Session) -> BTreeSet<u32> {
    session
        .rejections()
        .iter()
        .map(|rejection| rejection.index)
        .collect()
}

fn held_tests(session: &Session) -> Result<u64, crate::error::CliError> {
    session.targets().iter().try_fold(0u64, |total, target| {
        total
            .checked_add(u64::from(session.tests_of(&target.id)))
            .ok_or(crate::error::CliError::ProjectionOverflow {
                projection: "dry-run",
                field: "the total tests in all targets",
            })
    })
}

fn count(value: usize, field: &'static str) -> Result<u64, crate::error::CliError> {
    u64::try_from(value).map_err(|_overflow| overflow(field))
}

fn add<U: Unit>(
    slot: &mut Count<U>,
    value: u64,
    field: &'static str,
) -> Result<(), crate::error::CliError> {
    add_count(slot, Count::new(value), field)
}

fn add_count<U: Unit>(
    slot: &mut Count<U>,
    value: Count<U>,
    field: &'static str,
) -> Result<(), crate::error::CliError> {
    *slot = slot.checked_add(value).ok_or_else(|| overflow(field))?;
    Ok(())
}

const fn overflow(field: &'static str) -> crate::error::CliError {
    crate::error::CliError::ProjectionOverflow {
        projection: "dry-run",
        field,
    }
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
    ///
    /// # Errors
    /// Returns when an arithmetic invariant or the rounded duration cannot be represented exactly.
    pub fn said(&self, targets: Count<Targets>) -> Result<String, crate::error::CliError> {
        let whole = self
            .cataloged
            .checked_against(targets)
            .ok_or_else(|| overflow("the all-mutants pair count"))?;
        let removed = whole
            .checked_sub(self.pairs)
            .ok_or_else(|| overflow("more selected pairs than all possible pairs"))?;
        let share = match removed.share_of(whole) {
            Some(share) => share * 100.0,
            None => 0.0,
        };
        let tests_removed = self
            .tests_whole
            .checked_sub(self.tests)
            .ok_or_else(|| overflow("more selected tests than all possible tests"))?;
        let tests_share = match tests_removed.share_of(self.tests_whole) {
            Some(share) => share * 100.0,
            None => 0.0,
        };
        let seconds = self
            .duration
            .as_secs()
            .checked_add(u64::from(self.duration.subsec_nanos() > 0))
            .ok_or(crate::error::CliError::ProjectionOverflow {
                projection: "dry-run",
                field: "the rounded projected duration",
            })?;
        Ok(format!(
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
        ))
    }
}

/// What one route would take on this machine, priced from what this session timed.
fn priced(
    session: &Session,
    route: &Route,
) -> Result<std::time::Duration, session::RouteAccountingError> {
    route.costing(|target| {
        session::Timing::new(
            session
                .baseline(target)
                .unwrap_or_else(|| session.slowest_baseline()),
            session.tests_of(target),
        )
    })
}
