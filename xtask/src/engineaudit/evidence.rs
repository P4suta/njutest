// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report held to the files the run kept so that somebody else could re-decide it.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::layers::Closed;

use super::wire::{
    Guarded, GuardedItem, GuardedNarrowing, GuardedSeen, GuardedTarget, Measurement,
};
use super::{
    Audit, BRANCH_NEVER_TAKEN, CheckedEvidence, DISCHARGED, Decided, Granularity, KILLED, Layer,
    NEVER_INFECTED, NOT_RUN, Notes, Report, RouteDecision, Row, UNPROVEN_DISCHARGED, count, number,
    plural, string,
};

/// Every place a rule targets, against the decision the walk took about it.
pub(super) fn sites(evidence: &CheckedEvidence<'_>, audit: &mut Audit) -> Decided {
    let mut notes = Notes::on(audit, Layer::Sites);
    if !evidence.sites {
        notes.unaudited(
            "census",
            "the census of the walk's own decisions was not asked for, so the places it saw are \
             not counted"
                .to_owned(),
        );
        return notes.looked();
    }
    let Some(recorded) = &evidence.recorded else {
        notes.unaudited(
            "recording",
            "the run kept no recording, so the places the walk saw cannot be counted".to_owned(),
        );
        return notes.looked();
    };
    let mut discovery = Discovery::Absent;
    for event in &recorded.events {
        if audit_discovery(event, &mut notes) == Discovery::Present {
            discovery = Discovery::Present;
        }
    }
    if discovery == Discovery::Absent {
        notes.unaudited(
            "recording",
            "the recording names no file the walk went through".to_owned(),
        );
    }
    notes.looked()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Discovery {
    Absent,
    Present,
}

fn audit_discovery(event: &Value, notes: &mut Notes<'_>) -> Discovery {
    if string(event, "type").as_deref() != Some("discover-file") {
        return Discovery::Absent;
    }
    let Some(record) = event.get("discover") else {
        notes.violated(
            "discover-file",
            "a discover-file event carries no discover record".to_owned(),
        );
        return Discovery::Present;
    };
    let Some(path) = string(record, "path") else {
        notes.violated("discover-file", "the record names no path".to_owned());
        return Discovery::Present;
    };
    let Some(candidates) = number(record, "candidates") else {
        notes.violated(&path, "the record carries no candidate count".to_owned());
        return Discovery::Present;
    };
    let Some(tallies) = record.get("skips").and_then(Value::as_array) else {
        notes.violated(&path, "the record carries no skip array".to_owned());
        return Discovery::Present;
    };
    let Some(places) = record.get("sites").and_then(Value::as_array) else {
        notes.violated(&path, "the record carries no site array".to_owned());
        return Discovery::Present;
    };
    let Some(hidden) = skip_total(tallies, &path, notes) else {
        return Discovery::Present;
    };
    if candidates == 0 && places.is_empty() && whole_file_skip(tallies) {
        return Discovery::Present;
    }
    let decided = count(places.len());
    let Some(accounted) = candidates.checked_add(hidden) else {
        notes.violated(
            &path,
            "candidate plus skipped-place count exceeds the report's integer width".to_owned(),
        );
        return Discovery::Present;
    };
    if decided != accounted {
        notes.violated(
            &path,
            format!(
                "the walk of {path} took {decided} decisions and the file holds \
                 {candidates} candidates and {hidden} skipped places; a place with \
                 neither is one it passed over without saying so"
            ),
        );
    }
    Discovery::Present
}

fn whole_file_skip(tallies: &[Value]) -> bool {
    tallies.len() == 1
        && tallies
            .first()
            .and_then(|skip| string(skip, "reason"))
            .is_some_and(|reason| {
                matches!(
                    reason.as_str(),
                    "excluded"
                        | "test-only-file"
                        | "no-std-crate"
                        | "generated-outside-workspace"
                        | "forbidden-lints"
                )
            })
}

fn skip_total(tallies: &[Value], path: &str, notes: &mut Notes<'_>) -> Option<u64> {
    let mut hidden = 0u64;
    for skip in tallies {
        let Some(skipped) = number(skip, "count") else {
            notes.violated(path, "a skip record has no valid count".to_owned());
            return None;
        };
        if skipped == 0 {
            notes.violated(
                path,
                "a skip record names no skipped place; an empty decision is not evidence"
                    .to_owned(),
            );
            return None;
        }
        let Some(next_hidden) = hidden.checked_add(skipped) else {
            notes.violated(
                path,
                "the skip total exceeds the report's integer width".to_owned(),
            );
            return None;
        };
        hidden = next_hidden;
    }
    Some(hidden)
}

/// The parts of one catalog, against the whole they say they are, or, for a report `merge` wrote, each row against the part whose run it says decided it.
pub(super) fn merge(report: &Report, evidence: &CheckedEvidence<'_>, audit: &mut Audit) -> Decided {
    let mut notes = Notes::on(audit, Layer::Merge);
    if report.mutants.iter().any(|row| row.part_run_id.is_some()) {
        decided_in_parts(report, &evidence.shards, &mut notes);
        return notes.looked();
    }
    if evidence.shards.is_empty() {
        match merged_scope(report) {
            Closed::NothingOwed(why) => return notes.absent(why),
            Closed::Missing => {
                notes.unaudited(
                    "shards",
                    "no other part of this catalog was given, so whether the parts come to the \
                     whole cannot be re-derived"
                        .to_owned(),
                );
                return notes.looked();
            }
        }
    }
    let mut indices: BTreeSet<u64> = report.mutants.iter().map(|row| row.index).collect();
    let mut total = report.mutants.len();
    for (name, part) in &evidence.shards {
        for (what, mine, theirs) in [
            (
                "catalog_digest",
                &report.catalog_digest,
                &part.catalog_digest,
            ),
            (
                "workspace_digest",
                &report.workspace_digest,
                &part.workspace_digest,
            ),
            ("tool_version", &report.tool_version, &part.tool_version),
        ] {
            if mine != theirs {
                notes.violated(
                    name,
                    format!(
                        "the part's {what} is {theirs} and this run's is {mine}; parts of one \
                         catalog answer the same question"
                    ),
                );
            }
        }
        if part.selection != report.selection {
            notes.violated(
                name,
                "the part asked for something else; parts of one catalog were asked the same \
                 thing"
                    .to_owned(),
            );
        }
        let Some(next_total) = total.checked_add(part.mutants.len()) else {
            notes.violated(
                "shards",
                "the number of rows does not fit in this platform's address space".to_owned(),
            );
            return notes.looked();
        };
        total = next_total;
        count_each_row_once(name, part, &mut indices, &mut notes);
    }
    if indices.len() != total {
        notes.violated(
            "shards",
            format!(
                "the parts hold {total} rows over {} distinct indices; a part that repeats a \
                 mutant counts it twice",
                indices.len()
            ),
        );
    }
    notes.looked()
}

/// Every row of `part`, each held to belonging to one part only.
fn count_each_row_once(
    name: &str,
    part: &Report,
    indices: &mut BTreeSet<u64>,
    notes: &mut Notes<'_>,
) {
    for row in &part.mutants {
        if !indices.insert(row.index) {
            notes.violated(
                name,
                format!(
                    "index {} is in two parts; a mutant belongs to one part",
                    row.index
                ),
            );
        }
    }
}

/// The merge layer's subject, closed from the run's own scope: a run that measured the whole catalog itself, as its own `shard` field says, is one run's report and a merge is somebody else's question, while a run that measured one part is owed the other parts of its catalog.
const fn merged_scope(report: &Report) -> Closed {
    if report.shard.is_none() {
        return Closed::NothingOwed(
            "this report is one run's own whole catalog, and a merge is audited against its \
             parts",
        );
    }
    Closed::Missing
}

/// Every row of the merged `report`, against the row of the same index in the part its `part_run_id` names among `parts`: the same outcome, resting on the same executions, since a merge runs nothing and carries what its parts decided.
fn decided_in_parts(report: &Report, parts: &[(&str, Report)], notes: &mut Notes<'_>) {
    if parts.is_empty() {
        notes.unaudited(
            "shards",
            "this report merges parts and none of them was given, so whether each row is what \
             the run of its part decided cannot be re-derived"
                .to_owned(),
        );
        return;
    }
    for row in &report.mutants {
        let Some(run) = row.part_run_id.as_deref() else {
            notes.violated(
                row.label(),
                "the report merges parts and this row names no part run that decided it".to_owned(),
            );
            continue;
        };
        let Some((name, part)) = parts.iter().find(|(_, part)| part.run_id == run) else {
            notes.unaudited(
                row.label(),
                format!("the part {run} that decided it was not given"),
            );
            continue;
        };
        match part.mutants.iter().find(|one| one.index == row.index) {
            Some(one)
                if one.id == row.id
                    && one.outcome == row.outcome
                    && one.evidence == row.evidence
                    && one.part_run_id.is_none() => {}
            Some(_) => notes.violated(
                row.label(),
                format!(
                    "the part {name} its row names decided it otherwise; a merge carries what its \
                     parts decided and runs nothing of its own"
                ),
            ),
            None => notes.violated(
                row.label(),
                format!(
                    "the part {name} its row names holds no row of index {}; a row no part \
                     decided is one nobody ran",
                    row.index
                ),
            ),
        }
    }
}

/// Re-derives every discharge the run claimed from the evidence it kept, and says what the coverage measurement is owed.
fn measured_every_target(
    report: &Report,
    reached: Option<&Measurement>,
    touched: Option<&Guarded>,
    notes: &mut Notes<'_>,
) -> Closed {
    let scope = measured_scope(report, touched);
    if let Closed::NothingOwed(why) = scope {
        return Closed::NothingOwed(why);
    }
    let Some(reached) = reached else {
        notes.unaudited(
            "measurement",
            "the run kept no measurement, so what it was allowed to narrow by cannot be \
             re-derived"
                .to_owned(),
        );
        return Closed::Missing;
    };
    let named: BTreeSet<&str> = reached.targets.keys().map(String::as_str).collect();
    if named.is_empty() {
        notes.unaudited(
            "measurement",
            "the measurement names no target, so the run narrowed by nothing".to_owned(),
        );
        return Closed::Missing;
    }
    let excused = excused(&reached.limitations);
    for target in &report.targets {
        if target.contains("/doc/") {
            continue;
        }
        if !named.contains(target.as_str()) && !excused.contains(target) {
            notes.violated(
                "measurement",
                format!(
                    "the run built {target} and the measurement neither names it nor says it \
                     could not read it; a route that narrowed by this measurement narrowed by a \
                     target nobody looked at"
                ),
            );
        }
    }
    Closed::Missing
}

/// The proofs layer's coverage-measurement subject, closed from the run's own routing contract: a run whose guards measured routes by what they recorded, and so says the coverage measurement narrowed nothing, owes no measurement, and a catalog with no row narrows nothing whoever measured; anything else is owed the measurement its routes rested on.
fn measured_scope(report: &Report, touched: Option<&Guarded>) -> Closed {
    if report.mutants.is_empty() {
        return Closed::NothingOwed(
            "the catalog holds no mutant, so no route narrowed by anything a measurement would \
             say",
        );
    }
    if touched.is_some_and(|record| !record.targets.is_empty()) {
        return Closed::NothingOwed(
            "every route rests on what the guards recorded, which the touch layer re-decides, \
             so the coverage measurement narrowed nothing",
        );
    }
    Closed::Missing
}

/// Every route the guards decided, re-decided from what the guards recorded.
pub(super) fn touch(report: &Report, touched: Option<&Guarded>, audit: &mut Audit) -> Decided {
    let mut notes = Notes::on(audit, Layer::Touch);
    let routed = report
        .mutants
        .iter()
        .filter(|row| {
            row.route
                .as_ref()
                .is_some_and(|route| route.granularity == Granularity::Test)
        })
        .count();
    let Some(record) = touched else {
        if routed > 0 {
            notes.violated(
                "record",
                format!(
                    "{} put a mutation to some of a target's tests and the run kept no record of \
                     what its guards reached, so which tests those should have been cannot be \
                     re-derived",
                    plural(routed, "route")
                ),
            );
        }
        if routed == 0 {
            return notes.absent(
                "no mutation was put to some of a target's tests, and the run kept no record of what its guards reached",
            );
        }
        return notes.looked();
    };
    let recorded = Recorded::of(record);
    if recorded.targets.is_empty() {
        if routed > 0 {
            notes.violated(
                "record",
                "the record names no target and the run narrowed by it anyway".to_owned(),
            );
        }
        if routed == 0 {
            return notes.absent(
                "the record of what the guards reached names no target, and nothing was narrowed by it",
            );
        }
        return notes.looked();
    }
    accounted_for(report, &recorded, &mut notes);
    for row in &report.mutants {
        if row.source_run_id.is_some() {
            continue;
        }
        let Some(route) = row
            .route
            .as_ref()
            .filter(|route| route.granularity == Granularity::Test)
        else {
            continue;
        };
        re_decided(row, route, &recorded, &mut notes);
    }
    notes.looked()
}

/// Whether the record accounts for every target the run built.
fn accounted_for(report: &Report, recorded: &Recorded, notes: &mut Notes<'_>) {
    for target in &report.targets {
        if !recorded.targets.contains_key(target.as_str()) && !recorded.excused.contains(target) {
            notes.violated(
                "record",
                format!(
                    "the run built {target} and the record neither names it nor says why it \
                     could not; a route that narrowed by this record narrowed by a target \
                     nobody asked"
                ),
            );
        }
    }
}

/// One row's route, re-decided from the record alone.
fn re_decided(row: &Row, route: &RouteDecision, recorded: &Recorded, notes: &mut Notes<'_>) {
    let removed: BTreeSet<&String> = route.discharged.iter().map(|(target, _)| target).collect();
    for (target, touches) in &recorded.targets {
        if removed.contains(target) {
            continue;
        }
        let reaching = match touches.reaching(row.index, &recorded.narrowing) {
            Ok(reaching) => reaching,
            Err(unkept) => {
                notes.unaudited(
                    row.label(),
                    format!(
                        "the record keeps no {} for {target}, so whether the route should keep \
                         it, and for which of its tests, cannot be re-derived",
                        unkept.word()
                    ),
                );
                continue;
            }
        };
        let named = route.reaching.iter().any(|one| one == target);
        match (&reaching, named) {
            (Reaching::Nothing, true) => notes.violated(
                row.label(),
                format!(
                    "the route keeps {target} and the record says nothing of it reached this \
                     mutation; a target kept for nothing is work, not a wrong answer, but the \
                     route and the record disagree"
                ),
            ),
            (Reaching::Nothing, false) | (_, true) => {}
            (Reaching::Whole | Reaching::Tests(_), false) => notes.violated(
                row.label(),
                format!(
                    "the record says {target} reached this mutation and the route left it out; \
                     a target the tests reach and nothing runs is a kill reported as a survivor"
                ),
            ),
        }
        let asked = Asked::of(route, target);
        match (reaching, &asked) {
            (Reaching::Tests(expected), asked) if named => {
                let expected: BTreeSet<&String> = expected.iter().collect();
                if asked.tests() != Some(&expected) {
                    notes.violated(
                        row.label(),
                        format!(
                            "the route puts this mutation to {} of {target} and the record says \
                             the tests that reached it are {}",
                            asked.said(),
                            listed(&expected)
                        ),
                    );
                }
            }
            (Reaching::Whole, Asked::These(_)) => notes.violated(
                row.label(),
                format!(
                    "the route narrows {target} to {} and the record cannot attribute what \
                     reached this mutation to any test of it",
                    asked.said()
                ),
            ),
            (Reaching::Nothing | Reaching::Tests(_), _) | (Reaching::Whole, Asked::Every) => {}
        }
    }
}

/// What a route asked one target for: every test of it where the route names none of its tests, and the tests it names otherwise, which may be none.
enum Asked<'a> {
    /// The route names no tests of the target, which asks every test it has.
    Every,
    /// The tests the route names, perhaps none.
    These(BTreeSet<&'a String>),
}

impl<'a> Asked<'a> {
    /// What `route` asked `target` for.
    fn of(route: &'a RouteDecision, target: &str) -> Self {
        match route.tests.get(target) {
            None => Self::Every,
            Some(named) => Self::These(named.iter().collect()),
        }
    }

    /// The tests asked, where the route named them.
    const fn tests(&self) -> Option<&BTreeSet<&'a String>> {
        match self {
            Self::Every => None,
            Self::These(named) => Some(named),
        }
    }

    /// What was asked, as a reader reads it.
    fn said(&self) -> String {
        match self {
            Self::Every => "every test".to_owned(),
            Self::These(named) if named.is_empty() => "no test".to_owned(),
            Self::These(named) => listed(named),
        }
    }
}

