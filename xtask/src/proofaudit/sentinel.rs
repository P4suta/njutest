// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A clean synthetic run, and the defects planted in it that each layer of the audit must find.

use std::path::Path;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::{Layer, REPORT_FILE};

/// The run the specimen report names.
pub const RUN: &str = "20260906T101500Z-9f1c2d";
/// The display identity of the specimen's killed mutant.
pub const KILLED: &str = "aaaaaaaaaaaaaaaaaaaa";
/// The display identity of the specimen's survivor, which the one finding names.
pub const SURVIVED: &str = "bbbbbbbbbbbbbbbbbbbb";
/// The one target the specimen run tested with.
pub const TARGET: &str = "pkg/test/lib";
/// The display identity of the fault a planted defect puts.
pub const FAULTED: &str = "cccccccccccccccccccc";
/// The question about the status of the one exchange [`went_past`] holds.
pub const ASKED: &str = "f714f108a1ce93e4cae5d149115f5f2efc4d4ceb620ccecc762f1c4b914022ed";

/// A scoped run of two mutants, one killed and one survivor its finding names, over one target that passed.
#[must_use]
pub fn base() -> Value {
    json!({
        "schema": "njutest-assurance-report-v1",
        "schema_version": 2,
        "run_id": RUN,
        "run_kind": "scoped",
        "contract": "standard-v1",
        "verdict": "INSUFFICIENT",
        "accounting": {
            "targets": { "selected": 1, "passed": 1, "failed": 0, "skipped": 0, "missing": 0 },
            "mutants": {
                "cataloged": 2,
                "rejected": 0,
                "executed": 2,
                "killed": 1,
                "survived": 1,
                "step_limit_reached": 0,
                "waited": 0,
                "unreached": 0,
                "equivalent": 0,
                "accepted": 0,
                "reused_killed": 0,
                "reused_survived": 0,
                "model_noticed": 0,
                "model_proved": 0
            },
            "soundness": { "unsafe_items": 0, "packages_with_unsafe": 0, "executed": false },
            "faults": {
                "sites": 0, "noticed": 0, "unnoticed": 0, "unreached": 0,
                "waited": 0, "undecided": 0, "not_put": 0
            }
        },
        "faults": [],
        "targets": [
            {
                "id": "3f2a1b0c9d8e7f60",
                "name": TARGET,
                "package": "pkg",
                "status": "passed",
                "duration_ms": 5,
                "message": null
            }
        ],
        "mutants": [
            {
                "id": "a".repeat(64),
                "display_id": KILLED,
                "path": "src/lib.rs",
                "position": { "line": 7, "column": 9, "character_column": 9 },
                "rule": "negate-condition@1",
                "decision": {
                    "outcome": "killed", "killed_by": TARGET, "step_boundary": null
                },
                "accepted": false,
                "reuse": { "reused": false, "source_run_id": null }
            },
            {
                "id": "b".repeat(64),
                "display_id": SURVIVED,
                "path": "src/lib.rs",
                "position": { "line": 11, "column": 5, "character_column": 5 },
                "rule": "return-ok-default@1",
                "decision": {
                    "outcome": "survived", "killed_by": null, "step_boundary": null
                },
                "accepted": false,
                "reuse": { "reused": false, "source_run_id": null }
            }
        ],
        "models": [],
        "findings": [
            {
                "kind": "surviving-mutant",
                "subject": SURVIVED,
                "detail": "no test noticed return-ok-default@1 at src/lib.rs:11",
                "position": null
            }
        ],
        "limitations": []
    })
}

/// The display identity of the crash the crashes layer's planted defects put.
const CRASHED: &str = "dddddddddddddddddddd";

/// A report of one crash site decided `decision`, counted once under `counted`, with `finding` beside the specimen's own where it names one.
fn crash_reported(decision: &Value, counted: &str, finding: Option<(&str, &str)>) -> Value {
    let mut document = with(json!({
        "crashes": [{
            "catalog_index": 0,
            "id": "d".repeat(64),
            "display_id": CRASHED,
            "path": "src/lib.rs",
            "item": "save",
            "position": null,
            "decision": decision
        }],
        "accounting": { "crashes": { "sites": 1, counted: 1 } }
    }));
    if let (Some((kind, subject)), Some(findings)) = (
        finding,
        document.get_mut("findings").and_then(Value::as_array_mut),
    ) {
        findings.push(json!({
            "kind": kind, "subject": subject, "detail": "planted", "position": null
        }));
    }
    document
}

/// One step of the crash [`CRASHED`], as the runner records it.
fn crash_step(taken: &Value) -> Value {
    json!({
        "timestamp": "2026-09-06T00:00:07Z", "elapsed_ms": 7,
        "type": "crash-step",
        "step": { "crash": CRASHED, "taken": taken }
    })
}

/// One run of `test` with the crash [`CRASHED`] put to it, which `ended` stopped at the call, passed, failed or waited, with the `files` a stop left or a next or fresh run failed.
fn crash_run(test: &str, stage: &str, ended: &str, files: &[&str]) -> Value {
    let (exit_code, outcome) = match ended {
        "stopped" | "chose" => (93, "killed"),
        "passed" => (0, "survived"),
        "waited" => (124, "waited"),
        _ => (101, "killed"),
    };
    let (left, failed): (&[&str], &[&str]) = if stage == "crash" {
        (files, &[])
    } else {
        (&[], files)
    };
    let issued = (stage == "crash").then(|| {
        json!({
            "mutant": "d".repeat(64), "catalog": "c".repeat(64), "nonce": "0".repeat(32),
            "read": (ended == "stopped").then(|| notice(&"0".repeat(32)))
        })
    });
    json!({
        "timestamp": "2026-09-06T00:00:07Z", "elapsed_ms": 7,
        "type": "crash-exec",
        "crash": {
            "crash": CRASHED, "target": TARGET, "test": test, "stage": stage,
            "exit_code": exit_code, "outcome": outcome, "noticed": ended == "stopped",
            "issued": issued, "left": left, "failed": failed
        }
    })
}

/// The defects planted for the crashes layer about the evidence a stop is decided on: a notice the engine never read, one naming another run's nonce, and a run issued another mutation.
fn crashes_planted_against_evidence(clean: &Perturbation) -> Vec<Perturbation> {
    let on = format!("{TARGET}::t");
    let restarted = json!({ "decision": "restarted", "on": on, "left": ["count"] });
    let planted = |name: &'static str, document: Value, events: Vec<Value>| Perturbation {
        name,
        document,
        events: Some(events),
        ..clean.clone()
    };
    vec![
        planted(
            "a stop claimed over a run whose engine read no notice",
            crash_reported(&restarted, "restarted", None),
            reissued(crash_recorded(crash_restarted()), |issued| {
                issued["read"] = Value::Null;
            }),
        ),
        planted(
            "a stop claimed over a notice that carries another run's nonce",
            crash_reported(&restarted, "restarted", None),
            reissued(crash_recorded(crash_restarted()), |issued| {
                issued["read"] = json!(notice(&"f".repeat(32)));
            }),
        ),
        planted(
            "a stop claimed over a run issued another mutation than the site",
            crash_reported(&restarted, "restarted", None),
            reissued(crash_recorded(crash_restarted()), |issued| {
                let nonce = "a".repeat(32);
                let other = format!(
                    "{}\t{nonce}\t{}\t{}\n",
                    crate::crashes::NOTICE_SCHEMA,
                    "c".repeat(64),
                    "e".repeat(64)
                );
                issued["nonce"] = json!(nonce);
                issued["mutant"] = json!("e".repeat(64));
                issued["read"] = json!(other);
            }),
        ),
    ]
}

/// `events` with what the engine issued every crashed run changed by `change`.
fn reissued(mut events: Vec<Value>, change: impl Fn(&mut Value)) -> Vec<Value> {
    for event in &mut events {
        if let Some(issued) = event
            .get_mut("crash")
            .and_then(|crash| crash.get_mut("issued"))
            .filter(|issued| issued.is_object())
        {
            change(issued);
        }
    }
    events
}

/// The notice the runtime publishes for a run of [`CRASHED`] issued `nonce`.
fn notice(nonce: &str) -> String {
    format!(
        "{}\t{nonce}\t{}\t{}\n",
        crate::crashes::NOTICE_SCHEMA,
        "c".repeat(64),
        "d".repeat(64)
    )
}

