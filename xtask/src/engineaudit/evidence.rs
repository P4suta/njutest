// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report held to the files the run kept so that somebody else could re-decide it.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{
    Audit, BRANCH_NEVER_TAKEN, CheckedEvidence, DISCHARGED, Decided, Granularity, KILLED, Layer,
    NEVER_INFECTED, NOT_RUN, Notes, Report, RouteDecision, Row, count, number, plural, string,
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

/// The parts of one catalog, against the whole they say they are.
pub(super) fn merge(report: &Report, evidence: &CheckedEvidence<'_>, audit: &mut Audit) -> Decided {
    let mut notes = Notes::on(audit, Layer::Merge);
    if evidence.shards.is_empty() {
        notes.unaudited(
            "shards",
            "no other part of this catalog was given, so whether the parts come to the whole \
             cannot be re-derived"
                .to_owned(),
        );
        return notes.looked();
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

/// Re-derives every discharge the run claimed from the evidence it kept.
fn measured_every_target(report: &Report, reached: Option<&Value>, notes: &mut Notes<'_>) {
    let Some(reached) = reached else {
        notes.unaudited(
            "measurement",
            "the run kept no measurement, so what it was allowed to narrow by cannot be \
             re-derived"
                .to_owned(),
        );
        return;
    };
    let named: BTreeSet<&str> = reached
        .get("targets")
        .and_then(Value::as_object)
        .map(|targets| targets.keys().map(String::as_str).collect())
        .unwrap_or_default();
    if named.is_empty() {
        notes.unaudited(
            "measurement",
            "the measurement names no target, so the run narrowed by nothing".to_owned(),
        );
        return;
    }
    let excused: BTreeSet<String> = reached
        .get("limitations")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|one| one.split_once(':').map(|(_, target)| target.to_owned()))
                .collect()
        })
        .unwrap_or_default();
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
}

