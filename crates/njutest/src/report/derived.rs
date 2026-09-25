// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The findings a part's own records decide, through the same functions a run raises them with, which a report may neither drop nor add to.

use super::{BuildReport, Finding};

/// Every finding the records of `report` decide, as the run raises them: the fault, crash and schedule records in any part, and the drift, knob, hollow-target and dimension findings in a part of the whole catalog, which a shard leaves to the merge.
#[must_use]
pub fn findings(report: &BuildReport) -> Vec<Finding> {
    let mut derived = Vec::new();
    derived.extend(super::faults::found(&report.faults));
    derived.extend(super::crashes::found(&report.crashes));
    derived.extend(super::concurrency::found(&report.concurrency));
    if report.scope.shard.is_none() {
        derived.extend(super::hollow::found(&report.mutants));
        derived.extend(super::drift::found(&report.drift, &report.mutants));
        derived.extend(super::knobs::found(&report.knobs, &report.mutants));
        if report.contract.asks_every_dimension() {
            derived.extend(super::matrix::holes(&super::matrix::rows(
                &super::matrix::Evidence::of(report),
            )));
        }
    }
    derived
}