/// `events` with every crashed run issued a nonce of its own, and the notice it published naming that nonce.
fn nonced(mut events: Vec<Value>) -> Vec<Value> {
    let issued_runs = events.iter_mut().filter_map(|event| {
        event
            .get_mut("crash")
            .and_then(|crash| crash.get_mut("issued"))
            .filter(|issued| issued.is_object())
    });
    for (issued, next) in issued_runs.zip(1_u32..) {
        let nonce = format!("{next:032x}");
        if issued.get("read").is_some_and(Value::is_string) {
            issued["read"] = json!(notice(&nonce));
        }
        issued["nonce"] = json!(nonce);
    }
    events
}

/// The specimen's recording with the route that asks the one test `t`, and `runs` after it.
fn crash_recorded(runs: Vec<Value>) -> Vec<Value> {
    let route = crash_step(&json!({
        "kind": "route", "asked": [{ "target": TARGET, "tests": ["t"] }]
    }));
    numbered(nonced(
        routes().into_iter().chain([route]).chain(runs).collect(),
    ))
}

/// A stop of `t` that left `count`, and a next run that passed over it.
fn crash_restarted() -> Vec<Value> {
    vec![
        crash_run("t", "crash", "stopped", &["count"]),
        crash_run("t", "next", "passed", &[]),
    ]
}

/// A stop of `t`, a next run that failed `t`, and the rounds that confirm it, the last of whose next runs failed `again`.
fn crash_corrupted(again: &str) -> Vec<Value> {
    let mut runs = vec![
        crash_run("t", "crash", "stopped", &["count"]),
        crash_run("t", "next", "failed", &["t"]),
    ];
    for round in 1..=crate::crashes::CONFIRMATIONS {
        let failed = if round == crate::crashes::CONFIRMATIONS {
            again
        } else {
            "t"
        };
        runs.extend([
            crash_run("t", "fresh", "passed", &[]),
            crash_run("t", "crash", "stopped", &["count"]),
            crash_run("t", "next", "failed", &[failed]),
        ]);
    }
    runs
}

/// The defects planted for the crashes layer: each a report that claims a decision, or drops one, where the recorded steps decide otherwise.
fn crashes_planted(clean: &Perturbation) -> Vec<Perturbation> {
    let on = format!("{TARGET}::t");
    let restarted = json!({ "decision": "restarted", "on": on, "left": ["count"] });
    let planted = |name: &'static str, document: Value, events: Vec<Value>| Perturbation {
        name,
        document,
        events: Some(events),
        ..clean.clone()
    };
    vec![
        planted(
            "a crash said to have restarted that no recorded run stopped at",
            crash_reported(&restarted, "restarted", None),
            crash_recorded(Vec::new()),
        ),
        planted(
            "a crash said to be unreached whose recorded run waited",
            crash_reported(&json!({ "decision": "unreached" }), "unreached", None),
            crash_recorded(vec![crash_run("t", "crash", "waited", &[])]),
        ),
        planted(
            "a crash said to have restarted whose stop left nothing",
            crash_reported(&restarted, "restarted", None),
            crash_recorded(vec![
                crash_run("t", "crash", "stopped", &[]),
                crash_run("t", "next", "passed", &[]),
            ]),
        ),
        planted(
            "a crash said to be undecided whose runs decide it corrupt",
            crash_reported(
                &json!({ "decision": "undecided", "on": on, "why": "planted" }),
                "undecided",
                Some(("not-measured", CRASHED)),
            ),
            crash_recorded(crash_corrupted("t")),
        ),
        planted(
            "corrupt runs no site of the report holds",
            with(json!({})),
            crash_recorded(crash_corrupted("t")),
        ),
        planted(
            "a crash said to be not put that no recorded step refused",
            crash_reported(
                &json!({ "decision": "not-put", "diagnostic": "planted" }),
                "not_put",
                None,
            ),
            numbered(routes()),
        ),
        planted(
            "a crash said to be unreached whose route asks a test no run was recorded for",
            crash_reported(&json!({ "decision": "unreached" }), "unreached", None),
            crash_recorded(Vec::new()),
        ),
    ]
    .into_iter()
    .chain(crashes_planted_against_order(clean))
    .chain(crashes_planted_against_evidence(clean))
    .collect()
}

/// The defects planted for the crashes layer about counts, the runs a decision rests on, and the order they come in.
fn crashes_planted_against_order(clean: &Perturbation) -> Vec<Perturbation> {
    let on = format!("{TARGET}::t");
    let restarted = json!({ "decision": "restarted", "on": on, "left": ["count"] });
    let corrupt = json!({ "decision": "corrupt", "on": on, "failed": ["t"] });
    let planted = |name: &'static str, document: Value, events: Vec<Value>| Perturbation {
        name,
        document,
        events: Some(events),
        ..clean.clone()
    };
    vec![
        planted(
            "crash accounting that does not add up to the report's sites",
            with(json!({
                "crashes": [{
                    "catalog_index": 0, "id": "d".repeat(64), "display_id": CRASHED,
                    "path": "src/lib.rs", "item": "save", "position": null,
                    "decision": restarted
                }],
                "accounting": { "crashes": { "sites": 7, "restarted": 1 } }
            })),
            crash_recorded(crash_restarted()),
        ),
        planted(
            "a restart claimed over a next run of another test",
            crash_reported(&restarted, "restarted", None),
            crash_recorded(vec![
                crash_run("t", "crash", "stopped", &["count"]),
                crash_run("u", "next", "passed", &[]),
            ]),
        ),
        planted(
            "a crash said to be corrupt whose second next run failed another test",
            crash_reported(&corrupt, "corrupt", Some(("corrupt-after-crash", CRASHED))),
            crash_recorded(crash_corrupted("u")),
        ),
        planted(
            "a restart claimed with a run recorded after its decision",
            crash_reported(&restarted, "restarted", None),
            crash_recorded(
                crash_restarted()
                    .into_iter()
                    .chain([crash_run("t", "crash", "stopped", &["count"])])
                    .collect(),
            ),
        ),
        planted(
            "a restart claimed over a run that ended with the stop's status and no notice",
            crash_reported(&restarted, "restarted", None),
            crash_recorded(vec![
                crash_run("t", "crash", "chose", &["count"]),
                crash_run("t", "next", "passed", &[]),
            ]),
        ),
        planted(
            "a restart claimed where the stopped test wrote into the tree",
            crash_reported(&restarted, "restarted", None),
            crash_recorded(
                crash_restarted()
                    .into_iter()
                    .chain([crash_step(&json!({ "kind": "outside" }))])
                    .collect(),
            ),
        ),
        planted(
            "an undecided crash with no not-measured finding",
            crash_reported(
                &json!({ "decision": "undecided", "on": on, "why": "planted" }),
                "undecided",
                None,
            ),
            crash_recorded(vec![crash_run("t", "crash", "waited", &[])]),
        ),
    ]
}

/// The defects planted for the faults layer, and for the evidence beside a fault it also audits.
fn faults_and_besides_planted(clean: &Perturbation) -> Vec<Perturbation> {
    faults_planted(clean)
        .into_iter()
        .chain(besides_planted(clean))
        .collect()
}

/// The defect planted for the dimensions layer: a whole-v1 run that names none of the dimensions its records leave a hole.
fn dimensions_planted(clean: Perturbation) -> Vec<Perturbation> {
    let mut unput = with(json!({ "contract": "whole-v1" }));
    merge(
        &mut unput,
        json!({
            "knobs": [{
                "target": TARGET, "knob": "timezone",
                "standing": { "state": "not-put", "why": "zone-missing" }
            }]
        }),
    );
    if let Some(findings) = unput.get_mut("findings").and_then(Value::as_array_mut) {
        for dimension in ["fault", "durable"] {
            findings.push(json!({
                "kind": "dimension-not-measured", "subject": dimension,
                "detail": "planted", "position": null
            }));
        }
    }
    vec![
        Perturbation {
            name: "a whole-v1 run that names none of the dimensions it left a hole",
            document: with(json!({ "contract": "whole-v1" })),
            ..clean.clone()
        },
        Perturbation {
            name: "a whole-v1 run that calls knobs this machine could not put measured",
            document: unput,
            ..clean
        },
    ]
}