/// A set of names as a reader reads them.
fn listed(names: &BTreeSet<&String>) -> String {
    if names.is_empty() {
        return "none of them".to_owned();
    }
    names
        .iter()
        .map(|name| name.as_str())
        .collect::<Vec<&str>>()
        .join(", ")
}

/// What the guards recorded, read as data with no help from the engine that wrote it.
#[derive(Debug)]
struct Recorded {
    targets: BTreeMap<String, Touches>,
    excused: BTreeSet<String>,
    narrowing: Narrowing,
}

impl Recorded {
    fn of(document: &Guarded) -> Self {
        Self {
            targets: document
                .targets
                .iter()
                .map(|(target, touches)| (target.clone(), Touches::of(touches)))
                .collect(),
            excused: excused(&document.limitations),
            narrowing: Narrowing::of(document.narrowing.as_ref()),
        }
    }
}

/// The targets a list of limitations excuses, each written `<limitation>:<target>`.
fn excused(limitations: &[String]) -> BTreeSet<String> {
    limitations
        .iter()
        .filter_map(|one| one.split_once(':').map(|(_, target)| target.to_owned()))
        .collect()
}

/// Which of the records the tree could say anything about, which is what turns an absence into evidence; a list the record does not keep establishes nothing about any mutant.
#[derive(Debug)]
struct Narrowing {
    /// Every mutant whose guard evaluates its two branches, so `infected` is about it, where the record keeps the list.
    compared: Option<BTreeSet<u64>>,
    /// The marker each mutant's branch proof rests on, so `bodies` is about it, where the record keeps the map.
    bodies: Option<BTreeMap<u64, u64>>,
}

