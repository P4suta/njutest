// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An independent re-decision of what a completed run recorded. [ADR 0004](../../docs/adr/0004-proof-layers-not-budgets.md) ships a proof layer only against a re-implementation that never calls the runner's, so nothing here consults the code that wrote the report: every verdict is re-derived from the recording alone, and wherever the recording does not carry enough to re-derive one, that is said plainly rather than read as agreement.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;

/// The document a completed run leaves in its directory.
pub const REPORT_FILE: &str = "njutest-assurance-report-v1.json";

/// The schema this audit knows how to re-decide.
pub const SCHEMA: &str = "njutest-assurance-report-v1";

/// The exit code a run directory that could not be read earns, kept apart from the audit's own so that "I could not look" never reads as "I looked and found nothing".
pub const EXIT_UNREADABLE: u8 = 2;

const KILLED: &str = "killed";
const SURVIVED: &str = "survived";
const UNREACHED: &str = "unreached";
const REJECTED: &str = "compile-rejected";
const TIMED_OUT: &str = "timed_out";
const PASSED: &str = "passed";
const SURVIVING_MUTANT: &str = "surviving-mutant";
const UNMATCHED_ACCEPTANCE: &str = "unmatched-acceptance";
const EVERYTHING: &str = "all";
const EQUIVALENT: &str = "equivalent";

/// Why a recording could not be re-decided at all.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuditError {
    /// The run directory does not hold the report a completed run leaves behind.
    #[error("{path}: a completed run leaves its report here: {source}")]
    Unreadable {
        /// Where the report was looked for.
        path: String,
        /// What the filesystem said.
        #[source]
        source: std::io::Error,
    },
    /// The document is there and is not JSON.
    #[error("{path}: not a document this audit can read: {source}")]
    Unparsable {
        /// The document.
        path: String,
        /// What serde said.
        #[source]
        source: serde_json::Error,
    },
    /// The document is JSON and calls itself something other than the assurance report.
    #[error("{path}: {schema:?} is not the assurance report this audit re-decides")]
    Unrecognised {
        /// The document.
        path: String,
        /// What it calls itself.
        schema: String,
    },
}

/// What the re-decision was able to conclude about one thing it looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Standing {
    /// The recording contradicts itself, or rests a verdict on evidence it does not hold.
    Violated,
    /// The recording does not carry what a re-decision would need, which is neither a pass nor a failure.
    Unaudited,
}

impl Standing {
    /// What to write at the head of a line.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Violated => "violation",
            Self::Unaudited => "unaudited",
        }
    }
}

/// The part of a recording one re-decision was about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Layer {
    /// The columns of the accounting, against the records they summarise and against the verdict they carry.
    Accounting,
    /// The target a kill is attributed to.
    Killers,
    /// The correspondence between the mutations nothing noticed and the findings that name them.
    Findings,
    /// Whether an unmatched acceptance really fails to resolve in the complete catalog.
    Acceptances,
    /// The earlier run a disposition was read back from.
    Reuse,
    /// The layers that removed an execution, held to the kills the run recorded.
    Proofs,
    /// The targets the recording says were put to mutations and noticed none, held to the findings that name them.
    Hollow,
    /// The faults a seam recording licensed, re-derived and held to what the run says it put and what nothing noticed.
    Wire,
}

impl Layer {
    /// What to write in a report.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Accounting => "accounting",
            Self::Killers => "killers",
            Self::Findings => "findings",
            Self::Acceptances => "acceptances",
            Self::Reuse => "reuse",
            Self::Proofs => "proofs",
            Self::Hollow => "hollow",
            Self::Wire => "wire",
        }
    }
}

/// One thing the re-decision has to say about one part of one recording.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Remark {
    /// The part of the recording it is about.
    pub layer: Layer,
    /// What the re-decision concluded.
    pub standing: Standing,
    /// The column, mutant, or finding it names.
    pub subject: String,
    /// One sentence a person can act on.
    pub detail: String,
}

impl fmt::Display for Remark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}: {}: {}",
            self.standing.label(),
            self.layer.label(),
            self.subject,
            self.detail
        )
    }
}

/// What an independent re-decision made of one run's recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audit {
    /// The run the recording names.
    pub run_id: String,
    /// How many mutant records it re-decided over.
    pub mutants: usize,
    /// How many target records it re-decided over.
    pub targets: usize,
    /// Everything it has to say, grouped by layer with the violations of each first.
    pub remarks: Vec<Remark>,
}

impl Audit {
    /// How many things the recording does not support.
    #[must_use]
    pub fn violations(&self) -> usize {
        self.standing(Standing::Violated)
    }

    /// How many things the recording does not carry enough to re-decide.
    #[must_use]
    pub fn unaudited(&self) -> usize {
        self.standing(Standing::Unaudited)
    }