/// The defects planted for the evidence beside a fault: a record the recording does not hold, and one its recorded runs do not support.
fn besides_planted(clean: &Perturbation) -> Vec<Perturbation> {
    vec![
        Perturbation {
            name: "evidence beside a fault the recording does not hold",
            document: beside_claimed(),
            events: Some(fault_recorded(
                &json!({ "decision": "unnoticed" }),
                "survived",
            )),
            ..clean.clone()
        },
        Perturbation {
            name: "evidence beside a fault its recorded runs do not support",
            document: beside_claimed(),
            events: Some(
                fault_recorded(&json!({ "decision": "unnoticed" }), "survived")
                    .into_iter()
                    .chain([4, 5].map(|at| {
                        json!({
                            "timestamp": "2026-09-06T00:00:04Z", "elapsed_ms": at,
                            "type": "beside-run",
                            "pair": {
                                "mutant": SURVIVED, "fault": FAULTED, "target": TARGET,
                                "alone": "killed", "with": "killed"
                            }
                        })
                    }))
                    .chain([json!({
                        "timestamp": "2026-09-06T00:00:06Z", "elapsed_ms": 6,
                        "type": "beside",
                        "beside": {
                            "mutant": SURVIVED, "fault": FAULTED, "target": TARGET,
                            "failed": "beside"
                        }
                    })])
                    .collect(),
            ),
            ..clean.clone()
        },
    ]
}

/// The clean report with one unnoticed fault and a record saying the survivor failed only beside it.
fn beside_claimed() -> Value {
    with(json!({
        "faults": [fault_site(&json!({ "decision": "unnoticed" }))],
        "accounting": { "faults": { "sites": 1, "unnoticed": 1 } },
        "findings": [{}, {
            "kind": "unnoticed-fault",
            "subject": FAULTED,
            "detail": "nothing noticed the call failing",
            "position": null
        }],
        "beside": [{
            "mutant": SURVIVED, "fault": FAULTED, "target": TARGET, "failed": "beside"
        }]
    }))
}

/// The defects planted for the faults layer: one for every way a fault's record can disagree with what the recording says ran.
fn faults_planted(clean: &Perturbation) -> Vec<Perturbation> {
    vec![
        Perturbation {
            name: "a failed call nothing noticed that no finding names",
            document: with(json!({
                "faults": [fault_site(&json!({ "decision": "unnoticed" }))],
                "accounting": { "faults": { "sites": 1, "unnoticed": 1 } }
            })),
            events: Some(fault_recorded(
                &json!({ "decision": "unnoticed" }),
                "survived",
            )),
            ..clean.clone()
        },
        Perturbation {
            name: "a fault said to be noticed that no execution of it failed",
            document: with(json!({
                "faults": [fault_site(&json!({ "decision": "noticed", "by": TARGET }))],
                "accounting": { "faults": { "sites": 1, "noticed": 1 } }
            })),
            events: Some(fault_recorded(
                &json!({ "decision": "noticed", "by": TARGET }),
                "survived",
            )),
            ..clean.clone()
        },
        Perturbation {
            name: "a fault said to be unnoticed that an execution of it failed",
            document: with(json!({
                "faults": [fault_site(&json!({ "decision": "unnoticed" }))],
                "accounting": { "faults": { "sites": 1, "unnoticed": 1 } },
                "findings": [{}, {
                    "kind": "unnoticed-fault",
                    "subject": FAULTED,
                    "detail": "nothing noticed the call failing",
                    "position": null
                }]
            })),
            events: Some(fault_recorded(
                &json!({ "decision": "unnoticed" }),
                "killed",
            )),
            ..clean.clone()
        },
    ]
    .into_iter()
    .chain(faults_owing(clean))
    .collect()
}

/// The faults layer's planted defects about what a fault's decision owes: the finding a decision raises, and the attribution a write rests on.
fn faults_owing(clean: &Perturbation) -> Vec<Perturbation> {
    vec![
        Perturbation {
            name: "an undecided fault whose not-measured finding the report dropped",
            document: with(json!({
                "faults": [fault_site(&json!({
                    "decision": "undecided", "on": TARGET, "why": "the kill did not happen the second time"
                }))],
                "accounting": { "faults": { "sites": 1, "undecided": 1 } }
            })),
            events: Some(fault_recorded(
                &json!({
                    "decision": "undecided", "on": TARGET, "why": "the kill did not happen the second time"
                }),
                "killed",
            )),
            ..clean.clone()
        },
        Perturbation {
            name: "a write called broken-under-fault that no attribution ties to the fault",
            document: with(json!({
                "verdict": "DEFECT",
                "faults": [fault_site(&json!({ "decision": "unnoticed" }))],
                "accounting": { "faults": { "sites": 1, "unnoticed": 1 } },
                "findings": [{}, {
                    "kind": "unnoticed-fault",
                    "subject": FAULTED,
                    "detail": "nothing noticed the call failing",
                    "position": null
                }, {
                    "kind": "broken-under-fault",
                    "subject": FAULTED,
                    "detail": "wrote failed-read.log",
                    "position": null
                }]
            })),
            events: Some(fault_recorded(
                &json!({ "decision": "unnoticed" }),
                "survived",
            )),
            ..clean.clone()
        },
        Perturbation {
            name: "a fault site the recording holds and the report dropped",
            events: Some(fault_recorded(
                &json!({ "decision": "unnoticed" }),
                "survived",
            )),
            ..clean.clone()
        },
    ]
}

/// A route and an execution for each mutant of [`base`], then one fault the one target ran to `outcome`, and the site the run said it came to `decision`.
fn fault_recorded(decision: &Value, outcome: &str) -> Vec<Value> {
    routes()
        .into_iter()
        .chain([
            json!({
                "timestamp": "2026-09-06T00:00:02Z", "elapsed_ms": 2,
                "type": "fault-exec",
                "fault": {
                    "fault": FAULTED, "role": "first", "target": TARGET, "args": [],
                    "outcome": outcome, "duration_ms": 5, "alone": false
                }
            }),
            json!({
                "timestamp": "2026-09-06T00:00:03Z", "elapsed_ms": 3,
                "type": "fault",
                "fault": fault_site(decision)
            }),
        ])
        .collect()
}

/// One fault site, decided `decision`.
fn fault_site(decision: &Value) -> Value {
    json!({
        "catalog_index": 0,
        "id": "c".repeat(64),
        "display_id": FAULTED,
        "path": "src/lib.rs",
        "item": "load",
        "position": { "line": 13, "column": 16, "character_column": 16 },
        "decision": decision
    })
}

/// Lays `overrides` over `document`: objects key by key, a non-empty array position by position, anything else by replacement.
pub fn merge(document: &mut Value, overrides: Value) {
    match (document, overrides) {
        (Value::Object(into), Value::Object(from)) => {
            for (key, value) in from {
                merge(into.entry(key).or_insert(Value::Null), value);
            }
        }
        (Value::Array(into), Value::Array(from)) if !from.is_empty() => {
            for (at, value) in from.into_iter().enumerate() {
                match into.get_mut(at) {
                    Some(existing) => merge(existing, value),
                    None => into.push(value),
                }
            }
        }
        (into, from) => *into = from,
    }
}

/// The clean report with `overrides` laid over it.
#[must_use]
pub fn with(overrides: Value) -> Value {
    let mut document = base();
    merge(&mut document, overrides);
    document
}

/// A route and an execution for each mutant of [`base`], with no proof removing anything.
#[must_use]
pub fn routes() -> Vec<Value> {
    routes_for(&[(KILLED, "killed"), (SURVIVED, "survived")])
}

/// A route and an execution for each of `mutants`, each with the outcome it came to, with no proof removing anything.
fn routes_for(mutants: &[(&str, &str)]) -> Vec<Value> {
    let mut events = Vec::new();
    for &(mutant, outcome) in mutants {
        events.push(json!({
            "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0,
            "type": "route",
            "route": {
                "mutant": mutant, "granularity": "block", "fallback": null,
                "reaching": ["t1"], "discharged": [], "considered": [], "reused": null
            }
        }));
        events.push(json!({
            "timestamp": "2026-09-06T00:00:01Z", "elapsed_ms": 1,
            "type": "mutant-exec",
            "mutant": {
                "mutant": mutant, "target": "t1", "args": [], "outcome": outcome,
                "duration_ms": 5
            }
        }));
    }
    numbered(events)
}

/// `events` with every sequence number counted again from one.
fn numbered(mut events: Vec<Value>) -> Vec<Value> {
    for (seq, event) in (1_u64..).zip(events.iter_mut()) {
        merge(event, json!({ "seq": seq }));
    }
    events
}

/// The recording of [`routes`], after which one target, `blunt`, was put to both mutations and noticed neither.
#[must_use]
pub fn never_noticed() -> Vec<Value> {
    let mut events = routes();
    for mutant in [KILLED, SURVIVED] {
        events.push(json!({
            "timestamp": "2026-09-06T00:00:02Z",
            "elapsed_ms": 2,
            "type": "mutant-exec",
            "mutant": {
                "mutant": mutant, "target": "blunt", "args": [], "outcome": "survived",
                "duration_ms": 5
            }
        }));
    }
    numbered(events)
}

