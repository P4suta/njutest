// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What each control started under a knob established, re-derived from the engine's perturbed-control records and held to the report's knob records, findings, and limitations.

use std::collections::{BTreeMap, BTreeSet};

use crate::drift::Touched;
use crate::knobs::{Derived, Knob, Perturbations, Perturbed};

use super::{Audit, Engine, Layer, Notes, Recording, field, named, rows};

/// The finding a report raises about a target a knob broke.
const ENVIRONMENT_DEPENDENT: &str = "environment-dependent";

/// The finding a report raises about a target whose reach a knob moved.
const ENVIRONMENT_DEPENDENT_REACH: &str = "environment-dependent-reach";

/// The limitation a report states about a knob it was asked for and did not put.
const KNOB_NOT_PUT: &str = "knob-not-put";

/// The limitation a report states about the controls under a knob that compared nothing.
const KNOB_NOT_COMPARED: &str = "knob-not-compared";

/// One knob record of the report, as it reads.
struct Row<'a> {
    target: String,
    knob: String,
    state: String,
    standing: &'a serde_json::Value,
}

/// Which target was put under which knob.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Put {
    target: String,
    knob: Knob,
}

/// What each control under a knob established, re-derived from the engine's perturbed-control records against its baseline touch record, and held to what the report says of each.
pub(super) fn audited(recording: &Recording<'_>, engines: &[Engine], audit: &mut Audit) {
    let mut notes = Notes::on(audit, Layer::Knobs);
    let recorded: Vec<Row<'_>> = rows(recording.document, "knobs")
        .iter()
        .map(|row| {
            let standing = row.get("standing").unwrap_or(&serde_json::Value::Null);
            Row {
                target: field(row, "target").unwrap_or_default(),
                knob: field(row, "knob").unwrap_or_default(),
                state: field(standing, "state").unwrap_or_default(),
                standing,
            }
        })
        .collect();
    let (touched, perturbations) = match engines {
        [] => {
            if !recorded.is_empty() {
                notes.unaudited(
                    "knobs",
                    "the run kept no engine recording, so what each control under a knob \
                     established cannot be re-derived"
                        .to_owned(),
                );
            }
            return;
        }
        [Engine { touched, perturbed }] => (touched, perturbed),
        several => {
            notes.unaudited(
                "knobs",
                format!(
                    "the recording holds {} engine recordings and the report is one build's, so \
                     which of them its knob records answer to cannot be told from the recording",
                    several.len()
                ),
            );
            return;
        }
    };
    if perturbations.unreadable > 0 {
        notes.unaudited(
            "knobs",
            format!(
                "{} perturbed-control record(s) do not say what the control was started with, how \
                 it ended, or what became of its reach, so what they would have shown cannot be \
                 counted as agreement",
                perturbations.unreadable
            ),
        );
    }
    let derived = established(perturbations, touched, &mut notes);
    let not_put = held_to_records(&recorded, &derived, &mut notes);
    if recording.shard.is_some() {
        return;
    }
    held_to_findings(recording, &derived, &mut notes);
    held_to_limitations(recording, &derived, &not_put, &mut notes);
}

/// What each target put under each knob established, from the one control the engine started it with; a control no knob puts, and a knob put twice on one target, are violations of their own.
fn established(
    perturbations: &Perturbations,
    touched: &Touched,
    notes: &mut Notes<'_>,
) -> BTreeMap<Put, Derived> {
    let mut controls: BTreeMap<Put, Vec<&Perturbed>> = BTreeMap::new();
    for control in &perturbations.controls {
        match control.started.role() {
            crate::knobs::Role::Knob(knob) => controls
                .entry(Put {
                    target: control.target.clone(),
                    knob,
                })
                .or_default()
                .push(control),
            crate::knobs::Role::Delayed | crate::knobs::Role::Undelayed => {}
            crate::knobs::Role::Unknown => notes.violated(
                &control.target,
                format!(
                    "the engine started a control of {} with {}, which is not what any knob puts",
                    control.target,
                    control.started.said()
                ),
            ),
        }
    }
    let mut derived = BTreeMap::new();
    for (put, ran) in controls {
        let [control] = ran.as_slice() else {
            notes.violated(
                &put.target,
                format!(
                    "the engine started {} controls of {} under {}, where a knob is put once on a \
                     target",
                    ran.len(),
                    put.target,
                    put.knob.name()
                ),
            );
            continue;
        };
        derived.insert(put, crate::knobs::derived(control, touched));
    }
    derived
}