impl Narrowing {
    fn of(narrowing: Option<&GuardedNarrowing>) -> Self {
        Self {
            compared: narrowing
                .and_then(|one| one.compared.as_ref())
                .map(|compared| compared.iter().copied().collect()),
            bodies: narrowing.and_then(|one| one.bodies.clone()),
        }
    }

    /// Whether the record says the guard of the mutant at `index` compares its two branches.
    fn compares(&self, index: u64) -> bool {
        self.compared
            .as_ref()
            .is_some_and(|compared| compared.contains(&index))
    }

    /// The marker the branch proof of the mutant at `index` rests on, where the record says there is one.
    fn marker(&self, index: u64) -> Option<u64> {
        self.bodies
            .as_ref()
            .and_then(|bodies| bodies.get(&index).copied())
    }
}

/// One kind of record a target's guards keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kept {
    /// The mutant sites each test reached.
    Reached,
    /// The branch bodies each test entered.
    Bodies,
    /// The mutations each test saw a guard's branches part over.
    Infected,
    /// The items each test entered.
    Entered,
}

impl Kept {
    /// The record's own word for it.
    pub(super) const fn word(self) -> &'static str {
        match self {
            Self::Reached => "`reached`",
            Self::Bodies => "`bodies`",
            Self::Infected => "`infected`",
            Self::Entered => "`entered`",
        }
    }
}