/// One exchange that went past the `api` seam, which licenses the question [`ASKED`] among others.
#[must_use]
pub fn went_past() -> Value {
    json!({
        "type": "wire-exchange",
        "exchange": {
            "capability": "api",
            "seq": 0,
            "during": null,
            "duration_ms": 1,
            "read": { "wire": "http", "method": "GET", "path": "/orders", "status": 200 },
            "request_bytes": 1,
            "response_bytes": 1
        }
    })
}

/// A seam answer of `decision`, with the test that noticed it where that is what it says.
fn answered(decision: &str) -> Value {
    match decision {
        "tests" => json!({ "decision": "tests", "noticed_by": TARGET }),
        "proved" => json!({ "decision": "proved", "proof": "never-reached" }),
        other => json!({ "decision": other }),
    }
}

/// One fault put to the suite, decided as `decision` says.
#[must_use]
pub fn was_put(fault: &str, decision: &str) -> Value {
    json!({
        "type": "wire-exec",
        "wire": {
            "fault": fault,
            "capability": "api",
            "seq": 0,
            "rule": "status-server-error",
            "answer": answered(decision)
        }
    })
}

/// One touch record the engine writes about `target`, measured on `measured` over the one test `lib::works`, having reached `reached`.
#[must_use]
pub fn touch(measured: &str, reached: &[u32]) -> Value {
    json!({ "type": "touch", "touch": touched(measured, reached) })
}

/// The build record naming [`TARGET`] as a library test binary libtest runs.
fn built() -> Value {
    json!({
        "type": "build",
        "build": {
            "targets": [TARGET],
            "details": [{ "id": TARGET, "kind": "lib", "harness": true, "limitations": [] }]
        }
    })
}

/// The baseline record of [`TARGET`], run with the harness arguments `args`.
fn verified(args: &[&str]) -> Value {
    json!({
        "type": "verify",
        "verify": {
            "target": TARGET, "outcome": "survived", "tests_run": 1, "duration_ms": 1,
            "args": args, "remembered": false, "retried": false, "declined": []
        }
    })
}

/// The record of [`touch`] without its event.
fn touched(measured: &str, reached: &[u32]) -> Value {
    json!({
        "target": TARGET,
        "measured": measured,
        "mutant": null,
        "tests": 1,
        "sites": reached.len(),
        "loose": 0,
        "infected": 0,
        "entered": 0,
        "passed": ["lib::works"],
        "summary": { "protocol": "libtest", "tests_run": 1 },
        "reached_sites": reached,
        "entered_bodies": [],
        "infected_sites": [],
        "entered_items": []
    })
}

/// One control of [`TARGET`] the engine started with `TZ` set, which came to `outcome` with `failed` failing and `reach` becoming of its reach.
#[must_use]
pub fn perturbed(outcome: &str, failed: &[&str], reach: &Value) -> Value {
    json!({
        "type": "perturbed-control",
        "perturbed": {
            "target": TARGET,
            "perturbation": {
                "environment": [{ "name": "TZ", "value": "Australia/Lord_Howe" }],
                "launcher": null,
                "arguments": [],
                "delay": null,
                "confirms": null
            },
            "outcome": outcome,
            "failed_tests": failed,
            "duration_ms": 5,
            "reach": reach
        }
    })
}

/// The reach of a control that recorded having reached `reached` over the one test `lib::works`.
#[must_use]
pub fn recorded_reach(reached: &[u32]) -> Value {
    json!({ "state": "recorded", "touch": touched("control", reached) })
}

/// The report's knob record about [`TARGET`] under the time zone, in the standing `state` names.
#[must_use]
pub fn knobbed(state: &str) -> Value {
    json!({
        "knobs": [{ "target": TARGET, "knob": "timezone", "standing": { "state": state } }]
    })
}

/// The report's drift record about [`TARGET`], in the standing `state` names.
#[must_use]
pub fn drifted(state: &str) -> Value {
    let record = match state {
        "moved" => moved(TARGET),
        "not-measured" => json!({ "target": TARGET, "state": state, "why": "no-control" }),
        other => json!({ "target": TARGET, "state": other }),
    };
    json!({ "drift": [record] })
}

/// A drift record saying `target` reached one more site on a control than on its baseline, and nothing else moved.
#[must_use]
pub fn moved(target: &str) -> Value {
    json!({
        "target": target, "state": "moved",
        "reached": { "gained": [1], "lost": [] },
        "bodies": { "gained": [], "lost": [] },
        "infected": { "gained": [], "lost": [] },
        "entered": { "gained": [], "lost": [] }
    })
}

/// A specimen could not be laid out on disk.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SpecimenError {
    /// No temporary directory to lay it in.
    #[error("a temporary directory to lay the proofaudit specimen in: {source}")]
    Directory {
        /// What the filesystem said.
        #[source]
        source: std::io::Error,
    },
    /// One file of it could not be written.
    #[error("{path}: the proofaudit specimen could not be written: {source}")]
    Unwritable {
        /// The file.
        path: String,
        /// What the filesystem said.
        #[source]
        source: std::io::Error,
    },
    /// One event of the recording is not a JSON object.
    #[error("event {at} of the specimen recording is not an object")]
    NotAnObject {
        /// Its position in the recording.
        at: usize,
    },
    /// The flat report could not be completed into the document a run writes.
    #[error(transparent)]
    Incomplete(#[from] crate::specimen::CompletionError),
}

impl crate::error::Coded for SpecimenError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Directory { .. } | Self::Unwritable { .. } => {
                crate::error::XtCode::SpecimenUnwritable
            }
            Self::NotAnObject { .. } => crate::error::XtCode::SpecimenEvent,
            Self::Incomplete(_) => crate::error::XtCode::SpecimenIncomplete,
        }
    }
}

fn directory() -> Result<TempDir, SpecimenError> {
    tempfile::tempdir().map_err(|source| SpecimenError::Directory { source })
}

fn written(path: &Path, text: &str) -> Result<(), SpecimenError> {
    std::fs::write(path, text).map_err(|source| SpecimenError::Unwritable {
        path: path.display().to_string(),
        source,
    })
}

/// A run directory holding `document` as its report.
///
/// # Errors
/// [`SpecimenError`] when the directory or the report cannot be written.
pub fn run_directory(document: &Value) -> Result<TempDir, SpecimenError> {
    let run = directory()?;
    let laid = if document.get("document_type").is_some() {
        document.clone()
    } else {
        complete_report(document)?
    };
    written(&run.path().join(REPORT_FILE), &laid.to_string())?;
    Ok(run)
}

/// The complete document a run writes holding the flat specimen `flat`, as [`run_directory`] lays it.
///
/// # Errors
/// [`SpecimenError::Incomplete`] where `flat` is not an object, or the committed report it is completed from is not that shape.
pub fn complete_report(flat: &Value) -> Result<Value, SpecimenError> {
    Ok(crate::specimen::complete(flat)?)
}

/// The run the clean specimen's merge is, whose shards are this run with `-s1` and `-s2` after it.
pub const MERGED: &str = "20260906T101500Z-9f1c2e";

/// The flat report of the run that measured only `mutant` of [`base`], at the catalog index shard `index` of two owns: its rows, accounting, findings and verdict are that shard's own.
fn shard_flat(mutant: &str, index: u64) -> Value {
    let mut flat = with(drifted("held"));
    let owned = |row: &Value| row.get("display_id").and_then(Value::as_str) == Some(mutant);
    let noticed = mutant == KILLED;
    if let Some(Value::Array(rows)) = flat.get_mut("mutants") {
        rows.retain(owned);
        for row in rows.iter_mut() {
            merge(row, json!({ "catalog_index": index.saturating_sub(1) }));
        }
    }
    if let Some(Value::Array(rows)) = flat.get_mut("findings") {
        rows.retain(|row| row.get("subject").and_then(Value::as_str) == Some(mutant));
    }
    merge(
        &mut flat,
        json!({
            "verdict": if noticed { "PARTIAL" } else { "INSUFFICIENT" },
            "accounting": { "mutants": {
                "cataloged": 1,
                "executed": 1,
                "killed": u8::from(noticed),
                "survived": u8::from(!noticed)
            } }
        }),
    );
    flat
}

/// One shard a merged report was merged from: its document, and the recordings its run kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shard {
    /// The shard document.
    pub document: Value,
    /// The runner's recording, ending with the `run-end` that says what the shard concluded.
    pub events: Option<Vec<Value>>,
    /// The one configured build's engine recording.
    pub engine: Option<Vec<Value>>,
}

