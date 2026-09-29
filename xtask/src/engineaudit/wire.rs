// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The audit's private, exact reading of the engine report wire shape.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::Error as _;
use serde_json::Value;

use super::{
    Claim, ClaimStanding, Decline, Finding, FindingKind, Granularity, Named, NotRunReason, Outcome,
    Refusal, Report, RouteDecision, Row, StepNotice,
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
    facts: Vec<String>,
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
            facts,
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
            facts,
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
    unproven: u64,
    step_limit_reached: u64,
    waited: u64,
    inconclusive: u64,
    errored: u64,
    not_run: u64,
    unreached: u64,
    declined: u64,
    expected: u64,
    unproven_killed: u64,
    unproven_survived: u64,
    unproven_unreached: u64,
    unproven_discharged: u64,
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
            ("unproven".to_owned(), self.unproven),
            ("step_limit_reached".to_owned(), self.step_limit_reached),
            ("waited".to_owned(), self.waited),
            ("inconclusive".to_owned(), self.inconclusive),
            ("errored".to_owned(), self.errored),
            ("not_run".to_owned(), self.not_run),
            ("unreached".to_owned(), self.unreached),
            ("declined".to_owned(), self.declined),
            ("expected".to_owned(), self.expected),
            ("unproven_killed".to_owned(), self.unproven_killed),
            ("unproven_survived".to_owned(), self.unproven_survived),
            ("unproven_unreached".to_owned(), self.unproven_unreached),
            ("unproven_discharged".to_owned(), self.unproven_discharged),
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
    #[serde(rename = "sealed")]
    _sealed: SealedTarget,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SealedTarget {
    #[serde(rename = "state")]
    _state: String,
    #[serde(rename = "remedy", deserialize_with = "required_option")]
    _remedy: Option<String>,
    #[serde(rename = "uncontrolled")]
    _uncontrolled: Vec<UncontrolledTest>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UncontrolledTest {
    #[serde(rename = "test")]
    _test: String,
    #[serde(rename = "reason")]
    _reason: String,
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
    declined: Vec<Decline>,
    #[serde(deserialize_with = "required_option")]
    route: Option<Route>,
    #[serde(rename = "identical")]
    _identical: CodegenIdentity,
    expected: bool,
    unreached: bool,
    #[serde(deserialize_with = "required_option")]
    source_run_id: Option<String>,
    evidence: super::Resting,
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
            declined,
            route,
            _identical: _,
            expected,
            unreached,
            source_run_id,
            evidence,
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
            declined,
            evidence,
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
    reason: Left,
}

/// Why a run left a candidate out, as a report spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Left {
    /// The compiler refused the edit.
    CompilerRefused,
    /// The compiler evaluates the function the edit is in before the program runs.
    EvaluatedBeforeRun,
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
            reason,
        } = self;
        Refusal {
            index,
            display_id,
            refused: match reason {
                Left::CompilerRefused => true,
                Left::EvaluatedBeforeRun => false,
            },
        }
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
    #[serde(deserialize_with = "required_option")]
    locator: Option<Locator>,
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
    #[serde(deserialize_with = "required_option")]
    why: Option<String>,
    #[serde(rename = "where")]
    #[serde(deserialize_with = "required_option")]
    holds: Option<Holds>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Holds {
    #[serde(deserialize_with = "required_option")]
    cfg: Option<String>,
    env: BTreeMap<String, String>,
}

impl Expectation {
    fn claim(self) -> Claim {
        let Self {
            id,
            locator,
            _reason: _,
            _outcome: _,
            mutant,
            _covered: _,
            standing,
            _actual: _,
            why,
            holds,
        } = self;
        let (cfg, env) = match holds {
            Some(Holds { cfg, env }) => (cfg, !env.is_empty()),
            None => (None, false),
        };
        let named = match locator {
            None => Named::Identity(id.clone()),
            Some(Locator {
                path,
                item,
                rule,
                original,
                line,
                count,
            }) => Named::Place {
                path,
                item,
                rule,
                original,
                line,
                count,
            },
        };
        Claim {
            id,
            named,
            mutant,
            standing,
            why,
            cfg,
            env,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Locator {
    path: String,
    item: String,
    rule: String,
    original: String,
    #[serde(deserialize_with = "required_option")]
    line: Option<u64>,
    #[serde(deserialize_with = "required_option")]
    count: Option<u64>,
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

/// A reached-v1 document in its exact owned shape, which is what the audit reads its facts from.
pub(super) fn read_reached(value: &Value) -> Result<Measurement, serde_json::Error> {
    serde_json::from_value::<Measurement>(value.clone())
}

/// A touched-v1 document in its exact owned shape, which is what the audit reads its facts from.
///
/// A record in any form but the objects the engine writes, or a null where it writes a value or nothing, is refused, since either would read as a record of nothing.
pub(super) fn read_touched(value: &Value) -> Result<Guarded, serde_json::Error> {
    let document = serde_json::from_value::<Guarded>(value.clone())?;
    reject_unowned_touched_forms(value)?;
    Ok(document)
}

fn reject_unowned_touched_forms(value: &Value) -> Result<(), serde_json::Error> {
    let root = owned_object(value, "a touched document")?;
    reject_null(root.get("narrowing"), "touched narrowing")?;
    reject_null(root.get("items"), "the touched item catalog")?;
    if let Some(narrowing) = root.get("narrowing") {
        let narrowing = owned_object(narrowing, "touched narrowing")?;
        reject_null(narrowing.get("compared"), "the touched compared list")?;
        reject_null(narrowing.get("bodies"), "the touched body markers")?;
    }
    let Some(targets) = root.get("targets") else {
        return Ok(());
    };
    for target in owned_object(targets, "the touched targets")?.values() {
        let target = owned_object(target, "a touched target record")?;
        for kind in ["reached", "bodies", "infected", "entered"] {
            let Some(seen) = target.get(kind) else {
                continue;
            };
            let seen = owned_object(seen, "a touched kind record")?;
            reject_null(seen.get("tests"), "a touched test map")?;
            reject_null(seen.get("loose"), "a touched loose-site list")?;
        }
    }
    Ok(())
}

/// `value` as the object the engine writes there, or a refusal naming what it should have been.
fn owned_object<'a>(
    value: &'a Value,
    what: &str,
) -> Result<&'a serde_json::Map<String, Value>, serde_json::Error> {
    value.as_object().ok_or_else(|| {
        serde_json::Error::custom(format!("{what} must be an object, as the engine writes it"))
    })
}

fn reject_null(value: Option<&Value>, what: &str) -> Result<(), serde_json::Error> {
    if value.is_some_and(Value::is_null) {
        return Err(serde_json::Error::custom(format!(
            "{what} may be absent but may not be null"
        )));
    }
    Ok(())
}

/// What the coverage layer measured, as a run keeps it in `reached-v1.json`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Measurement {
    /// The blocks each target's measured run covered, by target.
    pub(super) targets: BTreeMap<String, Vec<CoverageBlock>>,
    #[serde(rename = "instrumented")]
    _instrumented: Vec<CoverageBlock>,
    /// Why a target the run built was not measured, each `<limitation>:<target>`.
    pub(super) limitations: Vec<String>,
}

/// One block a measured run covered.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CoverageBlock {
    /// The file it lies in.
    pub(super) file: String,
    /// Where it begins.
    pub(super) start: CoveragePoint,
    #[serde(rename = "end")]
    _end: CoveragePoint,
}

/// A place in a file, by line and column.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CoveragePoint {
    /// The line.
    pub(super) line: u64,
    /// The column.
    pub(super) column: u64,
}