/// One kind of thing the guards report, by the thread that reported it.
#[derive(Debug)]
struct Seen {
    /// What each named test of the target reported.
    tests: BTreeMap<String, BTreeSet<u64>>,
    /// What was reported where nothing named a test, which is therefore about every test of it.
    loose: BTreeSet<u64>,
}

impl Seen {
    /// What one kind of record says, where the target's record keeps that kind.
    ///
    /// The engine leaves out an empty map of tests and an empty list of loose sites, so a kind kept without one of them reports nothing there.
    fn of(kind: Option<&GuardedSeen>) -> Option<Self> {
        let kind = kind?;
        let tests = match &kind.tests {
            Some(tests) => tests
                .iter()
                .map(|(test, sites)| (test.clone(), sites.iter().copied().collect()))
                .collect(),
            None => BTreeMap::new(),
        };
        let loose = match &kind.loose {
            Some(loose) => loose.iter().copied().collect(),
            None => BTreeSet::new(),
        };
        Some(Self { tests, loose })
    }

    /// Whether this test reported `index`, where a report nothing could attribute is one every test made.
    fn by(&self, test: &str, index: u64) -> bool {
        self.loose.contains(&index)
            || self
                .tests
                .get(test)
                .is_some_and(|held| held.contains(&index))
    }

    /// Whether anything of the target reported `index`.
    fn any(&self, index: u64) -> bool {
        self.loose.contains(&index) || self.tests.values().any(|held| held.contains(&index))
    }

    /// The tests that reported `index`, by name.
    fn who(&self, index: u64) -> BTreeSet<String> {
        self.tests
            .iter()
            .filter(|(_, sites)| sites.contains(&index))
            .map(|(test, _)| test.clone())
            .collect()
    }
}

/// What one target's guards recorded, each kind where the record keeps it.
#[derive(Debug)]
struct Touches {
    /// The mutant sites each test reached.
    reached: Option<Seen>,
    /// The proved bodies each test entered, by the marker at the body's first statement.
    bodies: Option<Seen>,
    /// The mutations each test saw a guard's two branches part over.
    infected: Option<Seen>,
    /// The items whose bodies each test entered, by item index.
    entered: Option<Seen>,
    /// How many tests of this target the baseline ran, which is what "all of them" counts against.
    ran: usize,
}

impl Touches {
    fn of(target: &GuardedTarget) -> Self {
        Self {
            reached: Seen::of(target.reached.as_ref()),
            bodies: Seen::of(target.bodies.as_ref()),
            infected: Seen::of(target.infected.as_ref()),
            entered: Seen::of(target.entered.as_ref()),
            ran: target.ran.len(),
        }
    }