/// Shard `index` of two of the clean specimen, the run that measured `mutant` alone with the outcome it came to.
///
/// # Errors
/// [`SpecimenError::Incomplete`] where the shard's report cannot be completed.
fn shard_of((mutant, outcome): (&str, &str), index: u64) -> Result<Shard, SpecimenError> {
    let flat = shard_flat(mutant, index);
    Ok(Shard {
        document: crate::specimen::shard(&flat, (&format!("{MERGED}-s{index}"), index, 2))?,
        events: Some(concluding(&flat, &routes_for(&[(mutant, outcome)]))),
        engine: Some(vec![touch("baseline", &[0, 1]), touch("control", &[0, 1])]),
    })
}

/// The clean specimen measured in two shards, each with its own recording, and merged: on which no layer may find anything.
///
/// # Errors
/// [`SpecimenError::Incomplete`] where a shard cannot be completed or the shards cannot be merged.
pub fn sharded_clean() -> Result<Perturbation, SpecimenError> {
    let shards = vec![
        shard_of((KILLED, "killed"), 1)?,
        shard_of((SURVIVED, "survived"), 2)?,
    ];
    let documents: Vec<Value> = shards.iter().map(|shard| shard.document.clone()).collect();
    Ok(Perturbation {
        name: "the clean specimen, measured in two shards and merged",
        document: crate::specimen::merged(&documents, MERGED)?,
        events: None,
        engine: None,
        shards,
        outputs: Vec::new(),
    })
}

/// The runner recording `events` of a run whose flat report says it concluded something, ending with the `run-end` that says so, since a complete report stores no verdict.
#[must_use]
pub fn concluding(document: &Value, events: &[Value]) -> Vec<Value> {
    let mut all = events.to_vec();
    if let Some(verdict) = document.get("verdict").and_then(Value::as_str) {
        all.push(crate::specimen::concluded(verdict, events.len()));
    }
    all
}

/// A recording directory holding `events` as its `trace.jsonl`, each wrapped in the envelope the runner writes, numbered by position where an event carries no envelope of its own.
///
/// # Errors
/// [`SpecimenError`] when an event is not an object, or the recording cannot be written.
pub fn recorded(events: &[Value]) -> Result<TempDir, SpecimenError> {
    recorded_by(events, crate::schemas::Producer::Runner)
}

/// As [`recorded`], for a recording `producer` writes, each payload completed with what its test leaves out.
///
/// # Errors
/// [`SpecimenError`] when an event is not an object, or the recording cannot be written.
pub fn recorded_by(
    events: &[Value],
    producer: crate::schemas::Producer,
) -> Result<TempDir, SpecimenError> {
    let trace = directory()?;
    record_into(trace.path(), events, producer)?;
    Ok(trace)
}

/// Writes `events` as the `trace.jsonl` `producer` writes into the directory `into`.
fn record_into(
    into: &Path,
    events: &[Value],
    producer: crate::schemas::Producer,
) -> Result<(), SpecimenError> {
    written(&into.join("trace.jsonl"), &stream_of(events, producer)?)
}

/// `events` as the lines `producer` writes, each payload completed with what its test leaves out.
fn stream_of(
    events: &[Value],
    producer: crate::schemas::Producer,
) -> Result<String, SpecimenError> {
    let mut stream = String::new();
    for ((at, event), position) in events.iter().enumerate().zip(1_u64..) {
        let mut payload = event
            .as_object()
            .cloned()
            .ok_or(SpecimenError::NotAnObject { at })?;
        let seq = payload.remove("seq").unwrap_or_else(|| json!(position));
        let timestamp = payload
            .remove("timestamp")
            .unwrap_or_else(|| json!("2026-09-06T00:00:00Z"));
        let elapsed_ms = payload.remove("elapsed_ms").unwrap_or_else(|| json!(at));
        crate::specimen::completed(producer, &mut payload);
        let envelope = json!({
            "seq": seq,
            "timestamp": timestamp,
            "elapsed_ms": elapsed_ms,
            "payload": Value::Object(payload),
        });
        stream.push_str(&envelope.to_string());
        stream.push('\n');
    }
    Ok(stream)
}

/// One run for the audit to re-decide: a report, and the recording beside it when the run kept one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Perturbation {
    /// What a refusal calls it.
    pub name: &'static str,
    /// The report.
    pub document: Value,
    /// The recording, as events before their envelope, or nothing for a run recorded without `--trace`.
    pub events: Option<Vec<Value>>,
    /// The one configured build's engine recording, as events before their envelope, or nothing where the run kept none.
    pub engine: Option<Vec<Value>>,
    /// The shards a merged report was merged from, each laid in a run directory of its own with its recording under the run it names; none for a run measured whole.
    pub shards: Vec<Shard>,
    /// What runs in the recording said, each by the path its exec record gives, kept beside the recording.
    pub outputs: Vec<(&'static str, &'static str)>,
}

/// The clean specimen every perturbation starts from, on which no layer may find anything.
#[must_use]
pub fn clean() -> Perturbation {
    let mut document = with(drifted("held"));
    merge(&mut document, knobbed("stable"));
    Perturbation {
        name: "clean",
        document: {
            let mut document = document;
            merge(
                &mut document,
                json!({ "concurrency": [{
                    "target": TARGET,
                    "standing": { "state": "single-threaded" },
                    "explored": { "state": "unexplored", "why": "not-needed" }
                }] }),
            );
            document
        },
        events: Some(routes()),
        engine: Some(clean_engine()),
        shards: Vec::new(),
        outputs: Vec::new(),
    }
}

/// A perturbation laid out on disk, alive for as long as the audit reads it.
#[derive(Debug)]
pub struct Laid {
    run: TempDir,
    trace: Option<TempDir>,
    shards: Vec<TempDir>,
    traces: Option<TempDir>,
}

impl Laid {
    /// The run directory.
    #[must_use]
    pub fn run(&self) -> &Path {
        self.run.path()
    }

    /// The recording directory, when the perturbation carries a recording.
    #[must_use]
    pub fn trace(&self) -> Option<&Path> {
        self.trace.as_ref().map(TempDir::path)
    }

    /// The run directory of each shard a merged report was merged from, in the order they were laid.
    #[must_use]
    pub fn shards(&self) -> Vec<&Path> {
        self.shards.iter().map(TempDir::path).collect()
    }

    /// The directory holding each shard's recording under the run it names, when the shards were laid.
    #[must_use]
    pub fn traces(&self) -> Option<&Path> {
        self.traces.as_ref().map(TempDir::path)
    }
}

impl Perturbation {
    /// Writes the run directory and the recording to fresh temporary directories.
    ///
    /// # Errors
    /// [`SpecimenError`] when either cannot be written.
    pub fn lay(&self) -> Result<Laid, SpecimenError> {
        let run = run_directory(&self.document)?;
        let trace = self
            .events
            .as_deref()
            .map(|events| {
                let trace = recorded(&concluding(&self.document, events))?;
                lay_engine(trace.path(), self.engine.as_deref())?;
                Ok::<_, SpecimenError>(trace)
            })
            .transpose()?;
        if let Some(trace) = trace.as_ref() {
            for (relative, said) in &self.outputs {
                let at = trace.path().join(relative);
                if let Some(parent) = at.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| {
                        SpecimenError::Unwritable {
                            path: parent.display().to_string(),
                            source,
                        }
                    })?;
                }
                written(&at, said)?;
            }
        }
        let shards = self
            .shards
            .iter()
            .map(|shard| run_directory(&shard.document))
            .collect::<Result<Vec<_>, _>>()?;
        let traces = if self.shards.is_empty() {
            None
        } else {
            let traces = directory()?;
            for shard in &self.shards {
                let (Some(events), Some(run_id)) = (
                    shard.events.as_deref(),
                    shard
                        .document
                        .pointer("/report/run_id")
                        .and_then(Value::as_str),
                ) else {
                    continue;
                };
                let into = traces.path().join(run_id);
                std::fs::create_dir_all(&into).map_err(|source| SpecimenError::Unwritable {
                    path: into.display().to_string(),
                    source,
                })?;
                lay_recording(&into, events, shard.engine.as_deref())?;
            }
            Some(traces)
        };
        Ok(Laid {
            run,
            trace,
            shards,
            traces,
        })
    }
}