/// Each knob record against the one control it is the record of, and each control against the record it is owed; the targets the report says a knob was not put on, for the limitation that owes them.
fn held_to_records(
    recorded: &[Row<'_>],
    derived: &BTreeMap<Put, Derived>,
    notes: &mut Notes<'_>,
) -> BTreeSet<String> {
    let mut seen: BTreeSet<Put> = BTreeSet::new();
    let mut not_put = BTreeSet::new();
    for row in recorded {
        let Some(knob) = Knob::parse(&row.knob) else {
            notes.violated(
                &row.target,
                format!("{:?} is not a knob a record can name", row.knob),
            );
            continue;
        };
        let key = Put {
            target: row.target.clone(),
            knob,
        };
        if !seen.insert(key.clone()) {
            notes.violated(
                &row.target,
                format!(
                    "the report records {} twice for {}, where a knob is put once on a target",
                    knob.name(),
                    row.target
                ),
            );
            continue;
        }
        match (row.state.as_str(), derived.get(&key)) {
            ("not-put", found) => {
                not_put.insert(row.target.clone());
                if let Some(found) = found {
                    notes.violated(
                        &row.target,
                        format!(
                            "the report says {} was not put on {}, and the engine recorded a \
                             control of it under that knob, which {}",
                            knob.name(),
                            row.target,
                            found.said()
                        ),
                    );
                }
            }
            (state, None) => notes.violated(
                &row.target,
                format!(
                    "the report records {} under {} as {state}, and the engine recorded no one \
                     control of it under that knob for that to be the record of",
                    row.target,
                    knob.name()
                ),
            ),
            (_, Some(found)) => {
                if !agrees(found, row) {
                    notes.violated(
                        &row.target,
                        format!(
                            "the engine's perturbed-control record says {} under {} {}, and the \
                             report records {}",
                            row.target,
                            knob.name(),
                            found.said(),
                            row.standing
                        ),
                    );
                }
            }
        }
    }
    for put in derived.keys().filter(|put| !seen.contains(put)) {
        notes.violated(
            &put.target,
            format!(
                "the engine started a control of {} under {} and the report records nothing \
                 about it",
                put.target,
                put.knob.name()
            ),
        );
    }
    not_put
}

/// Whether the report's record says what the re-derivation found: the same state, the same failing tests, the same movement in every union, a reason that holds, or the same reason for settling nothing.
fn agrees(found: &Derived, row: &Row<'_>) -> bool {
    let standing = row.standing;
    if row.state != found.state() {
        return false;
    }
    match found {
        Derived::Stable | Derived::Passed => true,
        Derived::Broke { failed } => {
            crate::knobs::names(standing.get("failed")).as_ref() == Some(failed)
        }
        Derived::Moved { reach } => {
            crate::knobs::moves(standing.get("reach")).as_ref() == Some(reach.as_ref())
        }
        Derived::Uncompared { because } => {
            field(standing, "why").is_some_and(|why| because.iter().any(|one| one.name() == why))
        }
        Derived::Unsettled { why } => field(standing, "why").as_deref() == Some(why.name()),
    }
}

fn held_to_findings(
    recording: &Recording<'_>,
    derived: &BTreeMap<Put, Derived>,
    notes: &mut Notes<'_>,
) {
    for (kind, owing) in [
        (ENVIRONMENT_DEPENDENT, "broke"),
        (ENVIRONMENT_DEPENDENT_REACH, "moved"),
    ] {
        let owed: BTreeSet<&str> = derived
            .iter()
            .filter(|(_, found)| found.state() == owing)
            .map(|(put, _)| put.target.as_str())
            .collect();
        let named: BTreeSet<&str> = recording
            .findings
            .iter()
            .filter(|finding| finding.kind == kind)
            .map(|finding| finding.subject.as_str())
            .collect();
        for target in owed.difference(&named) {
            notes.violated(
                target,
                format!(
                    "a control of {target} under a knob {owing} it, and the report raises no {kind} \
                     finding about it"
                ),
            );
        }
        for target in named.difference(&owed) {
            notes.violated(
                target,
                format!(
                    "the report raises {kind} about {target}, and no control the engine recorded \
                     under a knob {owing} it"
                ),
            );
        }
    }
}

fn held_to_limitations(
    recording: &Recording<'_>,
    derived: &BTreeMap<Put, Derived>,
    not_put: &BTreeSet<String>,
    notes: &mut Notes<'_>,
) {
    let uncompared: BTreeSet<&str> = derived
        .iter()
        .filter(|(_, found)| {
            matches!(
                found,
                Derived::Uncompared { .. } | Derived::Unsettled { .. }
            )
        })
        .map(|(put, _)| put.target.as_str())
        .collect();
    let unput: BTreeSet<&str> = not_put.iter().map(String::as_str).collect();
    for (limitation, owed, why) in [
        (
            KNOB_NOT_COMPARED,
            &uncompared,
            "a control of it under a knob established nothing to compare",
        ),
        (
            KNOB_NOT_PUT,
            &unput,
            "the report says a knob asked for was not put on it",
        ),
    ] {
        let stated: BTreeSet<&str> = rows(recording.document, "limitations")
            .iter()
            .filter(|row| field(row, "name").as_deref() == Some(limitation))
            .filter_map(|row| row.get("detail").and_then(serde_json::Value::as_str))
            .flat_map(named)
            .collect();
        for target in owed.difference(&stated) {
            notes.violated(
                target,
                format!("{why}, and no {limitation} limitation names {target}"),
            );
        }
        for target in stated.difference(owed) {
            notes.violated(
                target,
                format!(
                    "a {limitation} limitation names {target}, and the recording does not show \
                     that {why}"
                ),
            );
        }
    }
}
