// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The report held to the files the run kept so that somebody else could re-decide it.
//!
//! A discharge removes an execution, so a report that names one without the
//! premises is a claim rather than a proof
//! ([ADR 0004](../../../docs/adr/0004-proof-layers-not-budgets.md)). Every
//! rule here is re-implemented from the documents alone: what each target's
//! guards recorded, what the coverage build measured, the catalog with the
//! bodies the compiler vouched for, and the log each probe process appended
//! to. Nothing calls the engine to ask whether it agrees with itself.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::recording::events;
use super::{Audit, Evidence, Layer, NOT_RUN, Notes, Report, Row, number, plural, string};

/// Every place a rule targets, against the decision the walk took about it.
///
/// A file the run may mutate has one decision per place: a candidate with its
/// guard form, or a skip with its reason. A file passed over whole has none of
/// either and one tally saying how much it hid. Anything else is a place the
/// walk saw and said nothing about, which is the one thing a reader cannot ask
/// the engine to explain.
pub(super) fn sites(evidence: &Evidence<'_>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Sites);
    if !evidence.sites {
        return;
    }
    let Some(recorded) = evidence.recorded else {
        notes.unaudited(
            "recording",
            "the run kept no recording, so the places the walk saw cannot be counted".to_owned(),
        );
        return;
    };
    let mut seen = 0usize;
    for event in events(recorded) {
        if string(&event, "type").unwrap_or_default() != "discover-file" {
            continue;
        }
        let Some(record) = event.get("discover") else {
            continue;
        };
        seen = seen.saturating_add(1);
        let path = string(record, "path").unwrap_or_default();
        let candidates = number(record, "candidates").unwrap_or_default();
        let places = record
            .get("sites")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let tallies = record
            .get("skips")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let hidden: u64 = tallies
            .iter()
            .filter_map(|skip| number(skip, "count"))
            .sum();
        if places == 0 && candidates == 0 && tallies.len() == 1 {
            continue;
        }
        let decided = u64::try_from(places).unwrap_or(u64::MAX);
        if decided != candidates.saturating_add(hidden) {
            notes.violated(
                &path,
                format!(
                    "the walk of {path} took {decided} decisions and the file holds \
                     {candidates} candidates and {hidden} skipped places; a place with \
                     neither is one it passed over without saying so"
                ),
            );
        }
    }
    if seen == 0 {
        notes.unaudited(
            "recording",
            "the recording names no file the walk went through".to_owned(),
        );
    }
}