/// The planted defect of the concurrency layer: a binary whose baseline reached code off its tests' threads, recorded as proven single-threaded.
fn loose_yet_single_threaded(clean: Perturbation) -> Perturbation {
    let mut loose = touch("baseline", &[0, 1]);
    merge(&mut loose, json!({ "touch": { "loose": 1 } }));
    Perturbation {
        name: "a binary whose baseline reached code off its tests' threads, proven single-threaded",
        engine: Some(vec![
            built(),
            verified(&["--test-threads=1"]),
            loose,
            touch("control", &[0, 1]),
        ]),
        ..clean
    }
}

/// The planted defect of the concurrency layer: a report that measured mutants and dropped every concurrency record.
fn unrecorded_threads(clean: Perturbation) -> Perturbation {
    let mut document = clean.document.clone();
    if let Some(part) = document.as_object_mut() {
        part.insert("concurrency".to_owned(), json!([]));
    }
    Perturbation {
        name: "a report that measured mutants and records nothing about any binary's threads",
        document,
        ..clean
    }
}

/// The planted defect of the concurrency layer: a binary libtest ran on every processor, recorded as proven single-threaded.
fn parallel_yet_single_threaded(clean: Perturbation) -> Perturbation {
    Perturbation {
        name: "a binary libtest ran its tests side by side in, proven single-threaded",
        engine: Some(vec![
            built(),
            verified(&[]),
            touch("baseline", &[0, 1]),
            touch("control", &[0, 1]),
        ]),
        ..clean
    }
}

/// `document` with its one concurrency row's exploration replaced whole by `explored`, since a merge would keep fields of the state it replaces.
fn explored_as(document: &mut Value, explored: Value) {
    if let Some(row) = document.pointer_mut("/concurrency/0/explored") {
        *row = explored;
    }
}

/// The defects planted for the reuse layer.
fn reuse_planted(clean: Perturbation) -> Vec<Perturbation> {
    vec![
        Perturbation {
            name: "a reused disposition that names this run as its source",
            document: with(json!({
                "mutants": [{ "reuse": { "reused": true, "source_run_id": RUN } }],
                "accounting": { "mutants": { "reused_killed": 1 } }
            })),
            ..clean.clone()
        },
        carried_past_its_killer(clean),
    ]
}

/// The defect planted for the reuse layer: a kill carried from an earlier tree by a target this run's route no longer reaches, which the engine refuses as `filter-differs`.
fn carried_past_its_killer(clean: Perturbation) -> Perturbation {
    let earlier = "20260905T090000Z-1a2b3c";
    let mut events = numbered(vec![json!({
        "type": "route",
        "route": {
            "mutant": KILLED, "granularity": "block", "fallback": null,
            "reaching": ["t1"], "tests": [], "discharged": [], "considered": [],
            "reused": earlier, "refused": null, "rule": "carried", "carry_refused": null
        }
    })]);
    events.extend(routes_for(&[(SURVIVED, "survived")]));
    Perturbation {
        name: "a kill carried from an earlier tree by a target the route no longer reaches",
        document: with(json!({
            "mutants": [{ "reuse": { "reused": true, "source_run_id": earlier } }],
            "accounting": { "mutants": { "reused_killed": 1 } }
        })),
        events: Some(numbered(events)),
        ..clean
    }
}

/// The engine recording of the clean specimen: its build, its single-threaded baseline, one control, and one knob's control that held.
fn clean_engine() -> Vec<Value> {
    vec![
        built(),
        verified(&["--test-threads=1"]),
        touch("baseline", &[0, 1]),
        touch("control", &[0, 1]),
        perturbed("survived", &[], &recorded_reach(&[0, 1])),
    ]
}

/// The planted defect of the concurrency layer: a delayed guard whose control ran past its bound, recorded as a sample that passed.
fn waited_yet_sampled(clean: Perturbation) -> Perturbation {
    let mut document = clean.document.clone();
    explored_as(
        &mut document,
        json!({ "state": "sampled", "asked": 1, "delayed": [0] }),
    );
    let mut delayed = perturbed("waited", &[], &json!({ "state": "not-read" }));
    merge(
        &mut delayed,
        json!({ "perturbed": { "perturbation": {
            "environment": [],
            "delay": { "site": 0, "pause_ms": 100 }
        } } }),
    );
    let mut engine = clean_engine();
    engine.push(delayed);
    Perturbation {
        name: "a delayed guard whose control ran past its bound, recorded as a sample that passed",
        document,
        engine: Some(engine),
        ..clean
    }
}

/// The second planted defect of the concurrency layer: a delayed guard whose control passed, recorded as a schedule that broke the binary.
fn passed_yet_broke(clean: Perturbation) -> Perturbation {
    let mut document = clean.document.clone();
    explored_as(
        &mut document,
        json!({
            "state": "broke",
            "site": 0,
            "path": "src/lib.rs",
            "line": 7,
            "failed": ["lib::works"],
            "rounds": 5
        }),
    );
    let mut delayed = perturbed("survived", &[], &recorded_reach(&[0, 1]));
    merge(
        &mut delayed,
        json!({ "perturbed": { "perturbation": {
            "environment": [],
            "delay": { "site": 0, "pause_ms": 100 }
        } } }),
    );
    let mut engine = clean_engine();
    engine.push(delayed);
    Perturbation {
        name: "a delayed guard whose control passed, recorded as a schedule that broke the binary",
        document,
        engine: Some(engine),
        ..clean
    }
}

/// Writes the runner recording `events` into `into`, and the engine recording `engine` under its one configured build.
fn lay_recording(
    into: &Path,
    events: &[Value],
    engine: Option<&[Value]>,
) -> Result<(), SpecimenError> {
    record_into(into, events, crate::schemas::Producer::Runner)?;
    lay_engine(into, engine)
}

/// Writes the engine recording `engine`, where there is one, under the one configured build of the recording directory `into`.
fn lay_engine(into: &Path, engine: Option<&[Value]>) -> Result<(), SpecimenError> {
    if let Some(engine) = engine {
        let namespace = into.join("builds").join("0000000000").join("engine");
        std::fs::create_dir_all(&namespace).map_err(|source| SpecimenError::Unwritable {
            path: namespace.display().to_string(),
            source,
        })?;
        record_into(&namespace, engine, crate::schemas::Producer::Engine)?;
    }
    Ok(())
}

/// The recording of a kill by a target the route's proof had discharged.
fn discharged_then_killed() -> Vec<Value> {
    vec![
        json!({
            "seq": 1, "timestamp": "2026-09-06T00:00:00Z", "elapsed_ms": 0,
            "type": "route",
            "route": {
                "mutant": KILLED, "granularity": "block", "fallback": null,
                "reaching": ["t1"],
                "discharged": [{ "target": "t2", "proof": "never-infected" }],
                "considered": [], "reused": null
            }
        }),
        json!({
            "seq": 2, "timestamp": "2026-09-06T00:00:01Z", "elapsed_ms": 1,
            "type": "mutant-exec",
            "mutant": {
                "mutant": KILLED, "target": "t2", "args": [], "outcome": "killed",
                "duration_ms": 5
            }
        }),
    ]
}

/// The defect planted for the knobs layer: a control a knob broke, recorded as stable.
fn broken_by_a_knob_called_stable(clean: Perturbation) -> Perturbation {
    Perturbation {
        name: "a control a knob broke, recorded as stable",
        engine: Some(vec![
            touch("baseline", &[0, 1]),
            touch("control", &[0, 1]),
            perturbed("killed", &["lib::works"], &json!({ "state": "not-read" })),
        ]),
        ..clean
    }
}

/// The defect planted for the repair layer: a disposition said to be run again against a target whose reach never moved.
fn repaired_where_nothing_moved(clean: Perturbation) -> Perturbation {
    let mut events = routes();
    events.push(json!({
        "type": "repair",
        "repair": {
            "mutant": SURVIVED, "target": "t1",
            "was": "survived", "now": "survived", "reached": "reached"
        }
    }));
    Perturbation {
        name: "a disposition run again against a target whose reach never moved",
        events: Some(events),
        ..clean
    }
}

/// The defect planted for the repair layer: a repair whose run nothing could observe, said to have survived.
fn unobserved_repair_called_a_survival() -> Perturbation {
    let mut events = routes();
    events.push(json!({
        "timestamp": "2026-09-06T00:00:02Z", "elapsed_ms": 2,
        "type": "mutant-exec",
        "mutant": {
            "mutant": SURVIVED, "target": TARGET, "args": [], "outcome": "inconclusive",
            "duration_ms": 5
        }
    }));
    events.push(json!({
        "type": "repair",
        "repair": {
            "mutant": SURVIVED, "target": TARGET,
            "was": "survived", "now": "survived", "reached": "reached"
        }
    }));
    let mut repair = touch("repair", &[1]);
    if let Some(record) = repair.get_mut("touch").and_then(Value::as_object_mut) {
        record.insert("mutant".to_owned(), json!("b".repeat(64)));
    }
    Perturbation {
        name: "a repair whose run nothing could observe, called a survival",
        document: with(json!({
            "drift": [moved(TARGET)],
            "mutants": [{}, { "catalog_index": 1 }],
            "limitations": [{ "name": "reach-moved", "detail": TARGET }]
        })),
        events: Some(events),
        engine: Some(vec![
            touch("baseline", &[0]),
            touch("control", &[0, 1]),
            repair,
        ]),
        shards: Vec::new(),
        outputs: Vec::new(),
    }
}