    /// The exit code this audit earns. A recording that could not be read at all never reaches here and earns [`EXIT_UNREADABLE`] instead.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        u8::from(self.violations() > 0)
    }

    fn standing(&self, standing: Standing) -> usize {
        self.remarks
            .iter()
            .filter(|remark| remark.standing == standing)
            .count()
    }
}

impl fmt::Display for Audit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for remark in &self.remarks {
            writeln!(f, "{remark}")?;
        }
        write!(
            f,
            "proofaudit: {}: {} and {} re-decided; {}, {} unaudited",
            self.run_id,
            plural(self.mutants, "mutant"),
            plural(self.targets, "target"),
            plural(self.violations(), "violation"),
            self.unaudited()
        )
    }
}

/// Where one layer's re-decisions are written down, so that a check names only the subject and the sentence and never repeats which layer it belongs to.
#[derive(Debug)]
struct Notes<'a> {
    audit: &'a mut Audit,
    layer: Layer,
}

impl<'a> Notes<'a> {
    const fn on(audit: &'a mut Audit, layer: Layer) -> Self {
        Self { audit, layer }
    }

    fn violated(&mut self, subject: &str, detail: String) {
        self.note(Standing::Violated, subject, detail);
    }

    fn unaudited(&mut self, subject: &str, detail: String) {
        self.note(Standing::Unaudited, subject, detail);
    }

    fn note(&mut self, standing: Standing, subject: &str, detail: String) {
        self.audit.remarks.push(Remark {
            layer: self.layer,
            standing,
            subject: subject.to_owned(),
            detail,
        });
    }

    /// One column against the records it summarises.
    fn tally(&mut self, column: Column<'_>) {
        let Column {
            subject,
            recorded,
            derived,
            records,
        } = column;
        match recorded {
            None => self.unaudited(
                subject,
                format!("the recording omits this column, so {records} answer to nothing"),
            ),
            Some(count) if count != derived => self.violated(
                subject,
                format!(
                    "the column says {count} and {records} come to {derived}; a report that \
                     contradicts itself is not evidence of anything"
                ),
            ),
            Some(_) => {}
        }
    }

    /// One equation the assurance contract states, over the columns alone.
    fn holds(&mut self, equation: Equation<'_>) {
        let Equation {
            subject,
            relation,
            sides,
            because,
        } = equation;
        let (Some(left), Some(right)) = sides else {
            self.unaudited(
                subject,
                format!(
                    "the recording omits a column this equation is over, so it cannot be \
                     re-decided: {because}"
                ),
            );
            return;
        };
        if !relation.holds(left, right) {
            self.violated(
                subject,
                format!(
                    "the recording's own columns come to {left} where they must come to {} \
                     {right}; {because}",
                    relation.word()
                ),
            );
        }
    }
}

/// One column of the accounting and the records that must agree with it.
#[derive(Debug, Clone, Copy)]
struct Column<'a> {
    subject: &'a str,
    recorded: Option<u64>,
    derived: u64,
    records: &'a str,
}

/// One relation the columns must stand in to each other, and why.
#[derive(Debug, Clone, Copy)]
struct Equation<'a> {
    subject: &'a str,
    relation: Relation,
    sides: (Option<u64>, Option<u64>),
    because: &'a str,
}

/// What an independent re-decision makes of the recording in `text`.
///
/// # Errors
/// [`AuditError::Unparsable`] for a document that is not JSON, and [`AuditError::Unrecognised`] for one that is not the assurance report.
pub fn audit(path: &str, text: &str, recorded: Option<&str>) -> Result<Audit, AuditError> {
    let document: serde_json::Value =
        serde_json::from_str(text).map_err(|source| AuditError::Unparsable {
            path: path.to_owned(),
            source,
        })?;
    let schema = document
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if schema != SCHEMA {
        return Err(AuditError::Unrecognised {
            path: path.to_owned(),
            schema: schema.to_owned(),
        });
    }
    let recording = Recording::of(&document);
    let mut audit = Audit {
        run_id: recording.run_id.clone(),
        mutants: recording.mutants.len(),
        targets: recording.targets.len(),
        remarks: Vec::new(),
    };
    target_columns(&recording, &mut audit);
    mutant_columns(&recording, &mut audit);
    equations(&recording, &mut audit);
    verdict(&recording, &mut audit);
    killers(&recording, &mut audit);
    findings(&recording, &mut audit);
    acceptances(&recording, &mut audit);
    reuse(&recording, &mut audit);
    proofs(&recording, recorded, &mut audit);
    hollow(&recording, recorded, &mut audit);
    wire(&recording, recorded, &mut audit);
    audit.remarks.sort();
    audit.remarks.dedup();
    Ok(audit)
}

#[derive(Debug)]
struct TargetRow {
    id: String,
    name: String,
    status: String,
}

#[derive(Debug)]
struct MutantRow {
    id: String,
    display_id: String,
    outcome: String,
    killed_by: Option<String>,
    reused: bool,
    source_run_id: Option<String>,
}