/// The parts of one catalog, against the whole they say they are.
pub(super) fn merge(report: &Report, evidence: &Evidence<'_>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Merge);
    if evidence.shards.is_empty() {
        notes.unaudited(
            "shards",
            "no other part of this catalog was given, so whether the parts come to the whole \
             cannot be re-derived"
                .to_owned(),
        );
        return;
    }
    let mut indices: BTreeSet<u64> = report.mutants.iter().filter_map(|row| row.index).collect();
    let mut total = report.mutants.len();
    for (name, text) in &evidence.shards {
        let Ok(document) = serde_json::from_str::<Value>(text) else {
            notes.unaudited(
                name,
                "the part is not a document this audit can read".to_owned(),
            );
            continue;
        };
        let part = Report::of(&document);
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
        total = total.saturating_add(part.mutants.len());
        for row in &part.mutants {
            if let Some(index) = row.index
                && !indices.insert(index)
            {
                notes.violated(
                    name,
                    format!("index {index} is in two parts; a mutant belongs to one part"),
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
}

/// Re-derives every discharge the run claimed from the evidence it kept.
///
/// A discharge removes an execution, so a report that names one without the
/// premises is a claim rather than a proof. This reads the measurement and
/// the catalog the run kept, re-implements the rule — a target whose covered
/// regions begin nowhere inside the body a branch proof names cannot have
/// noticed the mutation — and says whether the run's own answer follows.
/// Whether the measurement the run kept accounts for every target the run built.
///
/// A route removes a target by saying that target ran and covered nothing
/// there, and it can only say that about a target the measurement **named**.
/// A measurement that names neither the target nor a reason it could not read
/// it is one a route could narrow by without anybody having looked, which is
/// how a kill becomes a survivor.
fn measured_every_target(report: &Report, reached: Option<&str>, notes: &mut Notes<'_>) {
    let Some(reached) = reached else {
        notes.unaudited(
            "measurement",
            "the run kept no measurement, so what it was allowed to narrow by cannot be \
             re-derived"
                .to_owned(),
        );
        return;
    };
    let Ok(document) = serde_json::from_str::<Value>(reached) else {
        notes.violated(
            "measurement",
            "the measurement the run kept is not a document".to_owned(),
        );
        return;
    };
    let named: BTreeSet<&str> = document
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
    let excused: BTreeSet<String> = document
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
///
/// A route decided by the guards makes two removals, and both are checked
/// here against the record rather than against the engine that made them. A
/// target the route leaves out is one the record has to name and has to say
/// reached nothing at that index: leaving out a target the record does not
/// account for is how a kill becomes a survivor. And a target the route
/// narrows to some of its tests has to be narrowed to exactly the tests the
/// record names, because a test dropped from that set is a test that would
/// have run and did not.
pub(super) fn touch(report: &Report, touched: Option<&str>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Touch);
    let routed = report
        .mutants
        .iter()
        .filter(|row| row.granularity == "test")
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
        return;
    };
    let Ok(document) = serde_json::from_str::<Value>(record) else {
        notes.violated(
            "record",
            "what the guards recorded is not a document".to_owned(),
        );
        return;
    };
    let recorded = Recorded::of(&document);
    if recorded.targets.is_empty() {
        if routed > 0 {
            notes.violated(
                "record",
                "the record names no target and the run narrowed by it anyway".to_owned(),
            );
        }
        return;
    }
    accounted_for(report, &recorded, &mut notes);
    for row in &report.mutants {
        if row.source_run_id.is_some() || row.granularity != "test" {
            continue;
        }
        let Some(index) = row.index else {
            notes.unaudited(
                row.label(),
                "the row carries no catalog index, so what the guards said about it cannot be \
                 looked up"
                    .to_owned(),
            );
            continue;
        };
        re_decided(row, index, &recorded, &mut notes);
    }
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
fn re_decided(row: &Row, index: u64, recorded: &Recorded, notes: &mut Notes<'_>) {
    let removed: BTreeSet<&String> = row.discharged.iter().map(|(target, _)| target).collect();
    for (target, touches) in &recorded.targets {
        if removed.contains(target) {
            continue;
        }
        let reaching = touches.reaching(index);
        let named = row.reaching.iter().any(|one| one == target);
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
        let asked: BTreeSet<&String> = row
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
        Self { targets, excused }
    }
}

/// What one target's guards recorded.
#[derive(Debug, Default)]
struct Touches {
    tests: BTreeMap<String, BTreeSet<u64>>,
    loose: BTreeSet<u64>,
    ran: usize,
}

impl Touches {
    fn of(value: &Value) -> Self {
        Self {
            tests: value
                .get("tests")
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
                .get("loose")
                .and_then(Value::as_array)
                .map(|entries| entries.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default(),
            ran: value
                .get("ran")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
        }
    }

    /// Which of this target's tests reached `index`, re-derived from the record alone.
    fn reaching(&self, index: u64) -> Reaching {
        if self.loose.contains(&index) {
            return Reaching::Whole;
        }
        let named: BTreeSet<String> = self
            .tests
            .iter()
            .filter(|(_, sites)| sites.contains(&index))
            .map(|(test, _)| test.clone())
            .collect();
        if named.is_empty() {
            return Reaching::Nothing;
        }
        if named.len() >= self.ran {
            return Reaching::Whole;
        }
        Reaching::Tests(named)
    }
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

pub(super) fn proofs(report: &Report, evidence: &Evidence<'_>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Proofs);
    let discharged = report
        .mutants
        .iter()
        .filter(|row| row.not_run_reason.as_deref() == Some("discharged"))
        .count();
    let counted = report
        .columns
        .get("discharged")
        .copied()
        .unwrap_or_default();
    if u64::try_from(discharged).unwrap_or(u64::MAX) != counted {
        notes.violated(
            "discharged",
            format!(
                "the accounting says {counted} mutants were discharged and {discharged} rows \
                 say so"
            ),
        );
    }
    if let Some(recorded) = evidence.recorded {
        selected(report, recorded, &mut notes);
    }
    measured_every_target(report, evidence.reached, &mut notes);
    let claims = claimed(report);
    if claims.is_empty() {
        notes.unaudited(
            "discharge",
            "the run discharged nothing, so there is no proof to re-derive".to_owned(),
        );
        return;
    }
    branch_discharges(&claims, evidence, &mut notes);
    infection_discharges(&claims, evidence, &mut notes);
    if let Some(recorded) = evidence.recorded {
        never_ran(&claims, recorded, &mut notes);
    }
}

/// Every mutant that never ran says why, in the recording as well as in the report.
fn selected(report: &Report, recorded: &str, notes: &mut Notes<'_>) {
    let said: BTreeMap<String, String> = events(recorded)
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
        if row.not_run_reason.as_deref() != Some(reason.as_str()) {
            notes.violated(
                "select",
                format!(
                    "{} is {} in the report and {reason} in the recording",
                    row.label(),
                    row.not_run_reason.as_deref().unwrap_or("unexplained")
                ),
            );
        }
    }
}

/// One discharge a report claims: which mutant, which target, and which proof.
struct Discharged {
    mutant: String,
    target: String,
    proof: String,
}

/// Every discharge the report's own routes name.
fn claimed(report: &Report) -> Vec<Discharged> {
    report
        .mutants
        .iter()
        .flat_map(|row| {
            row.discharged.iter().map(|(target, proof)| Discharged {
                mutant: row.display_id.clone(),
                target: target.clone(),
                proof: proof.clone(),
            })
        })
        .collect()
}

/// Re-derives every `branch-never-taken` discharge from the measurement and the body the catalog names.
fn branch_discharges(claims: &[Discharged], evidence: &Evidence<'_>, notes: &mut Notes<'_>) {
    let branch: Vec<&Discharged> = claims
        .iter()
        .filter(|claim| claim.proof == "branch-never-taken")
        .collect();
    if branch.is_empty() {
        return;
    }
    let (Some(reached), Some(catalog)) = (evidence.reached, evidence.catalog) else {
        notes.unaudited(
            "branch-never-taken",
            format!(
                "{} discharges rest on a measurement and a catalog the run did not keep",
                branch.len()
            ),
        );
        return;
    };
    let Ok(reached) = serde_json::from_str::<Value>(reached) else {
        notes.unaudited(
            "branch-never-taken",
            "the measurement the run kept is not a document".to_owned(),
        );
        return;
    };
    let Ok(catalog) = serde_json::from_str::<Value>(catalog) else {
        notes.unaudited(
            "branch-never-taken",
            "the catalog the run kept is not a document".to_owned(),
        );
        return;
    };
    for claim in branch {
        let Some(row) = mutant_row(&catalog, &claim.mutant) else {
            notes.unaudited(
                "branch-never-taken",
                format!("the catalog holds no row for {}", claim.mutant),
            );
            continue;
        };
        let Some(body) = row.get("branch") else {
            notes.violated(
                "branch-never-taken",
                format!(
                    "{} was discharged from {} by a branch proof the catalog does not hold",
                    claim.mutant, claim.target
                ),
            );
            continue;
        };
        let path = string(row, "path").unwrap_or_default();
        if ran_the_body(&reached, &claim.target, &path, body) {
            notes.violated(
                "branch-never-taken",
                format!(
                    "{} covered a region inside the body {} sits in, so it may have noticed it",
                    claim.target, claim.mutant
                ),
            );
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
fn ran_the_body(reached: &Value, target: &str, path: &str, body: &Value) -> bool {
    let number = |value: &Value, key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0);
    let start = (number(body, "start_line"), number(body, "start_column"));
    let end = (number(body, "end_line"), number(body, "end_column"));
    let Some(blocks) = reached
        .get("targets")
        .and_then(|targets| targets.get(target))
        .and_then(Value::as_array)
    else {
        return false;
    };
    blocks.iter().any(|block| {
        if string(block, "file").as_deref() != Some(path) {
            return false;
        }
        let Some(at) = block.get("start") else {
            return false;
        };
        let position = (number(at, "line"), number(at, "column"));
        position >= start && position < end
    })
}

/// Every `never-infected` discharge rests on a log the run kept.
fn infection_discharges(claims: &[Discharged], evidence: &Evidence<'_>, notes: &mut Notes<'_>) {
    for claim in claims
        .iter()
        .filter(|claim| claim.proof == "never-infected")
    {
        let wanted = format!("{}.log", claim.target.replace('/', "-"));
        if !evidence.probe_logs.iter().any(|name| name == &wanted) {
            notes.unaudited(
                "never-infected",
                format!(
                    "{} was discharged from {} by a probe whose log the run did not keep",
                    claim.mutant, claim.target
                ),
            );
        }
    }
}

/// A discharged pair that then ran is a proof the run contradicted.
fn never_ran(claims: &[Discharged], recorded: &str, notes: &mut Notes<'_>) {
    let routing = crate::route::read(recorded);
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