/// The outcomes the executions layer is planted a report lying about, told consistently.
const LIED_OUTCOMES: [&str; 7] = [
    "killed",
    "unconfirmed",
    "waited",
    "unreached",
    "errored",
    "equivalent",
    "declined",
];

/// The defect planted for `rule` of the merge layer, on the clean specimen measured in two shards and merged.
///
/// # Errors
/// [`SpecimenError::Incomplete`] where the clean sharded specimen cannot be built.
pub fn merge_plant(
    rule: crate::proofaudit::merge::MergeRule,
) -> Result<Perturbation, SpecimenError> {
    use crate::proofaudit::merge::MergeRule;
    let clean = sharded_clean()?;
    let mut document = clean.document.clone();
    let mut shards = clean.shards.clone();
    let name = match rule {
        MergeRule::Division => {
            shards.truncate(1);
            keep_first(&mut document, "/report/composition/sources");
            keep_first(&mut document, "/report/builds/0/parts");
            "half of a catalog merged alone, as though it were the whole"
        }
        MergeRule::Parts => {
            keep_first(&mut document, "/report/builds/0/parts");
            "a build holding one part of a catalog merged from two shards"
        }
        MergeRule::Placement => {
            merge(
                &mut document,
                json!({ "report": { "composition": { "sources": [
                    { "shard": { "index": 2 } },
                    { "shard": { "index": 1 } }
                ] } } }),
            );
            "a composition that places each shard where the other measured"
        }
        MergeRule::Agreement => {
            merge(&mut document, json!({ "report": { "run_kind": "full" } }));
            "a merge that says its shards measured the whole project when they measured a scope"
        }
        MergeRule::Builds => {
            merge(
                &mut document,
                json!({ "report": { "builds": [{ "name": "renamed" }] } }),
            );
            "a merged build named other than the one its shards measured"
        }
        MergeRule::Bytes => {
            merge(
                &mut document,
                json!({ "report": { "builds": [{ "parts": [{ "mutants": [{ "item": "another" }] }] }] } }),
            );
            "a merged part that is not the part its shard measured"
        }
        MergeRule::Identity => {
            merge(
                &mut document,
                json!({ "report": { "run_id": format!("{MERGED}-s1") } }),
            );
            "a merged run that names itself as one of its shards"
        }
        MergeRule::Models => {
            merge(
                &mut document,
                json!({ "report": { "model_completion": {
                    "kind": "verified",
                    "batch": { "owner": MERGED, "records": [] }
                } } }),
            );
            "a merge that completes a model batch no merge can"
        }
        MergeRule::Shards => forged_alike(&mut document, &mut shards),
    };
    Ok(Perturbation {
        name,
        document,
        shards,
        ..clean
    })
}

/// The defect planted for every rule of the merge layer; one that cannot be built is left out here and refused by name by the gate that holds each rule to its plant.
fn merge_plants() -> Vec<Perturbation> {
    crate::proofaudit::merge::MergeRule::ALL
        .into_iter()
        .filter_map(|rule| match merge_plant(rule) {
            Ok(plant) => Some(plant),
            Err(_unbuilt_is_refused_by_the_merge_gate) => None,
        })
        .collect()
}

/// A kill reported as a survivor in the first shard and in the merged part it became, alike, which only re-deciding the shard against its recording can see.
fn forged_alike(document: &mut Value, shards: &mut [Shard]) -> &'static str {
    let forged = json!({ "mutants": [{ "decision": {
        "outcome": "survived", "killed_by": null, "step_boundary": null
    } }] });
    if let Some(shard) = shards.first_mut() {
        merge(
            &mut shard.document,
            json!({ "report": { "builds": [{ "source": forged }] } }),
        );
    }
    merge(
        document,
        json!({ "report": { "builds": [{ "parts": [forged] }] } }),
    );
    "a kill reported as a survivor in a shard and in its merge alike"
}

/// `document` with the array at `pointer` cut to its first element.
fn keep_first(document: &mut Value, pointer: &str) {
    if let Some(Value::Array(items)) = document.pointer_mut(pointer) {
        items.truncate(1);
    }
}

impl Layer {
    /// The defects planted for this layer, each of which it must report as a violation.
    #[must_use]
    pub fn planted(self) -> Vec<Perturbation> {
        let clean = clean();
        match self {
            Self::Accounting => vec![Perturbation {
                name: "a mutant column the records contradict",
                document: with(json!({ "accounting": { "mutants": { "killed": 5 } } })),
                ..clean
            }],
            Self::Killers => vec![Perturbation {
                name: "a kill by a target the run never recorded",
                document: with(
                    json!({ "mutants": [{ "decision": { "killed_by": "pkg/test/nowhere" } }] }),
                ),
                ..clean
            }],
            Self::Findings => vec![Perturbation {
                name: "a survivor no finding names",
                document: with(json!({ "findings": [] })),
                ..clean
            }],
            Self::Acceptances => vec![Perturbation {
                name: "an unmatched acceptance the whole catalog resolves",
                document: with(json!({
                    "findings": [{
                        "kind": "unmatched-acceptance",
                        "subject": "aaaa",
                        "detail": "no single mutant",
                        "position": null
                    }]
                })),
                ..clean
            }],
            Self::Reuse => reuse_planted(clean),
            Self::Proofs => vec![Perturbation {
                name: "a kill by a target a proof discharged",
                events: Some(discharged_then_killed()),
                ..clean
            }],
            Self::Hollow => vec![Perturbation {
                name: "a hollow target the report does not name",
                events: Some(never_noticed()),
                ..clean
            }],
            Self::Wire => vec![Perturbation {
                name: "a question nothing noticed that the report does not name",
                events: Some(vec![went_past(), was_put(ASKED, "unnoticed")]),
                ..clean
            }],
            Self::Model => vec![Perturbation {
                name: "a verified-v1 survivor with no model record",
                document: with(json!({ "contract": "verified-v1" })),
                ..clean
            }],
            Self::Faults => faults_and_besides_planted(&clean),
            Self::Dimensions => dimensions_planted(clean),
            Self::Crashes => crashes_planted(&clean),
            Self::Merge => merge_plants(),
            Self::Drift => vec![Perturbation {
                name: "a control that reached a site its baseline never did, recorded as held",
                engine: Some(vec![touch("baseline", &[0]), touch("control", &[0, 1])]),
                shards: Vec::new(),
                ..clean
            }],
            Self::Repair => vec![
                unobserved_repair_called_a_survival(),
                repaired_where_nothing_moved(clean),
            ],
            Self::Knobs => knobs_planted(clean),
            Self::Concurrency => concurrency_planted(&clean),
            Self::Soundness => soundness_planted(&clean),
            Self::Executions => LIED_OUTCOMES.into_iter().filter_map(lie).collect(),
        }
    }
}

/// The lies about knobs the knobs layer must refuse.
fn knobs_planted(clean: Perturbation) -> Vec<Perturbation> {
    vec![
        broken_by_a_knob_called_stable(clean.clone()),
        Perturbation {
            name: "a whole run with no row for a knob the contract puts",
            document: with(json!({ "contract": "whole-v1" })),
            ..clean
        },
    ]
}

/// A recorded run of the interpreter over the suite that ended with `code` and said `said`, kept at `output/1.txt` with its size and digest.
fn interpreted(code: i64, said: &str) -> Value {
    use sha2::Digest as _;
    json!({
        "type": "exec",
        "exec": {
            "argv": ["cargo", "+nightly", "miri", "test", "--workspace"],
            "dir": null, "env_names": [], "timeout_ms": null,
            "stopped": { "kind": "exited", "exit": { "kind": "code", "value": code } },
            "duration_ms": 1, "output_bytes": said.len(),
            "output_sha256": hex::encode(sha2::Sha256::digest(said.as_bytes())),
            "output_truncated": false, "output_path": "output/1.txt", "error": null
        }
    })
}

