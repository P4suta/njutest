// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run held to the ledger of survivors somebody accepted.

use std::collections::BTreeSet;

use super::{Audit, Decided, Layer, MET, Notes, Report, SURVIVING_MUTANT, UNREACHED_MUTANT};

pub(super) fn ledger(report: &Report, ledger: Option<&toml::Table>, audit: &mut Audit) -> Decided {
    let mut notes = Notes::on(audit, Layer::Ledger);
    let Some(ledger) = ledger else {
        notes.unaudited(
            "ledger",
            "no ledger was given, so whether every survivor is one somebody accepted cannot be \
             re-decided"
                .to_owned(),
        );
        return notes.looked();
    };
    let entries = accepted(ledger);
    for finding in &report.findings {
        if finding.kind == SURVIVING_MUTANT || finding.kind == UNREACHED_MUTANT {
            let subject = finding.mutant.as_deref().unwrap_or(finding.kind.as_str());
            notes.violated(
                subject,
                "no test noticed this mutation and the ledger does not accept it; a survivor \
                 is either killed or accepted with a reason"
                    .to_owned(),
            );
        }
    }
    let standing: BTreeSet<&str> = report
        .expectations
        .iter()
        .map(|claim| claim.id.as_str())
        .collect();
    for entry in &entries {
        if !standing.contains(entry.as_str()) {
            notes.violated(
                entry,
                "the ledger accepts this mutant and the run does not hold it; an acceptance \
                 nothing answers is one nobody will notice going stale"
                    .to_owned(),
            );
        }
    }
    for claim in report.expectations.iter().filter(|it| it.standing == MET) {
        if !entries.contains(&claim.id) {
            notes.violated(
                &claim.id,
                "the run met a claim the ledger does not carry; an acceptance a reviewer \
                 cannot find is not one they made"
                    .to_owned(),
            );
        }
    }
    notes.looked()
}

/// Every mutant the ledger accepts, by the identity it names.
fn accepted(document: &toml::Table) -> BTreeSet<String> {
    document
        .get("mutation")
        .and_then(toml::Value::as_table)
        .and_then(|mutation| mutation.get("expect"))
        .and_then(toml::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("id")?.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
