// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The audit's private, exact reading of the engine report wire shape.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::Error as _;
use serde_json::Value;

use super::{
    Claim, ClaimStanding, Finding, FindingKind, Granularity, NotRunReason, Outcome, Refusal,
    Report, RouteDecision, Row, StepNotice,
};

/// Reads a nullable value while leaving absence for serde to reject at the enclosing struct boundary.
pub(super) fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// The selection is compared between shards as a closed value, rather than being round-tripped through an untyped JSON tree.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Selection {
    tier: String,
    operators: Vec<String>,
    include: Vec<String>,
    exclude: Vec<String>,
    packages: Vec<String>,
    build: Vec<String>,
    #[serde(deserialize_with = "required_option")]
    mutant_steps: Option<u64>,
}

impl Selection {
    pub(super) const fn mutant_steps(&self) -> Option<u64> {
        self.mutant_steps
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Document {
    #[serde(rename = "document_type")]
    kind: String,
    schema_version: u64,
    tool_version: String,
    run: Run,
    workspace: Workspace,
    selection: Selection,
    accounting: Accounting,
    #[serde(deserialize_with = "required_option")]
    score: Option<Score>,
    targets: Vec<Target>,
    #[serde(rename = "established_tests")]
    _established_tests: u64,
    mutants: Vec<Mutant>,
    rejections: Vec<Rejection>,
    #[serde(rename = "skips")]
    _skips: Vec<Skip>,
    expectations: Vec<Expectation>,
    findings: Vec<FindingWire>,
}

impl Document {
    pub(super) fn document_type(&self) -> &str {
        &self.kind
    }

    pub(super) const fn schema_version(&self) -> u64 {
        self.schema_version
    }

    pub(super) fn into_report(self) -> Report {
        let Self {
            kind: _,
            schema_version: _,
            tool_version,
            run,
            workspace,
            selection,
            accounting,
            score,
            targets,
            _established_tests: _,
            mutants,
            rejections,
            _skips: skips,
            expectations,
            findings,
        } = self;
        let Run {
            id: run_id,
            _started_at: _,
            _finished_at: _,
            _duration_ms: _,
            interrupted,
            exit_code,
            _shard: _,
            _jobs: _,
        } = run;
        let Workspace {
            _root_name: _,
            _toolchain: _,
            digest: workspace_digest,
            catalog_digest,
            _platform: _,
        } = workspace;
        let mutant_steps = selection.mutant_steps();
        let columns = accounting.columns();
        let score = score.map(Score::tuple);
        let targets = targets.into_iter().map(Target::id).collect();
        let mutants = mutants.into_iter().map(Mutant::row).collect();
        let rejections = rejections.into_iter().map(Rejection::refusal).collect();
        let expectations = expectations.into_iter().map(Expectation::claim).collect();
        let findings = findings.into_iter().map(FindingWire::finding).collect();
        let skip_counts = skips.into_iter().map(Skip::count).collect();
        Report {
            run_id,
            targets,
            tool_version,
            workspace_digest,
            catalog_digest,
            selection,
            mutant_steps,
            interrupted,
            exit_code,
            columns,
            score,
            mutants,
            rejections,
            expectations,
            findings,
            skip_counts,
        }
    }
}

pub(super) fn decode(text: &str) -> Result<Document, serde_json::Error> {
    crate::strictjson::decode_str(text)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Run {
    id: String,
    #[serde(rename = "started_at")]
    _started_at: String,
    #[serde(rename = "finished_at")]
    _finished_at: String,
    #[serde(rename = "duration_ms")]
    _duration_ms: u64,
    interrupted: bool,
    exit_code: u8,
    #[serde(rename = "shard")]
    #[serde(deserialize_with = "required_option")]
    _shard: Option<String>,
    #[serde(rename = "jobs")]
    _jobs: Jobs,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Jobs {
    #[serde(rename = "asked")]
    _asked: String,
    #[serde(rename = "used")]
    _used: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Workspace {
    #[serde(rename = "root_name")]
    _root_name: String,
    #[serde(rename = "toolchain")]
    _toolchain: String,
    #[serde(rename = "workspace_digest")]
    digest: String,
    catalog_digest: String,
    #[serde(rename = "platform")]
    _platform: Platform,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Platform {
    #[serde(rename = "os")]
    _os: String,
    #[serde(rename = "arch")]
    _arch: String,
    #[serde(rename = "target")]
    _target: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Accounting {
    cataloged: u64,
    refused: u64,
    skipped: u64,
    executed: u64,
    killed: u64,
    survived: u64,
    step_limit_reached: u64,
    waited: u64,
    inconclusive: u64,
    errored: u64,
    not_run: u64,
    unreached: u64,
    discharged: u64,
    expected: u64,
}

impl Accounting {
    fn columns(self) -> BTreeMap<String, u64> {
        BTreeMap::from([
            ("cataloged".to_owned(), self.cataloged),
            ("refused".to_owned(), self.refused),
            ("skipped".to_owned(), self.skipped),
            ("executed".to_owned(), self.executed),
            ("killed".to_owned(), self.killed),
            ("survived".to_owned(), self.survived),
            ("step_limit_reached".to_owned(), self.step_limit_reached),
            ("waited".to_owned(), self.waited),
            ("inconclusive".to_owned(), self.inconclusive),
            ("errored".to_owned(), self.errored),
            ("not_run".to_owned(), self.not_run),
            ("unreached".to_owned(), self.unreached),
            ("discharged".to_owned(), self.discharged),
            ("expected".to_owned(), self.expected),
        ])
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Score {
    detected: u64,
    decided: u64,
    value: f64,
}

impl Score {
    const fn tuple(self) -> (u64, u64, f64) {
        (self.detected, self.decided, self.value)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    id: String,
    #[serde(rename = "kind")]
    _kind: String,
    #[serde(rename = "harness")]
    _harness: bool,
    #[serde(rename = "tests")]
    _tests: u64,
    #[serde(rename = "limitations")]
    _limitations: Vec<String>,
}

impl Target {
    fn id(self) -> String {
        self.id
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is an independent fact the published report row states, and the wire shape is the schema's"
)]
struct Mutant {
    index: u64,
    id: String,
    display_id: String,
    path: String,
    #[serde(rename = "package")]
    _package: String,
    #[serde(rename = "family")]
    _family: String,
    rule: String,
    item: String,
    rule_version: u64,
    #[serde(rename = "line")]
    _line: u64,
    #[serde(rename = "column")]
    _column: u64,
    start_byte: u64,
    end_byte: u64,
    source_digest: String,
    original: String,
    replacement: String,
    outcome: Outcome,
    #[serde(deserialize_with = "required_option")]
    step_notice: Option<StepNoticeWire>,
    target: String,
    #[serde(rename = "exit_code")]
    _exit_code: i64,
    #[serde(rename = "duration_ms")]
    _duration_ms: u64,
    #[serde(deserialize_with = "required_option")]
    tests_run: Option<u64>,
    killed_by: Vec<String>,
    #[serde(rename = "signal")]
    #[serde(deserialize_with = "required_option")]
    _signal: Option<i64>,
    retried: bool,
    lingered: bool,
    #[serde(deserialize_with = "required_option")]
    not_run_reason: Option<NotRunReason>,
    #[serde(deserialize_with = "required_option")]
    route: Option<Route>,
    #[serde(rename = "identical")]
    _identical: CodegenIdentity,
    expected: bool,
    unreached: bool,
    #[serde(deserialize_with = "required_option")]
    source_run_id: Option<String>,
}

/// What the engine's compiler-artifact comparison established about one mutant, as the published run report spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum CodegenIdentity {
    /// Nothing was compared.
    NotMeasured,
    /// The mutant compiles to the same code.
    Identical,
    /// It compiles to different code.
    Different,
    /// The comparison could not be made.
    NotEstablished,
}

impl Mutant {
    fn row(self) -> Row {
        let Self {
            index,
            id,
            display_id,
            path,
            _package: _,
            _family: _,
            rule,
            item,
            rule_version,
            _line: _,
            _column: _,
            start_byte,
            end_byte,
            source_digest,
            original,
            replacement,
            outcome,
            step_notice,
            target,
            _exit_code: _,
            _duration_ms: _,
            tests_run,
            killed_by,
            _signal: _,
            retried,
            lingered,
            not_run_reason,
            route,
            _identical: _,
            expected,
            unreached,
            source_run_id,
        } = self;
        let route = route.map(Route::decision);
        let step_notice = step_notice.map(StepNoticeWire::notice);
        Row {
            index,
            route,
            id,
            display_id,
            path,
            rule,
            rule_version,
            start_byte,
            end_byte,
            source_digest,
            original,
            replacement,
            outcome,
            step_notice,
            target,
            tests_run,
            killed_by,
            item,
            retried,
            lingered,
            expected,
            unreached,
            not_run_reason,
            source_run_id,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepNoticeWire {
    nonce: String,
    catalog: String,
    mutant: String,
    limit: u64,
    observed: u64,
}

impl StepNoticeWire {
    fn notice(self) -> StepNotice {
        StepNotice {
            nonce: self.nonce,
            catalog: self.catalog,
            mutant: self.mutant,
            limit: self.limit,
            observed: self.observed,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Route {
    granularity: Granularity,
    #[serde(deserialize_with = "required_option")]
    fallback: Option<String>,
    reaching: Vec<String>,
    discharged: Vec<Discharge>,
    executed: Vec<String>,
    tests: BTreeMap<String, Vec<String>>,
}

impl Route {
    fn decision(self) -> RouteDecision {
        let Self {
            granularity,
            fallback,
            reaching,
            discharged,
            executed,
            tests,
        } = self;
        let tests = tests
            .into_iter()
            .map(|(target, tests)| (target, tests.into_iter().collect()))
            .collect();
        let discharged = discharged.into_iter().map(Discharge::pair).collect();
        RouteDecision {
            granularity,
            fallback,
            tests,
            reaching,
            executed,
            discharged,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Discharge {
    target: String,
    proof: String,
}

impl Discharge {
    fn pair(self) -> (String, String) {
        (self.target, self.proof)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rejection {
    index: u64,
    #[serde(rename = "id")]
    _id: String,
    display_id: String,
    #[serde(rename = "path")]
    _path: String,
    #[serde(rename = "rule")]
    _rule: String,
    #[serde(rename = "code")]
    #[serde(deserialize_with = "required_option")]
    _code: Option<String>,
    #[serde(rename = "diagnostic")]
    _diagnostic: String,
    #[serde(rename = "isolated")]
    _isolated: bool,
}

impl Rejection {
    fn refusal(self) -> Refusal {
        let Self {
            index,
            _id: _,
            display_id,
            _path: _,
            _rule: _,
            _code: _,
            _diagnostic: _,
            _isolated: _,
        } = self;
        Refusal { index, display_id }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Skip {
    #[serde(rename = "reason")]
    _reason: String,
    #[serde(rename = "path")]
    _path: String,
    #[serde(rename = "count")]
    count: u64,
    #[serde(rename = "explanation")]
    _explanation: String,
}

impl Skip {
    fn count(self) -> u64 {
        self.count
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expectation {
    id: String,
    #[serde(rename = "locator")]
    #[serde(deserialize_with = "required_option")]
    _locator: Option<Locator>,
    #[serde(rename = "reason")]
    _reason: String,
    #[serde(rename = "outcome")]
    _outcome: String,
    #[serde(deserialize_with = "required_option")]
    mutant: Option<String>,
    #[serde(rename = "covered")]
    #[serde(deserialize_with = "required_option")]
    _covered: Option<u64>,
    standing: ClaimStanding,
    #[serde(rename = "actual")]
    #[serde(deserialize_with = "required_option")]
    _actual: Option<String>,
    #[serde(rename = "why")]
    #[serde(deserialize_with = "required_option")]
    _why: Option<String>,
}

impl Expectation {
    fn claim(self) -> Claim {
        let Self {
            id,
            _locator: _,
            _reason: _,
            _outcome: _,
            mutant,
            _covered: _,
            standing,
            _actual: _,
            _why: _,
        } = self;
        Claim {
            id,
            mutant,
            standing,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Locator {
    #[serde(rename = "path")]
    _path: String,
    #[serde(rename = "item")]
    _item: String,
    #[serde(rename = "rule")]
    _rule: String,
    #[serde(rename = "original")]
    _original: String,
    #[serde(rename = "line")]
    #[serde(deserialize_with = "required_option")]
    _line: Option<u64>,
    #[serde(rename = "count")]
    #[serde(deserialize_with = "required_option")]
    _count: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FindingWire {
    kind: FindingKind,
    #[serde(deserialize_with = "required_option")]
    mutant: Option<String>,
    #[serde(rename = "detail")]
    _detail: String,
}

impl FindingWire {
    fn finding(self) -> Finding {
        let Self {
            kind,
            mutant,
            _detail: _,
        } = self;
        Finding { kind, mutant }
    }
}

/// Proves that a reached-v1 document has its exact owned shape before the independent audit reads facts out of its JSON value.
pub(super) fn validate_reached(value: &Value) -> Result<(), serde_json::Error> {
    match serde_json::from_value::<ReachedEvidence>(value.clone()) {
        Ok(document) => {
            drop(document);
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Proves that a touched-v1 document has its exact owned shape before the independent audit interprets absence as evidence.
pub(super) fn validate_touched(value: &Value) -> Result<(), serde_json::Error> {
    let document = serde_json::from_value::<TouchedEvidence>(value.clone())?;
    reject_touched_nulls(value)?;
    drop(document);
    Ok(())
}

fn reject_touched_nulls(value: &Value) -> Result<(), serde_json::Error> {
    let Some(root) = value.as_object() else {
        return Err(serde_json::Error::custom(
            "a touched document must be an object",
        ));
    };
    reject_null(root.get("narrowing"), "touched narrowing")?;
    reject_null(root.get("items"), "the touched item catalog")?;
    let Some(targets) = root.get("targets").and_then(Value::as_object) else {
        return Ok(());
    };
    for target in targets.values() {
        let Some(target) = target.as_object() else {
            continue;
        };
        for kind in ["reached", "bodies", "infected", "entered"] {
            let seen = target.get(kind);
            reject_null(seen, "a touched target record")?;
            let Some(seen) = seen.and_then(Value::as_object) else {
                continue;
            };
            reject_null(seen.get("tests"), "a touched test map")?;
            reject_null(seen.get("loose"), "a touched loose-site list")?;
        }
    }
    Ok(())
}

fn reject_null(value: Option<&Value>, what: &str) -> Result<(), serde_json::Error> {
    if value.is_some_and(Value::is_null) {
        return Err(serde_json::Error::custom(format!(
            "{what} may be absent but may not be null"
        )));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReachedEvidence {
    #[serde(rename = "targets")]
    _targets: BTreeMap<String, Vec<CoverageBlock>>,
    #[serde(rename = "instrumented")]
    _instrumented: Vec<CoverageBlock>,
    #[serde(rename = "limitations")]
    _limitations: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CoverageBlock {
    #[serde(rename = "file")]
    _file: String,
    #[serde(rename = "start")]
    _start: CoveragePoint,
    #[serde(rename = "end")]
    _end: CoveragePoint,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CoveragePoint {
    #[serde(rename = "line")]
    _line: u64,
    #[serde(rename = "column")]
    _column: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TouchedEvidence {
    #[serde(rename = "targets")]
    _targets: BTreeMap<String, TouchedTarget>,
    #[serde(rename = "limitations")]
    _limitations: Vec<String>,
    #[serde(rename = "narrowing")]
    _narrowing: Option<TouchedNarrowing>,
    #[serde(rename = "items")]
    _items: Option<Vec<TouchedItem>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TouchedItem {
    #[serde(rename = "index")]
    _index: u64,
    #[serde(rename = "package")]
    _package: String,
    #[serde(rename = "path")]
    _path: String,
    #[serde(rename = "name")]
    _name: String,
    #[serde(rename = "span")]
    _span: TouchedSpan,
    #[serde(rename = "body")]
    _body: TouchedSpan,
    #[serde(rename = "measurable")]
    _measurable: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TouchedSpan {
    #[serde(rename = "start")]
    _start: u64,
    #[serde(rename = "end")]
    _end: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TouchedNarrowing {
    #[serde(rename = "compared")]
    _compared: Option<Vec<u64>>,
    #[serde(rename = "bodies")]
    _bodies: Option<BTreeMap<u64, u64>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TouchedTarget {
    #[serde(rename = "reached")]
    _reached: Option<TouchedSeen>,
    #[serde(rename = "bodies")]
    _bodies: Option<TouchedSeen>,
    #[serde(rename = "infected")]
    _infected: Option<TouchedSeen>,
    #[serde(rename = "entered")]
    _entered: Option<TouchedSeen>,
    #[serde(rename = "ran")]
    _ran: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TouchedSeen {
    #[serde(rename = "tests")]
    _tests: Option<BTreeMap<String, Vec<u64>>>,
    #[serde(rename = "loose")]
    _loose: Option<Vec<u64>>,
}