/// What cargo-miri prints when it cannot start the test binary it built.
const SETUP_FAILED: &str =
    "thread 'main' panicked at cargo-miri/src/util.rs:132:9:\nfailed to run `cd /gone`\n";

/// What Miri prints when every test it ran passed.
const PASSED: &str = "     Running unittests src/lib.rs (x)\n\nrunning 1 test\ntest t ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";

/// What Miri prints when a test failed and its captured output quotes the words of undefined behaviour.
const QUOTED_UNDEFINED: &str = "     Running unittests src/lib.rs (x)\n\nrunning 1 test\ntest t ... FAILED\n\nfailures:\n\n---- t stdout ----\nerror: Undefined Behavior: quoted by the test\n\nfailures:\n    t\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";

/// The report saying the suite was interpreted and a test failed under the interpreter.
fn failing_under_the_interpreter() -> Value {
    with(json!({
        "accounting": { "soundness": { "executed": true } },
        "findings": [{}, {
            "kind": "failing-test",
            "subject": "soundness",
            "detail": "a test fails under the interpreter that passes without it",
            "position": null
        }]
    }))
}

/// A recorded run of the interpreter that ran out of time, whose kept output is at `output/1.txt`.
fn interpreted_until_the_clock(said: &str) -> Value {
    use sha2::Digest as _;
    json!({
        "type": "exec",
        "exec": {
            "argv": ["cargo", "+nightly", "miri", "test", "--workspace"],
            "dir": null, "env_names": [], "timeout_ms": 1,
            "stopped": { "kind": "timed-out", "raised": null },
            "duration_ms": 1, "output_bytes": said.len(),
            "output_sha256": hex::encode(sha2::Sha256::digest(said.as_bytes())),
            "output_truncated": false, "output_path": "output/1.txt", "error": null
        }
    })
}

/// The lies about soundness the soundness layer must refuse.
fn soundness_planted(clean: &Perturbation) -> Vec<Perturbation> {
    let with_run = |code: i64, said: &str| {
        let mut events = routes();
        events.push(interpreted(code, said));
        Some(events)
    };
    vec![
        Perturbation {
            name: "a suite said to be interpreted with no run of the interpreter recorded",
            document: with(json!({ "accounting": { "soundness": { "executed": true } } })),
            ..clean.clone()
        },
        Perturbation {
            name: "a test failing under an interpreter that ran no test",
            document: failing_under_the_interpreter(),
            events: with_run(101, SETUP_FAILED),
            outputs: vec![("output/1.txt", SETUP_FAILED)],
            ..clean.clone()
        },
        Perturbation {
            name: "undefined behaviour read from a failing test's captured output",
            document: with(json!({
                "accounting": { "soundness": { "executed": true } },
                "findings": [{}, {
                    "kind": "undefined-behaviour",
                    "subject": "soundness",
                    "detail": "error: Undefined Behavior: quoted by the test",
                    "position": null
                }]
            })),
            events: with_run(101, QUOTED_UNDEFINED),
            outputs: vec![("output/1.txt", QUOTED_UNDEFINED)],
            ..clean.clone()
        },
        Perturbation {
            name: "an interpreter that ran out of time with no limitation stated",
            document: with(json!({ "accounting": { "soundness": { "executed": false } } })),
            events: Some({
                let mut events = routes();
                events.push(interpreted_until_the_clock("running 1 test\n"));
                events
            }),
            outputs: vec![("output/1.txt", "running 1 test\n")],
            ..clean.clone()
        },
        Perturbation {
            name: "an interpreter's kept output rewritten after the run",
            document: failing_under_the_interpreter(),
            events: with_run(0, PASSED),
            outputs: vec![(
                "output/1.txt",
                "test result: FAILED. 0 passed; 1 failed; 0 ignored\n",
            )],
            ..clean.clone()
        },
    ]
}

/// The lies about threads and schedules the concurrency layer must refuse.
fn concurrency_planted(clean: &Perturbation) -> Vec<Perturbation> {
    vec![
        loose_yet_single_threaded(clean.clone()),
        unrecorded_threads(clean.clone()),
        parallel_yet_single_threaded(clean.clone()),
        waited_yet_sampled(clean.clone()),
        passed_yet_broke(clean.clone()),
    ]
}

/// How one outcome is told about the specimen's survivor: the lie's name, whether a test is named, the column that counts it, and the finding it owes.
type Telling = (
    &'static str,
    bool,
    Option<&'static str>,
    Option<&'static str>,
);

/// How [`lie`] tells `outcome`, or nothing where the schema has no such outcome.
fn telling(outcome: &str) -> Option<Telling> {
    Some(match outcome {
        "killed" => ("a survivor reported as killed", true, Some("killed"), None),
        "unconfirmed" => (
            "a survivor reported as unconfirmed",
            true,
            None,
            Some("failing-test"),
        ),
        "errored" => (
            "a survivor reported as errored",
            true,
            None,
            Some("failing-test"),
        ),
        "declined" => (
            "a survivor reported as declined",
            true,
            None,
            Some("not-measured"),
        ),
        "waited" => (
            "a survivor reported as waited",
            true,
            Some("waited"),
            Some("waited-mutant"),
        ),
        "step-limit-reached" => (
            "a survivor reported as stopped at its step limit",
            true,
            Some("step_limit_reached"),
            Some("step-limit-reached-mutant"),
        ),
        "unreached" => (
            "a survivor reported as unreached",
            false,
            Some("unreached"),
            Some("surviving-mutant"),
        ),
        "equivalent" => (
            "a survivor reported as equivalent",
            false,
            Some("equivalent"),
            None,
        ),
        "compile-rejected" => (
            "a survivor reported as compile-rejected",
            false,
            Some("rejected"),
            None,
        ),
        "model-noticed" => (
            "a survivor reported as model-noticed",
            false,
            Some("model_noticed"),
            None,
        ),
        "model-proved" => (
            "a survivor reported as model-proved",
            false,
            Some("model_proved"),
            None,
        ),
        _ => return None,
    })
}

/// The clean run with its kill reported as a survivor, its columns and findings made to agree.
fn kill_reported_as_survivor() -> Perturbation {
    let clean = clean();
    let mut document = clean.document.clone();
    merge(
        &mut document,
        json!({
            "accounting": { "mutants": { "killed": 0, "survived": 2 } },
            "mutants": [{ "decision": { "outcome": "survived", "killed_by": null } }],
            "findings": [
                {
                    "kind": "surviving-mutant",
                    "subject": SURVIVED,
                    "detail": "no test noticed return-ok-default@1 at src/lib.rs:11",
                    "position": null
                },
                {
                    "kind": "surviving-mutant",
                    "subject": KILLED,
                    "detail": "no test noticed negate-condition@1 at src/lib.rs:7",
                    "position": null
                }
            ]
        }),
    );
    Perturbation {
        name: "a kill reported as a survivor",
        document,
        ..clean
    }
}

/// The clean run with its survivor reported as `outcome`, every column, finding and the verdict made to agree, and the recording left saying it survived; nothing where the schema has no such outcome.
///
/// A lie told consistently is the one an audit that only counts cannot see: the report contradicts nothing but the executions it rests on.
#[must_use]
pub fn lie(outcome: &str) -> Option<Perturbation> {
    if outcome == "survived" {
        return Some(kill_reported_as_survivor());
    }
    let (name, noticed, column, finding) = telling(outcome)?;
    let mut columns = serde_json::Map::new();
    columns.insert("survived".to_owned(), json!(0));
    if let Some(column) = column {
        columns.insert(
            column.to_owned(),
            json!(if column == "killed" { 2 } else { 1 }),
        );
    }
    if matches!(outcome, "unreached" | "equivalent" | "compile-rejected") {
        columns.insert("executed".to_owned(), json!(1));
    }
    let clean = clean();
    let mut document = clean.document.clone();
    merge(
        &mut document,
        json!({
            "verdict": if finding.is_some() { "INSUFFICIENT" } else { "SCOPE_ASSURED" },
            "accounting": { "mutants": columns },
            "mutants": [{}, {
                "decision": {
                    "outcome": outcome,
                    "killed_by": if noticed { json!(TARGET) } else { json!(null) },
                    "step_boundary": if outcome == "step-limit-reached" {
                        json!({ "limit": 1, "observed": 2 })
                    } else {
                        json!(null)
                    }
                }
            }],
            "findings": []
        }),
    );
    if let Some(kind) = finding {
        merge(
            &mut document,
            json!({ "findings": [{
                "kind": kind,
                "subject": SURVIVED,
                "detail": "planted",
                "position": null
            }] }),
        );
    }
    Some(Perturbation {
        name,
        document,
        ..clean
    })
}