    /// Which of this target's tests could have noticed the mutation at `index`, re-derived from the record alone, or the kind of record it rests on that the target's record does not keep.
    fn reaching(&self, index: u64, narrowing: &Narrowing) -> Result<Reaching, Kept> {
        let reached = self.reached.as_ref().ok_or(Kept::Reached)?;
        if reached.loose.contains(&index) {
            return Ok(Reaching::Whole);
        }
        let mut kept = reached.who(index);
        if kept.is_empty() {
            return Ok(Reaching::Nothing);
        }
        if kept.len() >= self.ran {
            return Ok(Reaching::Whole);
        }
        if let Some(marker) = narrowing.marker(index) {
            let bodies = self.bodies.as_ref().ok_or(Kept::Bodies)?;
            kept.retain(|test| bodies.by(test, marker));
        }
        if narrowing.compares(index) {
            let infected = self.infected.as_ref().ok_or(Kept::Infected)?;
            kept.retain(|test| infected.by(test, index));
        }
        if kept.is_empty() {
            return Ok(Reaching::Nothing);
        }
        Ok(Reaching::Tests(kept))
    }
}

/// Every target whose tests could have noticed the mutation at `index`, re-derived from what the guards recorded: each with the tests it narrows to, with nothing where every test of it could, or with the kind of record the answer rests on that the target's record does not keep.
pub(super) fn reaching_targets(
    touched: &Guarded,
    index: u64,
) -> BTreeMap<String, Result<Option<BTreeSet<String>>, Kept>> {
    let recorded = Recorded::of(touched);
    recorded
        .targets
        .iter()
        .filter_map(
            |(target, touches)| match touches.reaching(index, &recorded.narrowing) {
                Ok(Reaching::Nothing) => None,
                Ok(Reaching::Whole) => Some((target.clone(), Ok(None))),
                Ok(Reaching::Tests(tests)) => Some((target.clone(), Ok(Some(tests)))),
                Err(unkept) => Some((target.clone(), Err(unkept))),
            },
        )
        .collect()
}

/// Which of a target's tests could have noticed one mutation.
#[derive(Debug)]
enum Reaching {
    /// Nothing of it reached the mutation.
    Nothing,
    /// Exactly these tests reached it.
    Tests(BTreeSet<String>),
    /// It reached the mutation where nothing named a test, so every test of it reaches.
    Whole,
}

pub(super) fn proofs(
    report: &Report,
    evidence: &CheckedEvidence<'_>,
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Proofs);
    let discharged = report
        .mutants
        .iter()
        .filter(|row| row.not_run(DISCHARGED))
        .count();
    let Some(counted) = report.column(UNPROVEN_DISCHARGED) else {
        notes.unaudited(
            DISCHARGED,
            "the report carries no discharged accounting column".to_owned(),
        );
        return notes.looked();
    };
    if count(discharged) != counted {
        notes.violated(
            DISCHARGED,
            format!(
                "the accounting says {counted} mutants were discharged and {discharged} rows \
                 say so"
            ),
        );
    }
    if let Some(recorded) = &evidence.recorded {
        selected(report, &recorded.events, &mut notes);
    }
    let measured = measured_every_target(
        report,
        evidence.reached.as_ref(),
        evidence.touched.as_ref(),
        &mut notes,
    );
    let claims = claimed(report);
    let discharge = discharge_scope(report, counted, &claims);
    match (measured, discharge) {
        (Closed::NothingOwed(_), Closed::NothingOwed(_)) => {
            return notes.absent(
                "no discharge is claimed and no route rests on the coverage measurement, so no \
                 proof is owed",
            );
        }
        (Closed::Missing, _) | (_, Closed::Missing) => {}
    }
    if claims.is_empty() {
        return notes.looked();
    }
    let recorded = evidence.touched.as_ref().map(Recorded::of);
    branch_discharges(&claims, recorded.as_ref(), evidence, &mut notes);
    infection_discharges(&claims, recorded.as_ref(), evidence, &mut notes);
    if let Some(recorded) = &evidence.recorded {
        never_ran(&claims, &recorded.routing, &mut notes);
    }
    notes.looked()
}

/// The discharge subject, closed from the report's own accounting: a run whose accounting claims no discharge and whose routes name none has no proof to re-derive; a column that claims any, or a route that names one, is owed the evidence its proofs rest on.
fn discharge_scope(report: &Report, counted: u64, claims: &[Discharged]) -> Closed {
    if claims.is_empty()
        && counted == 0
        && report.mutants.iter().all(|row| {
            row.route
                .as_ref()
                .is_none_or(|route| route.discharged.is_empty())
        })
    {
        return Closed::NothingOwed(
            "the report's own accounting claims no discharge and no route names one, so no \
             proof is owed",
        );
    }
    Closed::Missing
}

/// Every mutant that never ran says why, in the recording as well as in the report.
fn selected(report: &Report, events: &[Value], notes: &mut Notes<'_>) {
    let said: BTreeMap<String, String> = events
        .iter()
        .filter(|event| string(event, "type").as_deref() == Some("select"))
        .filter_map(|event| {
            let select = event.get("select")?;
            Some((string(select, "mutant")?, string(select, "reason")?))
        })
        .collect();
    for row in report
        .mutants
        .iter()
        .filter(|row| row.outcome == NOT_RUN && row.source_run_id.is_none())
    {
        let Some(reason) = said.get(&row.display_id) else {
            notes.violated(
                "select",
                format!(
                    "{} never ran and the recording does not say why",
                    row.label()
                ),
            );
            continue;
        };
        if !row.not_run(reason) {
            let reported = match row.not_run_reason {
                Some(held) => held.as_str(),
                None => "unexplained",
            };
            notes.violated(
                "select",
                format!(
                    "{} is {} in the report and {reason} in the recording",
                    row.label(),
                    reported
                ),
            );
        }
    }
}