/// Every route the guards decided, re-decided from what the guards recorded.
pub(super) fn touch(report: &Report, touched: Option<&Value>, audit: &mut Audit) -> Decided {
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
        let reaching = touches.reaching(row.index, &recorded.narrowing);
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
        let asked: BTreeSet<&String> = route
            .tests
            .get(target)
            .map(|named| named.iter().collect())
            .unwrap_or_default();
        match reaching {
            Reaching::Tests(expected) if named => {
                let expected: BTreeSet<&String> = expected.iter().collect();
                if asked != expected {
                    notes.violated(
                        row.label(),
                        format!(
                            "the route puts this mutation to {} of {target} and the record says \
                             the tests that reached it are {}",
                            asked_for(&asked),
                            listed(&expected)
                        ),
                    );
                }
            }
            Reaching::Whole if !asked.is_empty() => notes.violated(
                row.label(),
                format!(
                    "the route narrows {target} to {} and the record cannot attribute what \
                     reached this mutation to any test of it",
                    listed(&asked)
                ),
            ),
            Reaching::Nothing | Reaching::Tests(_) | Reaching::Whole => {}
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

/// What a route asked one target for, where naming nothing means every test it has.
fn asked_for(names: &BTreeSet<&String>) -> String {
    if names.is_empty() {
        return "every test".to_owned();
    }
    listed(names)
}

/// What the guards recorded, read as data with no help from the engine that wrote it.
#[derive(Debug, Default)]
struct Recorded {
    targets: BTreeMap<String, Touches>,
    excused: BTreeSet<String>,
    narrowing: Narrowing,
}

impl Recorded {
    fn of(document: &Value) -> Self {
        let targets = document
            .get("targets")
            .and_then(Value::as_object)
            .map(|named| {
                named
                    .iter()
                    .map(|(target, touches)| (target.clone(), Touches::of(touches)))
                    .collect()
            })
            .unwrap_or_default();
        let excused = document
            .get("limitations")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(Value::as_str)
                    .filter_map(|one| one.split_once(':').map(|(_, target)| target.to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            targets,
            excused,
            narrowing: Narrowing::of(document.get("narrowing")),
        }
    }
}

/// Which of the records the tree could say anything about, which is what turns an absence into evidence.
#[derive(Debug, Default)]
struct Narrowing {
    /// Every mutant whose guard evaluates its two branches, so `infected` is about it.
    compared: BTreeSet<u64>,
    /// The marker each mutant's branch proof rests on, so `bodies` is about it.
    bodies: BTreeMap<u64, u64>,
}

impl Narrowing {
    fn of(value: Option<&Value>) -> Self {
        let Some(value) = value else {
            return Self::default();
        };
        Self {
            compared: value
                .get("compared")
                .and_then(Value::as_array)
                .map(|entries| entries.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default(),
            bodies: value
                .get("bodies")
                .and_then(Value::as_object)
                .map(|named| {
                    named
                        .iter()
                        .filter_map(|(index, marker)| {
                            match (index.parse::<u64>(), marker.as_u64()) {
                                (Ok(index), Some(marker)) => Some((index, marker)),
                                (Err(_) | Ok(_), None) | (Err(_), Some(_)) => None,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

/// One kind of thing the guards report, by the thread that reported it.
#[derive(Debug, Default)]
struct Seen {
    /// What each named test of the target reported.
    tests: BTreeMap<String, BTreeSet<u64>>,
    /// What was reported where nothing named a test, which is therefore about every test of it.
    loose: BTreeSet<u64>,
}

impl Seen {
    fn of(value: &Value, kind: &str) -> Self {
        Self {
            tests: value
                .get(kind)
                .and_then(|seen| seen.get("tests"))
                .and_then(Value::as_object)
                .map(|named| {
                    named
                        .iter()
                        .map(|(test, sites)| {
                            (
                                test.clone(),
                                sites
                                    .as_array()
                                    .map(|entries| {
                                        entries.iter().filter_map(Value::as_u64).collect()
                                    })
                                    .unwrap_or_default(),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default(),
            loose: value
                .get(kind)
                .and_then(|seen| seen.get("loose"))
                .and_then(Value::as_array)
                .map(|entries| entries.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default(),
        }
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

/// What one target's guards recorded.
#[derive(Debug, Default)]
struct Touches {
    /// The mutant sites each test reached.
    reached: Seen,
    /// The proved bodies each test entered, by the marker at the body's first statement.
    bodies: Seen,
    /// The mutations each test saw a guard's two branches part over.
    infected: Seen,
    /// The items whose bodies each test entered, by item index.
    entered: Seen,
    /// How many tests of this target the baseline ran, which is what "all of them" counts against.
    ran: usize,
}

impl Touches {
    fn of(value: &Value) -> Self {
        Self {
            reached: Seen::of(value, "reached"),
            bodies: Seen::of(value, "bodies"),
            infected: Seen::of(value, "infected"),
            entered: Seen::of(value, "entered"),
            ran: value
                .get("ran")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
        }
    }

    /// Which of this target's tests could have noticed the mutation at `index`, re-derived from the record alone.
    fn reaching(&self, index: u64, narrowing: &Narrowing) -> Reaching {
        if self.reached.loose.contains(&index) {
            return Reaching::Whole;
        }
        let named = self.reached.who(index);
        if named.is_empty() {
            return Reaching::Nothing;
        }
        if named.len() >= self.ran {
            return Reaching::Whole;
        }
        let kept: BTreeSet<String> = named
            .into_iter()
            .filter(|test| {
                narrowing
                    .bodies
                    .get(&index)
                    .is_none_or(|marker| self.bodies.by(test, *marker))
            })
            .filter(|test| !narrowing.compared.contains(&index) || self.infected.by(test, index))
            .collect();
        if kept.is_empty() {
            return Reaching::Nothing;
        }
        Reaching::Tests(kept)
    }
}

/// Every target whose tests could have noticed the mutation at `index`, re-derived from what the guards recorded: each with the tests it narrows to, or with nothing where every test of it could.
pub(super) fn reaching_targets(
    touched: &Value,
    index: u64,
) -> BTreeMap<String, Option<BTreeSet<String>>> {
    let recorded = Recorded::of(touched);
    recorded
        .targets
        .iter()
        .filter_map(
            |(target, touches)| match touches.reaching(index, &recorded.narrowing) {
                Reaching::Nothing => None,
                Reaching::Whole => Some((target.clone(), None)),
                Reaching::Tests(tests) => Some((target.clone(), Some(tests))),
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
    let Some(counted) = report.column(DISCHARGED) else {
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
    measured_every_target(report, evidence.reached.as_ref(), &mut notes);
    let claims = claimed(report);
    if claims.is_empty() {
        notes.unaudited(
            "discharge",
            "the run discharged nothing, so there is no proof to re-derive".to_owned(),
        );
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

/// Whether the record says `claim`'s target entered the body its branch proof names.
fn entered_the_body(recorded: &Recorded, claim: &Discharged) -> Observation {
    let Some(marker) = recorded.narrowing.bodies.get(&claim.index).copied() else {
        return Observation::Unavailable;
    };
    let Some(touches) = recorded.targets.get(&claim.target) else {
        return Observation::Unavailable;
    };
    if touches.bodies.any(marker) {
        Observation::Observed
    } else {
        Observation::Absent
    }
}

/// Whether the record says `claim`'s target ever saw the guard's two branches part.
fn saw_a_difference(recorded: &Recorded, claim: &Discharged) -> Observation {
    let index = claim.index;
    if !recorded.narrowing.compared.contains(&index) {
        return Observation::Unavailable;
    }
    let Some(touches) = recorded.targets.get(&claim.target) else {
        return Observation::Unavailable;
    };
    if touches.infected.any(index) {
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
fn ran_the_body(reached: &Value, target: &str, path: &str, body: &Value) -> Observation {
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
    let Some(blocks) = reached
        .get("targets")
        .and_then(|targets| targets.get(target))
        .and_then(Value::as_array)
    else {
        return Observation::Unavailable;
    };
    for block in blocks {
        let Some(file) = string(block, "file") else {
            return Observation::Unavailable;
        };
        if file != path {
            continue;
        }
        let Some(at) = block.get("start") else {
            return Observation::Unavailable;
        };
        let (Some(line), Some(column)) = (number(at, "line"), number(at, "column")) else {
            return Observation::Unavailable;
        };
        let position = (line, column);
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

/// A discharged pair that then ran is a proof the run contradicted.
fn never_ran(claims: &[Discharged], routing: &crate::route::Routing, notes: &mut Notes<'_>) {
    for claim in claims {
        if routing
            .execs
            .iter()
            .any(|exec| exec.mutant == claim.mutant && exec.target == claim.target)
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
    /// Every item the record's catalog holds, in the order it holds them; the wire check has already refused an item missing a field.
    fn all(document: &Value) -> Vec<Self> {
        document
            .get("items")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Self::of).collect())
            .unwrap_or_default()
    }

    /// One item, when every field it needs is there.
    fn of(item: &Value) -> Option<Self> {
        let body = item.get("body")?;
        Some(Self {
            index: number(item, "index")?,
            path: string(item, "path")?,
            name: string(item, "name")?,
            start_byte: number(body, "start")?,
            end_byte: number(body, "end")?,
            measurable: item.get("measurable")?.as_bool()?,
        })
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
pub(super) fn entry(report: &Report, touched: Option<&Value>, audit: &mut Audit) -> Decided {
    let mut notes = Notes::on(audit, Layer::Entry);
    let Some(record) = touched else {
        notes.unaudited(
            "record",
            "the run kept no record of what its guards saw, so no entry can be re-derived"
                .to_owned(),
        );
        return notes.looked();
    };
    let items = CatalogItem::all(record);
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

/// Every index the record says a test entered is an item of the catalog.
fn named_entries(target: &str, touches: &Touches, items: &[CatalogItem], notes: &mut Notes<'_>) {
    let named = touches
        .entered
        .loose
        .iter()
        .chain(touches.entered.tests.values().flat_map(|held| held.iter()));
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
        }
    }
}

/// The item a row sits in, or a violation saying why there is none a change could be routed by.
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
    if !item.measurable {
        notes.violated(
            row.label(),
            format!(
                "the innermost item holding it is {}, which the catalog says nothing records \
                 entering, and a mutation was made in it anyway",
                item.name
            ),
        );
        return None;
    }
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

/// A site a test reached is inside an item that test entered.
fn reached_entered(
    target: &str,
    touches: &Touches,
    (rows, items): (&BTreeMap<u64, &Row>, &[CatalogItem]),
    notes: &mut Notes<'_>,
) {
    for (test, sites) in &touches.reached.tests {
        for site in sites {
            let Some(row) = rows.get(site) else {
                continue;
            };
            let Some(item) = sitting_in(row, items, notes) else {
                continue;
            };
            if !touches.entered.by(test, item.index) {
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
    for site in &touches.reached.loose {
        let Some(row) = rows.get(site) else {
            continue;
        };
        let Some(item) = sitting_in(row, items, notes) else {
            continue;
        };
        if !touches.entered.loose.contains(&item.index) {
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
    if row.killed_by.is_empty() {
        if !touches.entered.any(item.index) {
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
        if !touches.entered.by(test, item.index) {
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