/// What the guards of a whole run recorded, as a run keeps it in `touched-v1.json`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Guarded {
    /// What each target's guards recorded, by target.
    pub(super) targets: BTreeMap<String, GuardedTarget>,
    /// Why a target the run built is not in `targets`, each `<limitation>:<target>`.
    pub(super) limitations: Vec<String>,
    /// Which of the records are facts about which mutant, where the record keeps it.
    pub(super) narrowing: Option<GuardedNarrowing>,
    /// Every item of every instrumented file, where the record keeps the catalog.
    pub(super) items: Option<Vec<GuardedItem>>,
}

/// One item of the catalog the guards' record keeps.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GuardedItem {
    /// The index an entry marker names it by.
    pub(super) index: u64,
    #[serde(rename = "package")]
    _package: String,
    /// The workspace-relative path.
    pub(super) path: String,
    /// The item as a reader writes it.
    pub(super) name: String,
    #[serde(rename = "span")]
    _span: GuardedSpan,
    /// Its body's bytes.
    pub(super) body: GuardedSpan,
    /// Whether the tree records entering it.
    pub(super) measurable: bool,
}

/// A byte range of a file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GuardedSpan {
    /// The first byte.
    pub(super) start: u64,
    /// One past the last byte.
    pub(super) end: u64,
}

/// Which records are facts about which mutant, each list where the record keeps it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GuardedNarrowing {
    /// Every mutant whose guard compares its two branches.
    pub(super) compared: Option<Vec<u64>>,
    /// The marker each mutant's branch proof rests on.
    pub(super) bodies: Option<BTreeMap<u64, u64>>,
}

/// What one target's guards recorded, each kind where the record keeps it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GuardedTarget {
    /// The mutant sites each test reached.
    pub(super) reached: Option<GuardedSeen>,
    /// The branch bodies each test entered.
    pub(super) bodies: Option<GuardedSeen>,
    /// The mutations each test saw a guard's branches part over.
    pub(super) infected: Option<GuardedSeen>,
    /// The items each test entered.
    pub(super) entered: Option<GuardedSeen>,
    /// Every test the baseline ran.
    pub(super) ran: Vec<String>,
}

/// One kind of record, by the test that made it; the engine leaves out an empty map and an empty list.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GuardedSeen {
    /// What each named test reported.
    pub(super) tests: Option<BTreeMap<String, Vec<u64>>>,
    /// What was reported where nothing named a test.
    pub(super) loose: Option<Vec<u64>>,
}