/// One discharge a report claims: which mutant, which target, and which proof.
struct Discharged {
    mutant: String,
    index: u64,
    target: String,
    proof: String,
}

/// Every discharge the report's own routes name.
fn claimed(report: &Report) -> Vec<Discharged> {
    report
        .mutants
        .iter()
        .flat_map(|row| {
            row.route.iter().flat_map(|route| {
                route.discharged.iter().map(|(target, proof)| Discharged {
                    mutant: row.display_id.clone(),
                    index: row.index,
                    target: target.clone(),
                    proof: proof.clone(),
                })
            })
        })
        .collect()
}

/// What a guard record establishes about one event, without encoding a third state as `Option<bool>`.
enum Observation {
    Observed,
    Absent,
    Unavailable,
}

/// Whether the record says `claim`'s target entered the body its branch proof names; a target whose record keeps no `bodies` says nothing either way.
fn entered_the_body(recorded: &Recorded, claim: &Discharged) -> Observation {
    let Some(marker) = recorded.narrowing.marker(claim.index) else {
        return Observation::Unavailable;
    };
    let Some(bodies) = recorded
        .targets
        .get(&claim.target)
        .and_then(|touches| touches.bodies.as_ref())
    else {
        return Observation::Unavailable;
    };
    if bodies.any(marker) {
        Observation::Observed
    } else {
        Observation::Absent
    }
}

/// Whether the record says `claim`'s target ever saw the guard's two branches part; a target whose record keeps no `infected` says nothing either way.
fn saw_a_difference(recorded: &Recorded, claim: &Discharged) -> Observation {
    let index = claim.index;
    if !recorded.narrowing.compares(index) {
        return Observation::Unavailable;
    }
    let Some(infected) = recorded
        .targets
        .get(&claim.target)
        .and_then(|touches| touches.infected.as_ref())
    else {
        return Observation::Unavailable;
    };
    if infected.any(index) {
        Observation::Observed
    } else {
        Observation::Absent
    }
}

/// Re-derives every `branch-never-taken` discharge from what the guards recorded, and from the coverage regions for the ones they say nothing about.
fn branch_discharges(
    claims: &[Discharged],
    recorded: Option<&Recorded>,
    evidence: &CheckedEvidence<'_>,
    notes: &mut Notes<'_>,
) {
    let mut branch: Vec<&Discharged> = Vec::new();
    for claim in claims
        .iter()
        .filter(|claim| claim.proof == BRANCH_NEVER_TAKEN)
    {
        let observation = match recorded {
            Some(one) => entered_the_body(one, claim),
            None => Observation::Unavailable,
        };
        match observation {
            Observation::Observed => notes.violated(
                BRANCH_NEVER_TAKEN,
                format!(
                    "the guards of {} say it entered the body {} sits in, so it may have \
                     noticed it",
                    claim.target, claim.mutant
                ),
            ),
            Observation::Absent => {}
            Observation::Unavailable => branch.push(claim),
        }
    }
    if branch.is_empty() {
        return;
    }
    let (Some(reached), Some(catalog)) = (evidence.reached.as_ref(), evidence.catalog.as_ref())
    else {
        notes.unaudited(
            BRANCH_NEVER_TAKEN,
            format!(
                "{} discharges the guards say nothing about rest on a measurement and a \
                 catalog the run did not keep",
                branch.len()
            ),
        );
        return;
    };
    for claim in branch {
        let Some(row) = mutant_row(catalog, &claim.mutant) else {
            notes.unaudited(
                BRANCH_NEVER_TAKEN,
                format!("the catalog holds no row for {}", claim.mutant),
            );
            continue;
        };
        let Some(body) = row.get("branch") else {
            notes.violated(
                BRANCH_NEVER_TAKEN,
                format!(
                    "{} was discharged from {} by a branch proof the catalog does not hold",
                    claim.mutant, claim.target
                ),
            );
            continue;
        };
        let Some(path) = string(row, "path") else {
            notes.unaudited(
                BRANCH_NEVER_TAKEN,
                format!("the catalog row for {} names no path", claim.mutant),
            );
            continue;
        };
        match ran_the_body(reached, &claim.target, &path, body) {
            Observation::Observed => notes.violated(
                BRANCH_NEVER_TAKEN,
                format!(
                    "{} covered a region inside the body {} sits in, so it may have noticed it",
                    claim.target, claim.mutant
                ),
            ),
            Observation::Absent => {}
            Observation::Unavailable => notes.unaudited(
                BRANCH_NEVER_TAKEN,
                format!(
                    "the retained measurement cannot locate the body {} sits in",
                    claim.mutant
                ),
            ),
        }
    }
}

/// The catalog row of one mutant, by the identity a report names it with.
fn mutant_row<'a>(catalog: &'a Value, display_id: &str) -> Option<&'a Value> {
    catalog
        .get("mutants")?
        .as_array()?
        .iter()
        .find(|row| string(row, "display_id").as_deref() == Some(display_id))
}