impl MutantRow {
    fn label(&self) -> &str {
        if !self.display_id.is_empty() {
            &self.display_id
        } else if self.id.is_empty() {
            "a mutant the recording does not name"
        } else {
            &self.id
        }
    }

    /// A mutation nothing noticed, whether a test reached it and stayed silent or none reached it at all.
    fn survived(&self) -> bool {
        self.outcome == SURVIVED || self.outcome == UNREACHED
    }

    fn answers_to(&self, subject: &str) -> bool {
        subject == self.display_id || subject == self.id
    }
}

#[derive(Debug)]
struct FindingRow {
    kind: String,
    subject: String,
}

/// Whether any layer removed a target that then killed the mutation it removed.
/// The targets the recording says noticed nothing, held to the findings that name them.
///
/// Re-derived from the executions alone. A target is asked about a mutation
/// only after every target before it in the route survived it, so every
/// execution the recording holds is one where that target had its chance.
fn hollow(recording: &Recording<'_>, recorded: Option<&str>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Hollow);
    let Some(recorded) = recorded else {
        notes.unaudited(
            "executions",
            "the run kept no recording of what it ran, so which targets were put to a \
             mutation and noticed none cannot be re-derived"
                .to_owned(),
        );
        return;
    };
    let routing = crate::route::read(recorded);
    if routing.execs.is_empty() {
        notes.unaudited(
            "executions",
            "the recording holds no mutation execution, so no target was put to anything \
             this audit could hold it to"
                .to_owned(),
        );
        return;
    }
    let mut asked: BTreeMap<&str, (u64, bool)> = BTreeMap::new();
    for exec in &routing.execs {
        let held = asked.entry(exec.target.as_str()).or_insert((0, false));
        held.0 = held.0.saturating_add(1);
        if matches!(exec.outcome.as_str(), "killed" | "timed_out") {
            held.1 = true;
        }
    }
    let owed: BTreeSet<&str> = asked
        .iter()
        .filter(|(_, (count, noticed))| *count > 0 && !*noticed)
        .map(|(target, _)| *target)
        .collect();
    let named: BTreeSet<&str> = recording
        .document
        .get("findings")
        .and_then(serde_json::Value::as_array)
        .map(|findings| {
            findings
                .iter()
                .filter(|one| {
                    one.get("kind").and_then(serde_json::Value::as_str) == Some("hollow-target")
                })
                .filter_map(|one| one.get("subject").and_then(serde_json::Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    for target in owed.difference(&named) {
        let (count, _) = asked.get(target).copied().unwrap_or((0, false));
        notes.violated(
            target,
            format!(
                "{target} was put to {count} mutation(s) and answered none of them with a \
                 detection, and the report names no hollow-target finding about it"
            ),
        );
    }
    for target in named.difference(&owed) {
        notes.violated(
            target,
            format!(
                "the report calls {target} hollow, and the recording has it noticing \
                 something or being asked nothing"
            ),
        );
    }
}

/// The faults a seam recording licensed, re-derived here and held to what the run says became of them.
///
/// The catalogue is minted again from the exchanges alone, by the rules and
/// the identity recipe written out in `crate::wire`, so a fault this audit
/// does not derive is one the run invented and a fault it derives that the
/// run never put is a question the report is quiet about.
fn wire(recording: &Recording<'_>, recorded: Option<&str>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Wire);
    let Some(recorded) = recorded else {
        return;
    };
    let watched = crate::wire::read(recorded);
    if watched.exchanges.is_empty() && watched.execs.is_empty() {
        return;
    }
    let mut owed: BTreeMap<String, String> = BTreeMap::new();
    for exchange in &watched.exchanges {
        for (id, rule) in crate::wire::licensed(exchange) {
            owed.insert(id, rule);
        }
    }
    let put: BTreeMap<&str, &crate::wire::Exec> = watched
        .execs
        .iter()
        .map(|one| (one.fault.as_str(), one))
        .collect();
    if put.is_empty() {
        notes.unaudited(
            "faults",
            format!(
                "{} exchange(s) went past a seam and licensed {} question(s), and the \
                 recording holds none of them being put, so what the suite would have \
                 done with them cannot be re-derived",
                watched.exchanges.len(),
                owed.len()
            ),
        );
        return;
    }
    for (id, rule) in &owed {
        if !put.contains_key(id.as_str()) {
            notes.violated(
                id,
                format!(
                    "the exchanges the recording holds license {rule} here, and the run \
                     records neither putting it nor why it did not"
                ),
            );
        }
    }
    for (id, exec) in &put {
        if !owed.contains_key(*id) {
            notes.violated(
                id,
                format!(
                    "the run put {} on the {} seam at exchange {}, and no exchange this \
                     audit re-derives from the recording licenses it",
                    exec.rule, exec.capability, exec.seq
                ),
            );
        }
    }
    gaps(recording, &put, &mut notes);
}

/// The questions the recording says nothing noticed, held to the findings that name them.
fn gaps(
    recording: &Recording<'_>,
    put: &BTreeMap<&str, &crate::wire::Exec>,
    notes: &mut Notes<'_>,
) {
    let named: BTreeSet<&str> = recording
        .findings
        .iter()
        .filter(|one| one.kind == "wire-unnoticed")
        .map(|one| one.subject.as_str())
        .collect();
    for (id, exec) in put {
        let unnoticed = exec.decision == "unnoticed";
        if unnoticed && !named.contains(*id) {
            notes.violated(
                id,
                format!(
                    "the recording has nothing noticing {} on the {} seam, and the report \
                     names no wire-unnoticed finding about it",
                    exec.rule, exec.capability
                ),
            );
        }
        if !unnoticed && named.contains(*id) {
            notes.violated(
                id,
                format!(
                    "the report calls this a gap, and the recording has it decided by {}",
                    exec.decision
                ),
            );
        }
    }
    for id in named {
        if !put.contains_key(id) {
            notes.violated(
                id,
                "the report names a question nothing noticed, and the recording has no \
                 run putting it"
                    .to_owned(),
            );
        }
    }
}

fn proofs(recording: &Recording<'_>, recorded: Option<&str>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Proofs);
    let Some(recorded) = recorded else {
        notes.unaudited(
            "route",
            "the run kept no recording of how it routed, so which target each proof removed \
             cannot be re-derived"
                .to_owned(),
        );
        return;
    };
    let routing = crate::route::read(recorded);
    let removed: BTreeMap<String, BTreeMap<String, String>> = routing
        .routes
        .iter()
        .map(|route| {
            let discharged = route
                .discharged
                .iter()
                .map(|one| (one.target.clone(), one.proof.clone()))
                .collect();
            (route.mutant.clone(), discharged)
        })
        .collect();
    let executed: BTreeSet<String> = routing
        .execs
        .iter()
        .map(|exec| exec.mutant.clone())
        .collect();
    let ran: Vec<(String, String, String)> = routing
        .execs
        .iter()
        .map(|exec| {
            (
                exec.mutant.clone(),
                exec.target.clone(),
                exec.outcome.clone(),
            )
        })
        .collect();
    if removed.is_empty() && ran.is_empty() {
        notes.unaudited(
            "route",
            "the recording holds no routing decision and no mutation execution, so there is \
             nothing to hold a layer to"
                .to_owned(),
        );
        return;
    }
    let known: BTreeSet<&str> = recording
        .targets
        .iter()
        .map(|target| target.name.as_str())
        .collect();
    discharges(&removed, &ran, &mut notes);
    kept(&routing.routes, &ran, &mut notes);
    reach(&routing.routes, &known, &executed, &mut notes);
    believed(&routing.routes, &mut notes);
}

/// Reuse, re-derived: a route names the run whose answer it took, or why it took none, and never both.
fn believed(routes: &[crate::route::Route], notes: &mut Notes<'_>) {
    for route in routes {
        if let (Some(run), Some(refusal)) = (route.reused.as_ref(), route.refused.as_ref()) {
            notes.violated(
                &route.mutant,
                format!(
                    "the route says the answer was read back from {run} and that it was \
                     refused as {refusal}; one of those is not what happened, and a \
                     recording that says both cannot be held to either"
                ),
            );
        }
    }
}

/// Every proof that removed a target, against the kills the recording holds: a layer that drops a target which then finds a defect is unsound.
fn discharges(
    removed: &BTreeMap<String, BTreeMap<String, String>>,
    ran: &[(String, String, String)],
    notes: &mut Notes<'_>,
) {
    for (mutant, target, outcome) in ran {
        if outcome != KILLED && outcome != TIMED_OUT {
            continue;
        }
        let Some(proof) = removed.get(mutant).and_then(|one| one.get(target)) else {
            continue;
        };
        notes.violated(
            mutant,
            format!(
                "{proof} removed {target} from what could notice this mutation, and {target} \
                 then {outcome} it; a layer that drops a target which finds a defect is unsound"
            ),
        );
    }
}

/// Every kill, against the route that decided which targets would be asked: a layer that drops a target which then finds a defect is unsound, however it dropped it.
fn kept(routes: &[crate::route::Route], ran: &[(String, String, String)], notes: &mut Notes<'_>) {
    for (mutant, target, outcome) in ran {
        if outcome != KILLED && outcome != TIMED_OUT {
            continue;
        }
        let Some(route) = routes
            .iter()
            .find(|route| &route.mutant == mutant && route.reused.is_none())
        else {
            continue;
        };
        if route.reaching.is_empty() || route.reaching.iter().any(|one| one == target) {
            continue;
        }
        if route.discharges(target) {
            continue;
        }
        notes.violated(
            mutant,
            format!(
                "the route did not keep {target} for this mutation and the recording then \
                 shows {target} {outcome} it; a target the measurement placed elsewhere is \
                 a target the reach layer removed, and a layer that removes one which \
                 finds a defect is unsound"
            ),
        );
    }
}

/// The reach layer, re-derived from what the route named rather than confirmed from what it decided.
fn reach(
    routes: &[crate::route::Route],
    known: &BTreeSet<&str>,
    executed: &BTreeSet<String>,
    notes: &mut Notes<'_>,
) {
    for route in routes {
        if route.reused.is_some() {
            continue;
        }
        let ran = executed.contains(&route.mutant);
        if route.granularity == UNREACHED {
            if ran {
                notes.violated(
                    &route.mutant,
                    "the route says no measured target reaches this mutation and the \
                     recording then runs one against it; a claim about the code that its \
                     own run contradicts is not a claim"
                        .to_owned(),
                );
            }
            if route.considered.is_empty() {
                notes.violated(
                    &route.mutant,
                    "the route removed every execution and named no target it removed \
                     them from; nothing reaches a place only if somebody was in a \
                     position to notice and did not, and this says nobody was"
                        .to_owned(),
                );
            }
        }
        if route.granularity == EVERYTHING && !ran {
            notes.violated(
                &route.mutant,
                "the route says the measurement does not carry which targets reach this \
                 mutation, and the recording runs nothing against it; a premise that \
                 fails has to end in more work rather than in less"
                    .to_owned(),
            );
        }
        considered(route, known, notes);
    }
}

/// Every target a route says was asked and did not reach, against the run that says which targets there were.
fn considered(route: &crate::route::Route, known: &BTreeSet<&str>, notes: &mut Notes<'_>) {
    for target in &route.considered {
        if !known.contains(target.as_str()) {
            notes.violated(
                &route.mutant,
                format!(
                    "the route says {target} was measured and did not reach this \
                     mutation, and the run reports no such target; a layer held to \
                     targets that are not there is held to nothing"
                ),
            );
        }
        if route.reaching.contains(target) {
            notes.violated(
                &route.mutant,
                format!(
                    "the route both keeps {target} for this mutation and says it did not \
                     reach it; one route cannot answer a question two ways"
                ),
            );
        }
        if route.discharges(target) {
            notes.violated(
                &route.mutant,
                format!(
                    "the route says {target} did not reach this mutation and also names \
                     a proof that removed it; a target that reaches nothing needs no \
                     proof, and a proof that removed it says it did reach"
                ),
            );
        }
    }
}

#[derive(Debug)]
struct Recording<'a> {
    document: &'a serde_json::Value,
    run_id: String,
    targets: Vec<TargetRow>,
    mutants: Vec<MutantRow>,
    findings: Vec<FindingRow>,
    shard: Option<String>,
}

impl<'a> Recording<'a> {
    fn of(document: &'a serde_json::Value) -> Self {
        Self {
            document,
            run_id: field(document, "run_id").unwrap_or_default(),
            targets: rows(document, "targets")
                .iter()
                .map(|row| TargetRow {
                    id: field(row, "id").unwrap_or_default(),
                    name: field(row, "name").unwrap_or_default(),
                    status: field(row, "status").unwrap_or_default(),
                })
                .collect(),
            mutants: rows(document, "mutants")
                .iter()
                .map(|row| MutantRow {
                    id: field(row, "id").unwrap_or_default(),
                    display_id: field(row, "display_id").unwrap_or_default(),
                    outcome: field(row, "outcome").unwrap_or_default(),
                    killed_by: field(row, "killed_by"),
                    reused: row
                        .get("reused")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or_default(),
                    source_run_id: field(row, "source_run_id"),
                })
                .collect(),
            findings: rows(document, "findings")
                .iter()
                .map(|row| FindingRow {
                    kind: field(row, "kind").unwrap_or_default(),
                    subject: field(row, "subject").unwrap_or_default(),
                })
                .collect(),
            shard: document
                .get("scope")
                .and_then(|scope| field(scope, "shard")),
        }
    }

    fn dispositions(&self, outcome: &str) -> usize {
        self.mutants
            .iter()
            .filter(|mutant| mutant.outcome == outcome)
            .count()
    }
}

/// Whether the target columns say what the target records say.
fn target_columns(recording: &Recording<'_>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Accounting);
    notes.tally(Column {
        subject: "accounting.targets.selected",
        recorded: column(recording.document, "targets", "selected"),
        derived: size(recording.targets.len()),
        records: "the target records it carries",
    });
    for status in [PASSED, "failed", "skipped", "missing"] {
        let recorded = recording
            .targets
            .iter()
            .filter(|target| target.status == status)
            .count();
        notes.tally(Column {
            subject: &format!("accounting.targets.{status}"),
            recorded: column(recording.document, "targets", status),
            derived: size(recorded),
            records: "the target records carrying that status",
        });
    }
}

/// Whether the mutant columns say what the mutant records say.
fn mutant_columns(recording: &Recording<'_>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Accounting);
    notes.tally(Column {
        subject: "accounting.mutants.cataloged",
        recorded: column(recording.document, "mutants", "cataloged"),
        derived: size(recording.mutants.len()),
        records: "the mutant records it carries",
    });
    for (name, outcome) in [
        ("rejected", REJECTED),
        (KILLED, KILLED),
        (SURVIVED, SURVIVED),
        (TIMED_OUT, TIMED_OUT),
        (UNREACHED, UNREACHED),
        (EQUIVALENT, EQUIVALENT),
    ] {
        notes.tally(Column {
            subject: &format!("accounting.mutants.{name}"),
            recorded: column(recording.document, "mutants", name),
            derived: size(recording.dispositions(outcome)),
            records: "the mutant records carrying that disposition",
        });
    }
    let ran = recording
        .mutants
        .len()
        .saturating_sub(recording.dispositions(REJECTED))
        .saturating_sub(recording.dispositions(UNREACHED))
        .saturating_sub(recording.dispositions(EQUIVALENT));
    notes.tally(Column {
        subject: "accounting.mutants.executed",
        recorded: column(recording.document, "mutants", "executed"),
        derived: size(ran),
        records: "the mutant records the compiler accepted and something reached",
    });
    for (name, outcome) in [("reused_killed", KILLED), ("reused_survived", SURVIVED)] {
        let recorded = recording
            .mutants
            .iter()
            .filter(|mutant| mutant.outcome == outcome && mutant.reused)
            .count();
        notes.tally(Column {
            subject: &format!("accounting.mutants.{name}"),
            recorded: column(recording.document, "mutants", name),
            derived: size(recorded),
            records: "the mutant records of that disposition read back from an earlier run",
        });
    }
    if column(recording.document, "mutants", "accepted").is_some_and(|count| count > 0) {
        notes.unaudited(
            "accounting.mutants.accepted",
            "the recording counts the survivors a reviewer accepted without naming any of them, \
             so the column has no records to be held to"
                .to_owned(),
        );
    }
}

/// Whether the columns add up the way the assurance contract says they do, which a recording carrying no records at all must still satisfy.
fn equations(recording: &Recording<'_>, audit: &mut Audit) {
    let targets = |name: &str| column(recording.document, "targets", name);
    let mutants = |name: &str| column(recording.document, "mutants", name);
    let mut notes = Notes::on(audit, Layer::Accounting);
    notes.holds(Equation {
        subject: "accounting.targets",
        relation: Relation::Exactly,
        sides: (
            sum(&[
                targets(PASSED),
                targets("failed"),
                targets("skipped"),
                targets("missing"),
            ]),
            targets("selected"),
        ),
        because: "every selected target has exactly one terminal state",
    });
    notes.holds(Equation {
        subject: "accounting.mutants",
        relation: Relation::Exactly,
        sides: (
            sum(&[
                mutants("rejected"),
                mutants("executed"),
                mutants(UNREACHED),
                mutants(EQUIVALENT),
            ]),
            mutants("cataloged"),
        ),
        because: "every cataloged mutant was refused by the compiler, reached by nothing, \
                  rendered identically to what it mutates, or executed",
    });
    notes.holds(Equation {
        subject: "accounting.mutants.executed",
        relation: Relation::AtMost,
        sides: (
            sum(&[mutants(KILLED), mutants(SURVIVED), mutants(TIMED_OUT)]),
            mutants("executed"),
        ),
        because: "a mutation a test noticed, one nothing noticed, and one that ran out of time \
                  were each executed",
    });
    notes.holds(Equation {
        subject: "accounting.mutants.accepted",
        relation: Relation::AtMost,
        sides: (
            mutants("accepted"),
            sum(&[mutants(SURVIVED), mutants(UNREACHED), mutants(EQUIVALENT)]),
        ),
        because: "an acceptance is a reviewer's answer to a mutation nothing noticed, which is \
                  one nothing reached as much as one every reaching test passed",
    });
    for (name, whole) in [("reused_killed", KILLED), ("reused_survived", SURVIVED)] {
        notes.holds(Equation {
            subject: &format!("accounting.mutants.{name}"),
            relation: Relation::AtMost,
            sides: (mutants(name), mutants(whole)),
            because: "a reused disposition is one of that column, not an addition to it",
        });
    }
}

#[derive(Debug, Clone, Copy)]
enum Relation {
    Exactly,
    AtMost,
}

impl Relation {
    const fn word(self) -> &'static str {
        match self {
            Self::Exactly => "exactly",
            Self::AtMost => "at most",
        }
    }

    const fn holds(self, left: u64, right: u64) -> bool {
        match self {
            Self::Exactly => left == right,
            Self::AtMost => left <= right,
        }
    }
}

/// Whether the verdict is one the accounting and the findings support.
fn verdict(recording: &Recording<'_>, audit: &mut Audit) {
    let Some(concluded) = field(recording.document, "verdict") else {
        Notes::on(audit, Layer::Accounting).unaudited(
            "verdict",
            "the recording does not say what the run concluded, so there is nothing to hold its \
             accounting to"
                .to_owned(),
        );
        return;
    };
    match concluded.as_str() {
        "ASSURED" | "CHANGE_ASSURED" | "SCOPE_ASSURED" => assurance(recording, audit, &concluded),
        "DEFECT" => defect(recording, audit),
        _ => {}
    }
}

fn assurance(recording: &Recording<'_>, audit: &mut Audit, concluded: &str) {
    scope(recording, audit, concluded);
    let mut notes = Notes::on(audit, Layer::Accounting);
    let unsupported = |notes: &mut Notes<'_>, because: &str| {
        notes.violated(
            "verdict",
            format!("the recording concludes {concluded} and {because}"),
        );
    };
    if !recording.findings.is_empty() {
        unsupported(
            &mut notes,
            &format!(
                "carries {} findings; an assurance is the claim that nothing was found",
                recording.findings.len()
            ),
        );
    }
    for (name, because) in [
        ("failed", "a failing test is not an assurance"),
        (
            "missing",
            "a target that never ran cannot support a claim about what it would have said",
        ),
    ] {
        if let Some(count) = column(recording.document, "targets", name)
            && count > 0
        {
            unsupported(
                &mut notes,
                &format!("counts {count} {name} targets; {because}"),
            );
        }
    }
    for (group, name, because) in [
        (
            "targets",
            PASSED,
            "nothing was observed, so nothing is assured",
        ),
        (
            "mutants",
            "executed",
            "nothing was asked of the tests, so nothing about them is assured",
        ),
    ] {
        if column(recording.document, group, name) == Some(0) {
            unsupported(&mut notes, &format!("counts no {name} {group}; {because}"));
        }
    }
}

/// An assurance reaches exactly as far as the run looked, and the recording says how far that was.
fn scope(recording: &Recording<'_>, audit: &mut Audit, concluded: &str) {
    let mut notes = Notes::on(audit, Layer::Accounting);
    let Some(kind) = field(recording.document, "run_kind") else {
        notes.unaudited(
            "verdict",
            "the recording does not say how much of the workspace the run looked at, so the \
             reach of its assurance cannot be re-decided"
                .to_owned(),
        );
        return;
    };
    let reached = match kind.as_str() {
        "full" => "ASSURED",
        "changed" => "CHANGE_ASSURED",
        "scoped" => "SCOPE_ASSURED",
        _ => {
            notes.unaudited(
                "verdict",
                format!(
                    "the recording names the scope {kind:?}, which this audit knows no assurance \
                     to hold it to"
                ),
            );
            return;
        }
    };
    if reached != concluded {
        notes.violated(
            "verdict",
            format!(
                "the recording concludes {concluded} over a {kind} run, which assures only what \
                 it looked at, and that is {reached}"
            ),
        );
    }
}

fn defect(recording: &Recording<'_>, audit: &mut Audit) {
    const FAULTS: [&str; 3] = ["build-failure", "failing-test", "flaky-test"];
    if !recording
        .findings
        .iter()
        .any(|finding| FAULTS.contains(&finding.kind.as_str()))
    {
        Notes::on(audit, Layer::Accounting).violated(
            "verdict",
            "the recording concludes DEFECT and names nothing wrong with the code under test; a \
             surviving mutation is a gap in the tests, and a fault a reader cannot see named is \
             not one they can act on"
                .to_owned(),
        );
    }
}

/// Whether every kill names a target this run itself saw pass on the original tree.
fn killers(recording: &Recording<'_>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Killers);
    for mutant in recording
        .mutants
        .iter()
        .filter(|mutant| mutant.outcome == KILLED)
    {
        let Some(killer) = mutant.killed_by.as_deref() else {
            notes.violated(
                mutant.label(),
                "the recording says a test noticed this mutation and does not say which; a kill \
                 nobody can name is not a kill a reader can check"
                    .to_owned(),
            );
            continue;
        };
        let Some(target) = recording
            .targets
            .iter()
            .find(|target| target.name == killer || target.id == killer)
        else {
            notes.violated(
                mutant.label(),
                format!(
                    "the kill is attributed to {killer:?}, which is not among the targets this \
                     run recorded"
                ),
            );
            continue;
        };
        if target.status != PASSED {
            notes.violated(
                mutant.label(),
                format!(
                    "the kill is attributed to {killer:?}, which this run recorded as {} on the \
                     original tree; a target it never saw pass there cannot tell the two \
                     programs apart",
                    target.status
                ),
            );
        }
    }
}

/// Whether the mutations nothing noticed and the findings that raise them are the same set.
fn findings(recording: &Recording<'_>, audit: &mut Audit) {
    let raised: BTreeSet<&str> = recording
        .findings
        .iter()
        .filter(|finding| finding.kind == SURVIVING_MUTANT)
        .map(|finding| finding.subject.as_str())
        .collect();
    let accepted = column(recording.document, "mutants", "accepted");
    let mut notes = Notes::on(audit, Layer::Findings);
    for mutant in recording.mutants.iter().filter(|mutant| mutant.survived()) {
        if raised.iter().any(|subject| mutant.answers_to(subject)) {
            continue;
        }
        match accepted {
            Some(0) => notes.violated(
                mutant.label(),
                "nothing noticed this mutation and no finding names it, while the recording \
                 counts no acceptance that would explain the silence; a survivor a report does \
                 not raise is a gap in the tests the report hides"
                    .to_owned(),
            ),
            Some(_) => notes.unaudited(
                mutant.label(),
                "nothing noticed this mutation and no finding names it; the recording counts the \
                 acceptances a reviewer made without naming any of them, so whether this is one \
                 of them cannot be re-decided"
                    .to_owned(),
            ),
            None => notes.unaudited(
                mutant.label(),
                "nothing noticed this mutation and no finding names it; the recording omits the \
                 acceptance column, so whether a reviewer accepted it cannot be re-decided"
                    .to_owned(),
            ),
        }
    }
    for finding in recording
        .findings
        .iter()
        .filter(|finding| finding.kind == SURVIVING_MUTANT)
    {
        if !recording
            .mutants
            .iter()
            .any(|mutant| mutant.survived() && mutant.answers_to(&finding.subject))
        {
            notes.violated(
                &finding.subject,
                "the finding names no mutant this run recorded as surviving; a finding a reader \
                 cannot trace to the evidence under it is a claim without one"
                    .to_owned(),
            );
        }
    }
}

/// Whether a finding that calls an acceptance unmatched is supported by the complete catalog.
fn acceptances(recording: &Recording<'_>, audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Acceptances);
    for finding in recording
        .findings
        .iter()
        .filter(|finding| finding.kind == UNMATCHED_ACCEPTANCE)
    {
        if recording.shard.is_some() {
            notes.unaudited(
                &finding.subject,
                "this shard does not carry the complete catalog, so whether the acceptance \
                 resolves uniquely is decided only after the shards are merged"
                    .to_owned(),
            );
            continue;
        }
        let subject = finding.subject.as_str();
        let valid = (4..=64).contains(&subject.len())
            && subject
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !valid {
            continue;
        }
        let matches: Vec<&MutantRow> = recording
            .mutants
            .iter()
            .filter(|mutant| mutant.id.starts_with(subject))
            .collect();
        if let [mutant] = matches.as_slice() {
            notes.violated(
                subject,
                format!(
                    "the finding calls this acceptance unmatched, but it uniquely resolves to \
                     {}; an unmatched acceptance must suppress nothing",
                    mutant.id
                ),
            );
        }
    }
}

/// Whether every disposition read back from an earlier run names one a reader could go and read.
fn reuse(recording: &Recording<'_>, audit: &mut Audit) {
    let mut read_back = 0_usize;
    let mut notes = Notes::on(audit, Layer::Reuse);
    for mutant in &recording.mutants {
        match (mutant.reused, mutant.source_run_id.as_deref()) {
            (true, None) => notes.violated(
                mutant.label(),
                "the disposition was read back from an earlier run and does not name it; a \
                 verdict a reader cannot trace back is one taken on trust"
                    .to_owned(),
            ),
            (true, Some(run)) if run == recording.run_id => notes.violated(
                mutant.label(),
                "the disposition names this run itself as the run it was read back from; a run \
                 cannot have read its own answer back"
                    .to_owned(),
            ),
            (true, Some(_)) => read_back = read_back.saturating_add(1),
            (false, Some(run)) => notes.violated(
                mutant.label(),
                format!(
                    "this run established the disposition itself and also names {run:?} as the \
                     run it came from; one of the two is wrong and a reader cannot tell which"
                ),
            ),
            (false, None) => {}
        }
    }
    if read_back > 0 {
        notes.unaudited(
            "provenance",
            format!(
                "{read_back} dispositions were read back from an earlier run; whether the \
                 recorded target is still routed to the mutation under the same behaviour key is \
                 a fact this report does not carry"
            ),
        );
    }
}

/// A string the recording says something in. Whitespace is nothing to say, and reads here as the absent value it is.
fn field(value: &serde_json::Value, key: &str) -> Option<String> {
    let said = value.get(key)?.as_str()?.trim();
    (!said.is_empty()).then(|| said.to_owned())
}

fn rows<'a>(document: &'a serde_json::Value, key: &str) -> &'a [serde_json::Value] {
    document
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn column(document: &serde_json::Value, group: &str, name: &str) -> Option<u64> {
    document.get("accounting")?.get(group)?.get(name)?.as_u64()
}

fn sum(counts: &[Option<u64>]) -> Option<u64> {
    counts
        .iter()
        .try_fold(0_u64, |total, count| Some(total.saturating_add((*count)?)))
}

fn size(count: usize) -> u64 {
    u64::try_from(count).unwrap_or(u64::MAX)
}

fn plural(count: usize, thing: &str) -> String {
    if count == 1 {
        format!("{count} {thing}")
    } else {
        format!("{count} {thing}s")
    }
}
