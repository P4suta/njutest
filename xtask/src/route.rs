// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading how a run routed, from either recording that writes it down.

use serde_json::Value;

/// Every granularity a route can be decided at.
pub const GRANULARITIES: [&str; 5] = ["all", "block", "test", "discharged", "unreached"];

/// The granularities the engine writes, which are [`GRANULARITIES`].
pub const ENGINE_GRANULARITIES: [&str; 5] = GRANULARITIES;

/// The granularities the runner writes, which are [`GRANULARITIES`].
pub const RUNNER_GRANULARITIES: [&str; 5] = GRANULARITIES;

/// One target a proof removed, and the proof that removed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discharge {
    /// The target.
    pub target: String,
    /// The proof's name.
    pub proof: String,
}

/// One routing decision, as either producer records it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Route {
    /// The mutant, as a person types it.
    pub mutant: String,
    /// Its dense catalog index, which only the engine records.
    pub index: Option<u64>,
    /// How narrow the decision was, in the producer's own words.
    pub granularity: String,
    /// What widened the route back, when something did.
    pub fallback: Option<String>,
    /// The targets that could notice the mutation.
    pub reaching: Vec<String>,
    /// The targets a proof removed.
    pub discharged: Vec<Discharge>,
    /// The targets that ran, which only the engine records.
    pub executed: Vec<String>,
    /// The targets that were measured, asked, and did not reach the mutation.
    pub considered: Vec<String>,
    /// The run this disposition was read back from.
    pub reused: Option<String>,
    /// Why the answer an earlier run left was not the one used, when there was a store of them to ask.
    pub refused: Option<String>,
}

impl Route {
    /// Whether this decision is about the mutant either identity names.
    #[must_use]
    pub fn names(&self, id: &str, display_id: &str) -> bool {
        self.mutant == id || (!display_id.is_empty() && self.mutant == display_id)
    }

    /// Whether a proof removed this target.
    #[must_use]
    pub fn discharges(&self, target: &str) -> bool {
        self.discharged
            .iter()
            .any(|discharge| discharge.target == target)
    }
}

/// One mutant execution, as either producer records it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Exec {
    /// The mutant, as a person types it, or its full identity when that is all the producer wrote.
    pub mutant: String,
    /// Its dense catalog index, which only the engine records.
    pub index: Option<u64>,
    /// The target it ran against.
    pub target: String,
    /// What the execution established.
    pub outcome: String,
    /// How many tests ran, when the harness said.
    pub tests_run: Option<u64>,
    /// How long it took.
    pub duration_ms: Option<u64>,
    /// Whether the machine was given to this execution alone.
    pub alone: Option<bool>,
}

impl Exec {
    /// Whether this execution is about the mutant either identity names.
    #[must_use]
    pub fn names(&self, id: &str, display_id: &str) -> bool {
        self.mutant == id || (!display_id.is_empty() && self.mutant == display_id)
    }
}

/// The routes and the executions of one recording, in the order they were written.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Routing {
    /// Every routing decision.
    pub routes: Vec<Route>,
    /// Every mutant execution.
    pub execs: Vec<Exec>,
}

impl Routing {
    /// The route of one mutant, by the name either producer wrote.
    #[must_use]
    pub fn route(&self, mutant: &str) -> Option<&Route> {
        self.route_of(mutant, mutant)
    }

    /// The route of one mutant a caller holds both identities of.
    #[must_use]
    pub fn route_of(&self, id: &str, display_id: &str) -> Option<&Route> {
        self.routes.iter().find(|route| route.names(id, display_id))
    }

    /// Every execution of one mutant, in the order they ran.
    pub fn execs_of<'a>(&'a self, mutant: &'a str) -> impl Iterator<Item = &'a Exec> {
        self.execs_for(mutant, mutant)
    }

    /// Every execution of one mutant a caller holds both identities of.
    pub fn execs_for<'a>(
        &'a self,
        id: &'a str,
        display_id: &'a str,
    ) -> impl Iterator<Item = &'a Exec> {
        self.execs
            .iter()
            .filter(move |exec| exec.names(id, display_id))
    }
}

/// Reads the routes and executions out of a recording, ignoring every line that is neither.
#[must_use]
pub fn read(recorded: &str) -> Routing {
    let mut routing = Routing::default();
    for line in recorded.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match text(&event, "type").as_deref() {
            Some("route") => {
                if let Some(record) = event.get("route") {
                    routing.routes.push(route(record));
                }
            }
            Some("mutant-exec") => {
                if let Some(record) = event.get("mutant") {
                    routing.execs.push(exec(record));
                }
            }
            _ => {}
        }
    }
    routing
}

/// One route record, from whichever producer wrote it.
fn route(record: &Value) -> Route {
    Route {
        mutant: named(record),
        index: number(record, "index"),
        granularity: text(record, "granularity").unwrap_or_default(),
        fallback: text(record, "fallback"),
        reaching: strings(record, "reaching"),
        discharged: discharges(record),
        executed: strings(record, "executed"),
        considered: strings(record, "considered"),
        reused: text(record, "reused"),
        refused: text(record, "refused"),
    }
}

/// One execution record, from whichever producer wrote it.
fn exec(record: &Value) -> Exec {
    Exec {
        mutant: named(record),
        index: number(record, "index"),
        target: text(record, "target").unwrap_or_default(),
        outcome: text(record, "outcome").unwrap_or_default(),
        tests_run: number(record, "tests_run"),
        duration_ms: number(record, "duration_ms"),
        alone: record.get("alone").and_then(Value::as_bool),
    }
}

/// The mutant a record is about: the runner writes `mutant`, the engine writes `id`.
fn named(record: &Value) -> String {
    text(record, "mutant")
        .or_else(|| text(record, "id"))
        .unwrap_or_default()
}

/// Every target a proof removed, with the proof.
fn discharges(record: &Value) -> Vec<Discharge> {
    record
        .get("discharged")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| Discharge {
                    target: text(entry, "target").unwrap_or_default(),
                    proof: text(entry, "proof").unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One string field, absent when it is absent or null.
fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_owned)
}

/// One number field, absent when it is absent or null.
fn number(value: &Value, key: &str) -> Option<u64> {
    value.get(key)?.as_u64()
}

/// One array of strings, empty when it is absent.
fn strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