/// Whether the target's measured run covered a region beginning inside the body.
fn ran_the_body(reached: &Measurement, target: &str, path: &str, body: &Value) -> Observation {
    let (Some(start_line), Some(start_column), Some(end_line), Some(end_column)) = (
        number(body, "start_line"),
        number(body, "start_column"),
        number(body, "end_line"),
        number(body, "end_column"),
    ) else {
        return Observation::Unavailable;
    };
    let start = (start_line, start_column);
    let end = (end_line, end_column);
    let Some(blocks) = reached.targets.get(target) else {
        return Observation::Unavailable;
    };
    for block in blocks.iter().filter(|block| block.file == path) {
        let position = (block.start.line, block.start.column);
        if position >= start && position < end {
            return Observation::Observed;
        }
    }
    Observation::Absent
}

/// Re-derives every `never-infected` discharge from what the guards recorded, and asks for the probe's own log where they say nothing.
fn infection_discharges(
    claims: &[Discharged],
    recorded: Option<&Recorded>,
    evidence: &CheckedEvidence<'_>,
    notes: &mut Notes<'_>,
) {
    for claim in claims.iter().filter(|claim| claim.proof == NEVER_INFECTED) {
        let observation = match recorded {
            Some(one) => saw_a_difference(one, claim),
            None => Observation::Unavailable,
        };
        match observation {
            Observation::Observed => notes.violated(
                NEVER_INFECTED,
                format!(
                    "the guards of {} say the two branches of {} answered differently there, \
                     so it may have noticed it",
                    claim.target, claim.mutant
                ),
            ),
            Observation::Absent => {}
            Observation::Unavailable => {
                let wanted = format!("{}.log", claim.target.replace('/', "-"));
                if !evidence.probe_logs.iter().any(|name| name == &wanted) {
                    notes.unaudited(
                        NEVER_INFECTED,
                        format!(
                            "{} was discharged from {} by a probe whose log the run did not keep",
                            claim.mutant, claim.target
                        ),
                    );
                }
            }
        }
    }
}

/// A discharged pair that then ran natively is a proof the run contradicted; a sealed execution is put where its own control reached, which a discharge does not decide.
fn never_ran(claims: &[Discharged], routing: &crate::route::Routing, notes: &mut Notes<'_>) {
    for claim in claims {
        if routing
            .executions()
            .iter()
            .any(|execution| match execution {
                crate::route::Execution::Native(exec) => {
                    exec.mutant == claim.mutant && exec.target == claim.target
                }
                crate::route::Execution::Sealed(_) => false,
            })
        {
            notes.violated(
                "discharge",
                format!(
                    "{} was discharged from {} and then executed against it",
                    claim.mutant, claim.target
                ),
            );
        }
    }
}

/// One item of the catalog a record keeps, read with no help from the engine that wrote it.
#[derive(Debug)]
struct CatalogItem {
    /// The index an entry marker names.
    index: u64,
    /// The workspace-relative path.
    path: String,
    /// The item as a reader writes it.
    name: String,
    /// The first byte of its body.
    start_byte: u64,
    /// One past the last byte of its body.
    end_byte: u64,
    /// Whether the tree records entering it.
    measurable: bool,
}

impl CatalogItem {
    /// Every item the record's catalog holds, in the order it holds them, where the record keeps a catalog.
    fn all(document: &Guarded) -> Option<Vec<Self>> {
        document
            .items
            .as_ref()
            .map(|items| items.iter().map(Self::of).collect())
    }

    /// One item of the catalog.
    fn of(item: &GuardedItem) -> Self {
        Self {
            index: item.index,
            path: item.path.clone(),
            name: item.name.clone(),
            start_byte: item.body.start,
            end_byte: item.body.end,
            measurable: item.measurable,
        }
    }

    /// The innermost item whose body holds every byte of `row`'s edit.
    fn holding<'a>(items: &'a [Self], row: &Row) -> Option<&'a Self> {
        items
            .iter()
            .filter(|item| {
                item.path == row.path
                    && item.start_byte <= row.start_byte
                    && row.end_byte <= item.end_byte
            })
            .filter_map(|item| Some((item.end_byte.checked_sub(item.start_byte)?, item)))
            .min_by_key(|(length, _)| *length)
            .map(|(_, item)| item)
    }
}

/// Every reached site and every kill, held to the items the entry markers say each test entered.
pub(super) fn entry(report: &Report, touched: Option<&Guarded>, audit: &mut Audit) -> Decided {
    let mut notes = Notes::on(audit, Layer::Entry);
    let Some(record) = touched else {
        notes.unaudited(
            "record",
            "the run kept no record of what its guards saw, so no entry can be re-derived"
                .to_owned(),
        );
        return notes.looked();
    };
    let Some(items) = CatalogItem::all(record) else {
        notes.unaudited(
            "items",
            "the record keeps no item catalog, so what a test entered cannot be held to anything"
                .to_owned(),
        );
        return notes.looked();
    };
    if items.is_empty() {
        notes.unaudited(
            "items",
            "the record names no item, so what a test entered cannot be held to anything"
                .to_owned(),
        );
        return notes.looked();
    }
    for (at, item) in items.iter().enumerate() {
        if item.index != count(at) {
            notes.violated(
                "items",
                format!(
                    "the item at position {at} of the catalog calls itself {}, so an index an \
                     entry marker names is not the item it says",
                    item.index
                ),
            );
        }
    }
    let recorded = Recorded::of(record);
    let rows: BTreeMap<u64, &Row> = report
        .mutants
        .iter()
        .filter(|row| row.source_run_id.is_none())
        .map(|row| (row.index, row))
        .collect();
    for (target, touches) in &recorded.targets {
        named_entries(target, touches, &items, &mut notes);
        reached_entered(target, touches, (&rows, &items), &mut notes);
    }
    for row in rows.values().filter(|row| row.outcome == KILLED) {
        killed_entered(row, &recorded, &items, &mut notes);
    }
    notes.looked()
}

