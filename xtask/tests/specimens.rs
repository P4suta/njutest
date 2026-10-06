// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every specimen an audit plants is on the published schemas, so one that leaves out a field every run carries is caught where it is written, not first by an audit of a real run.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::path::Path;

use serde_json::Value;
use xtask::schemas::{Checker, Checkers, Producer};

/// Where `document` departs from `checker`, said about `what`, or nothing where it holds.
fn departure(checker: &Checker, document: &Value, what: &str) -> Option<String> {
    match checker.check(document) {
        Ok(()) => None,
        Err(off) => Some(format!("{what}: {off}")),
    }
}

/// Every line of the recording `path`, each where it departs from `producer`'s schema.
fn recording_departures(checkers: &Checkers, path: &Path, producer: Producer) -> Vec<String> {
    let text = std::fs::read_to_string(path).expect("a laid recording reads");
    (1_u64..)
        .zip(text.lines())
        .filter_map(|(at, line)| {
            let event = xtask::strictjson::from_str(line).expect("a laid line is JSON");
            departure(
                checkers.lines(producer),
                &event,
                &format!("{} line {at}", path.display()),
            )
        })
        .collect()
}

/// The report laid at `path`, where it departs from `checker`.
fn report_departure(checker: &Checker, path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).expect("a laid report reads");
    let document = xtask::strictjson::from_str(&text).expect("a laid report is JSON");
    departure(checker, &document, &path.display().to_string())
}

/// Every place the proofaudit specimen `perturbation`, laid as the audit lays it, departs from the published schemas: its report, its recording, the engine's recording beside it, and each shard's.
fn proofaudit_departures(
    checkers: &Checkers,
    perturbation: &xtask::proofaudit::sentinel::Perturbation,
) -> Vec<String> {
    let laid = perturbation.lay().expect("the specimen lays out");
    let mut found = Vec::new();
    let report = checkers.assurance_report();
    found.extend(report_departure(
        report,
        &laid.run().join(xtask::proofaudit::REPORT_FILE),
    ));
    if let Some(trace) = laid.trace() {
        found.extend(recording_departures(
            checkers,
            &trace.join("trace.jsonl"),
            Producer::Runner,
        ));
        if perturbation.engine.is_some() {
            found.extend(recording_departures(
                checkers,
                &trace.join("builds/0000000000/engine/trace.jsonl"),
                Producer::Engine,
            ));
        }
    }
    for shard in laid.shards() {
        found.extend(report_departure(
            report,
            &shard.join(xtask::proofaudit::REPORT_FILE),
        ));
    }
    if let Some(traces) = laid.traces() {
        for run in std::fs::read_dir(traces).expect("the shards' recordings list") {
            let run = run.expect("a shard's recording").path();
            found.extend(recording_departures(
                checkers,
                &run.join("trace.jsonl"),
                Producer::Runner,
            ));
        }
    }
    found
        .into_iter()
        .map(|off| format!("{}: {off}", perturbation.name))
        .collect()
}

#[test]
fn every_proofaudit_specimen_is_on_the_published_schemas() {
    let checkers = Checkers::compiled().expect("the published schemas compile");
    let specimens: Vec<_> = std::iter::once(xtask::proofaudit::sentinel::clean())
        .chain(
            xtask::proofaudit::Layer::ALL
                .into_iter()
                .flat_map(xtask::proofaudit::Layer::planted),
        )
        .collect();
    let off: Vec<String> = specimens
        .iter()
        .flat_map(|specimen| proofaudit_departures(&checkers, specimen))
        .collect();
    assert!(
        specimens.len() > 1 && off.is_empty(),
        "a specimen is off the schema a run's own output is held to, so the audit refuses it \
         before any layer reads it:\n{}",
        off.join("\n")
    );
}

#[test]
fn every_engine_audit_specimen_is_on_the_published_schemas() {
    let checkers = Checkers::compiled().expect("the published schemas compile");
    let specimens: Vec<_> = std::iter::once(xtask::engineaudit::sentinel::clean())
        .chain(
            xtask::engineaudit::Layer::ALL
                .into_iter()
                .flat_map(xtask::engineaudit::Layer::planted),
        )
        .collect();
    let mut off = Vec::new();
    for specimen in &specimens {
        let laid = specimen.lay().expect("the specimen lays out");
        off.extend(
            report_departure(
                checkers.engine_report(),
                &laid.run().join(xtask::engineaudit::REPORT_FILE),
            )
            .map(|off| format!("{}: {off}", specimen.name)),
        );
        off.extend(
            recording_departures(
                &checkers,
                &laid.trace().join("trace.jsonl"),
                Producer::Engine,
            )
            .into_iter()
            .map(|off| format!("{}: {off}", specimen.name)),
        );
    }
    assert!(
        specimens.len() > 1 && off.is_empty(),
        "a specimen is off the schema a run's own output is held to:\n{}",
        off.join("\n")
    );
}