/// Every index the record says a test entered is an item of the catalog, and one the pristine file lets record its entry; a target whose record keeps no `entered` names none.
fn named_entries(target: &str, touches: &Touches, items: &[CatalogItem], notes: &mut Notes<'_>) {
    let Some(entered) = &touches.entered else {
        return;
    };
    let named = entered
        .loose
        .iter()
        .chain(entered.tests.values().flat_map(|held| held.iter()));
    for index in named {
        if *index >= count(items.len()) {
            notes.violated(
                target,
                format!(
                    "the record says something of {target} entered item {index}, and the catalog \
                     holds {}",
                    items.len()
                ),
            );
        } else if match usize::try_from(*index) {
            Ok(at) => items.get(at),
            Err(_beyond_the_positions) => None,
        }
        .is_some_and(|item| item.index == *index && !item.measurable)
        {
            notes.violated(
                target,
                format!(
                    "the record says something of {target} entered item {index}, which the \
                     pristine file makes a const fn, so no marker of the tree can say it"
                ),
            );
        }
    }
}

/// The item a row sits in, or a violation saying why there is none a change could be routed by.
///
/// An item the catalog says nothing records entering is one the pristine file makes a const fn, whose mutants [ADR 0047](../../../docs/adr/0047-a-const-fn-is-mutated-where-nothing-evaluates-it-early.md) measures by the reach of their guards alone: the row names it, and no entered record is asked for or held to it.
fn sitting_in<'a>(
    row: &Row,
    items: &'a [CatalogItem],
    notes: &mut Notes<'_>,
) -> Option<&'a CatalogItem> {
    let Some(item) = CatalogItem::holding(items, row) else {
        notes.violated(
            row.label(),
            format!(
                "{} bytes {}..{} sit in no item of the catalog, so a change there is one nothing \
                 could be routed by",
                row.path, row.start_byte, row.end_byte
            ),
        );
        return None;
    };
    if item.name != row.item {
        notes.violated(
            row.label(),
            format!(
                "the catalog names the item holding it {} and the row names it {}",
                item.name, row.item
            ),
        );
    }
    Some(item)
}

/// A site a test reached is inside an item that test entered; a target whose record keeps no `reached`, or reached something and keeps no `entered`, cannot be held to it, which is said.
fn reached_entered(
    target: &str,
    touches: &Touches,
    (rows, items): (&BTreeMap<u64, &Row>, &[CatalogItem]),
    notes: &mut Notes<'_>,
) {
    let (reached, entered) = match (&touches.reached, &touches.entered) {
        (Some(reached), Some(entered)) => (reached, entered),
        (None, _) => {
            notes.unaudited(
                target,
                format!(
                    "the record keeps no {} for {target}, so the sites its tests reached cannot \
                     be held to the items they entered",
                    Kept::Reached.word()
                ),
            );
            return;
        }
        (Some(reached), None) => {
            if !reached.tests.is_empty() || !reached.loose.is_empty() {
                notes.unaudited(
                    target,
                    format!(
                        "the record keeps no {} for {target}, so the sites its tests reached \
                         cannot be held to the items they entered",
                        Kept::Entered.word()
                    ),
                );
            }
            return;
        }
    };
    for (test, sites) in &reached.tests {
        for site in sites {
            let Some(row) = rows.get(site) else {
                continue;
            };
            let Some(item) = sitting_in(row, items, notes) else {
                continue;
            };
            if item.measurable && !entered.by(test, item.index) {
                notes.violated(
                    row.label(),
                    format!(
                        "the guards say {test} of {target} reached it, and the entry markers say \
                         {test} never entered {}, the item it sits in",
                        item.name
                    ),
                );
            }
        }
    }
    for site in &reached.loose {
        let Some(row) = rows.get(site) else {
            continue;
        };
        let Some(item) = sitting_in(row, items, notes) else {
            continue;
        };
        if item.measurable && !entered.loose.contains(&item.index) {
            notes.violated(
                row.label(),
                format!(
                    "the guards say a thread of {target} no test answers for reached it, and no \
                     such thread entered {}, the item it sits in",
                    item.name
                ),
            );
        }
    }
}

/// A test that noticed a mutation entered the item the mutation is in.
fn killed_entered(row: &Row, recorded: &Recorded, items: &[CatalogItem], notes: &mut Notes<'_>) {
    let Some(touches) = recorded.targets.get(&row.target) else {
        if !recorded.excused.contains(&row.target) {
            notes.unaudited(
                row.label(),
                format!(
                    "{} noticed it and the record neither names that target nor says why",
                    row.target
                ),
            );
        }
        return;
    };
    let Some(item) = sitting_in(row, items, notes) else {
        return;
    };
    if !item.measurable {
        return;
    }
    let Some(entered) = &touches.entered else {
        notes.unaudited(
            row.label(),
            format!(
                "{} noticed it and the record keeps no {} for that target, so whether what \
                 noticed it entered {} cannot be re-derived",
                row.target,
                Kept::Entered.word(),
                item.name
            ),
        );
        return;
    };
    if row.killed_by.is_empty() {
        if !entered.any(item.index) {
            notes.violated(
                row.label(),
                format!(
                    "{} noticed it, and the entry markers say nothing of it entered {}, the item \
                     it sits in",
                    row.target, item.name
                ),
            );
        }
        return;
    }
    for test in &row.killed_by {
        if !entered.by(test, item.index) {
            notes.violated(
                row.label(),
                format!(
                    "{test} of {} noticed it, and the entry markers say {test} never entered {}, \
                     the item it sits in",
                    row.target, item.name
                ),
            );
        }
    }
}
