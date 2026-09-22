// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Kani 0.68's exported property document, interpreted without exit-status shortcuts.

use std::path::{Component, Path};

use serde::Deserialize;

use super::{
    Harness, KANI_BUILD_MODE, KANI_CBMC_VERSION, KANI_EXPORT_VERSION, KANI_GOTO_CC_BACKEND,
    KANI_GOTO_INSTRUMENT_VERSION, KANI_RUSTC_VERSION, KANI_SOLVER, KANI_VERSION,
};

/// Invocation facts to which a raw export must be bound before it can decide anything.
/// They come from the fresh private workspace, not from the export.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Expectation<'a> {
    pub harness: &'a Harness,
    pub target: &'a str,
    pub root: &'a Path,
    pub target_dir: &'a Path,
    pub package: &'a str,
}

/// What the model checker established about the differential property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Every property, including the tagged equality and every unwind check, succeeded.
    Proved,
    /// The tagged equality alone failed with a counterexample.
    Noticed,
    /// No sound conclusion was established.
    Undecided(Undecided),
}

/// Why no model-checking conclusion was established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Undecided {
    /// A failed unwind property says the configured bound was exhausted.
    BoundExhausted,
    /// The externally supervised wall-clock ceiling expired.
    Cutoff,
    /// The caller cancelled the proof before it produced a complete answer.
    Cancelled,
    /// The invocation could not faithfully represent the measured build.
    Configuration(Configuration),
    /// The pinned verifier could not be established before verification.
    Tool(ToolFailure),
    /// The supervised verifier process did not produce a normal, supported exit.
    Process(ProcessFailure),
    /// The raw result artifact could not be trusted or retained.
    Artifact(ArtifactFailure),
    /// An affirmative document contradicted the verifier process's exit code.
    ExitMismatch {
        /// The affirmative decision encoded in the document.
        decision: Affirmative,
        /// The exit code the verifier actually returned.
        actual: i32,
    },
    /// The pinned export schema was absent, malformed, duplicated, or contradictory.
    Protocol(Protocol),
    /// Kani returned a property status that proves neither equality nor difference.
    Property {
        /// The status that prevented a decision.
        status: PropertyStatus,
    },
    /// A property other than the differential equality failed.
    OtherFailure {
        /// The category Kani assigned to it.
        category: String,
    },
}

/// A model-checker answer that requires a corresponding, pinned process exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Affirmative {
    /// Equality was proved; Kani must have exited zero.
    Proved,
    /// A counterexample was produced; Kani 0.68 must have exited one.
    Noticed,
}

/// Why the requested build cannot be represented by the closed Kani command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Configuration {
    /// A Cargo package name is mandatory and may not be blank.
    Package,
    /// A custom Cargo profile may change integer and panic semantics.
    Profile,
    /// Caller-supplied compiler flags would make the proof semantics depend on environment or Cargo configuration outside the retained evidence.
    CompilerFlags,
    /// Caller-supplied compiler selection, wrappers, or bootstrap state could interpose on the pinned Kani compiler named by the evidence.
    CompilerEnvironment,
    /// The artifact or target directory was not an absolute path.
    RelativePath,
    /// The artifact parent or verifier working directory was absent.
    Directory,
    /// A measured test wrote into the subject tree, so its post-measurement bytes are not the pristine tree from which the mutation was catalogued.
    TreeWritten,
    /// The fresh proof copy did not match the immutable prepared workspace,
    /// or the verifier changed that copy outside its restored harness file.
    WorkspaceDrift,
}

/// Why the executable at the configured Cargo path was not the pinned Kani.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolFailure {
    /// `cargo kani --version` could not be started or supervised.
    Unavailable,
    /// The version command exited unsuccessfully or by signal.
    VersionCommand,
    /// The exact pinned version banner was absent or truncated.
    VersionBanner,
    /// `cargo kani list` could not be run to a normal successful exit.
    HarnessListCommand,
    /// The list command did not leave one complete regular catalog file.
    HarnessListArtifact,
    /// The harness catalog was not exactly the pinned list schema.
    HarnessListSchema,
    /// The catalog did not name exactly one fully-qualified generated harness.
    HarnessListMatch,
}

/// Why a started verifier process supplied no interpretable exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessFailure {
    /// The process could not be started or supervised.
    NotStarted,
    /// The process was stopped through a monitor this runner did not install.
    Stopped,
    /// The execution monitor could not be inspected safely.
    Monitor,
    /// The operating system did not yield a trustworthy final status.
    Wait,
    /// The process exited because of a signal.
    Signal,
    /// The process status exposed neither a code nor a signal.
    UnknownExit,
    /// Kani returned a code outside its pinned zero/one protocol.
    UnexpectedExit,
}

/// Why the exported JSON cannot be the retained audit artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArtifactFailure {
    /// A path already existed, so a stale document could be mistaken for this run.
    AlreadyExists,
    /// Kani exited without creating the required document.
    Missing,
    /// The path is not a regular file.
    NotFile,
    /// The document exceeded the parser's fixed memory ceiling.
    TooLarge,
    /// The document could not be read completely.
    Unreadable,
    /// The generated source differed before or after the verifier process.
    SourceChanged,
}

/// Whether the export names the pinned backend, saying which field disagreed when it does not.
fn backend_agrees(document: &Document, expected: &Expectation<'_>) -> Result<(), Protocol> {
    for (field, found, pinned) in [
        (
            "build_mode",
            document.metadata.build_mode.as_str(),
            KANI_BUILD_MODE,
        ),
        ("target", document.metadata.target.as_str(), expected.target),
        ("rustc", document.tools.rustc.as_str(), KANI_RUSTC_VERSION),
        ("cbmc", document.tools.cbmc.as_str(), KANI_CBMC_VERSION),
        (
            "goto-instrument",
            document.tools.goto_instrument.as_str(),
            KANI_GOTO_INSTRUMENT_VERSION,
        ),
    ] {
        if found != pinned {
            note_backend_mismatch(field, found, pinned);
            return Err(Protocol::Backend);
        }
    }
    if !document.tools.goto_cc.contains(KANI_GOTO_CC_BACKEND) {
        note_backend_mismatch("goto-cc", &document.tools.goto_cc, KANI_GOTO_CC_BACKEND);
        return Err(Protocol::Backend);
    }
    if !matches!(document.tools.solvers.as_slice(), [Solver { name, version: Nullable(None) }] if name == KANI_SOLVER)
    {
        note_backend_mismatch(
            "solvers",
            &format!("{:?}", document.tools.solvers),
            KANI_SOLVER,
        );
        return Err(Protocol::Backend);
    }
    Ok(())
}

/// Says which pinned backend field the export disagreed with, because `Protocol::Backend` names none of them.
fn note_backend_mismatch(field: &str, found: &str, pinned: &str) {
    use std::io::Write as _;
    match writeln!(
        std::io::stderr(),
        "njutest: the Kani export's {field} is {found:?}, and the pinned backend is {pinned:?}"
    ) {
        Ok(()) | Err(_) => {}
    }
}

/// Which protocol invariant the exported result broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Protocol {
    /// The bytes were not exactly the pinned JSON schema.
    Schema,
    /// The metadata and tool records do not both name the pinned Kani version.
    ToolVersion,
    /// The document does not use the pinned Kani export schema version.
    ExportVersion,
    /// The document was produced by a different build mode or proof backend.
    Backend,
    /// The summary is not exactly one completed, executed harness.
    Summary,
    /// There is not exactly one result for the generated harness.
    Harness,
    /// The tagged assertion is absent, repeated, or not an assertion.
    Assertion,
    /// Harness, summary, and property statuses contradict each other.
    Contradiction,
}

/// Every property status Kani 0.68 can export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) enum PropertyStatus {
    /// A counterexample exists.
    Failure,
    /// A cover property was reached.
    Covered,
    /// A property was satisfied in Kani's cover/contract vocabulary.
    Satisfied,
    /// The property was proved.
    Success,
    /// The solver did not determine the property.
    Undetermined,
    /// Kani could not classify the property.
    Unknown,
    /// The property was unreachable, which is not a proof of its predicate.
    Unreachable,
    /// A cover property was not reached.
    Uncovered,
    /// A property was unsatisfiable in Kani's contract vocabulary.
    Unsatisfiable,
    /// The solver or checker errored.
    Error,
}

/// The independently checkable identity of one Kani answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KaniEvidence {
    /// The inseparable tool and proof-engine identity exported by a parsed document.
    pub verifier: Option<VerifiedBackend>,
    /// The fully-qualified harness identity Kani returned.
    pub harness: String,
    /// The exact tagged property description.
    pub assertion: String,
    /// The nonzero unwind compiled onto the proof harness.
    pub unwind: std::num::NonZeroU32,
    /// The nonzero external wall-clock ceiling applied to this invocation.
    pub timeout_ms: std::num::NonZeroU64,
    /// The pristine source digest admitted by the eligibility checker.
    pub source_digest: String,
    /// The generated proof source digest handed to rustc and Kani.
    pub rendered_digest: String,
    /// Domain-separated identity of the fixed manifest, lockfile, and source.
    pub crate_digest: String,
    /// The full mutation identity embedded in both the harness and assertion.
    pub mutant_id: String,
    /// The workspace-relative path in the independently re-mintable identity.
    pub path: String,
    /// The mutation rule name in the independently re-mintable identity.
    pub rule: String,
    /// The mutation rule version in the independently re-mintable identity.
    pub rule_version: u32,
    /// The first pristine-source byte replaced by the mutation.
    pub start_byte: u32,
    /// One past the last pristine-source byte replaced by the mutation.
    pub end_byte: u32,
    /// Hexadecimal pristine bytes covered by the mutation span.
    pub original_hex: String,
    /// Hexadecimal replacement bytes used for the mutant rendering.
    pub replacement_hex: String,
    /// SHA-256 of the exact exported JSON bytes retained for independent audit.
    pub raw_digest: String,
}

/// The tool banner and backend metadata established by the same parsed export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedBackend {
    /// Kani's own version record, equal to [`KANI_VERSION`] for a decision.
    pub tool: String,
    /// The exact proof-engine identity exported by that Kani document.
    pub backend: KaniBackend,
}

/// The proof-engine boundary whose exact values are prerequisites for an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KaniBackend {
    /// Kani's exported result schema.
    pub export_version: String,
    /// Kani's code-generation mode.
    pub build_mode: String,
    /// The explicitly fixed target triple Kani exported.
    pub target: String,
    /// The compiler embedded in Kani rather than the project's measured rustc.
    pub rustc: String,
    /// The bounded model checker version.
    pub cbmc: String,
    /// The goto compiler version.
    pub goto_cc: String,
    /// The goto instrumentation version.
    pub goto_instrument: String,
    /// The explicitly selected solver.
    pub solver: String,
}

/// A decision together with the identity of the bytes from which it was derived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Parsed {
    /// The fail-closed model-checking decision.
    pub decision: Decision,
    /// Evidence sufficient to locate and re-hash the retained raw document.
    pub evidence: KaniEvidence,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    metadata: Metadata,
    project: Project,
    tools: Tools,
    harness_metadata: Vec<HarnessMetadata>,
    error_details: Vec<ErrorDetail>,
    property_details: Vec<PropertyDetail>,
    cbmc: Vec<Cbmc>,
    verification_results: VerificationResults,
    coverage: Coverage,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Project {
    crate_name: Vec<String>,
    workspace_root: String,
    output_dir: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    version: String,
    #[serde(rename = "timestamp")]
    _timestamp: String,
    kani_version: String,
    target: String,
    build_mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tools {
    kani: String,
    rustc: String,
    cbmc: String,
    goto_cc: String,
    goto_instrument: String,
    solvers: Vec<Solver>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Solver {
    name: String,
    version: Nullable<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(transparent)]
struct Nullable<T>(Option<T>);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HarnessMetadata {
    pretty_name: String,
    mangled_name: String,
    crate_name: String,
    source: HarnessSource,
    goto_file: String,
    attributes: HarnessAttributes,
    contract: HarnessContract,
    has_loop_contracts: Flag,
    is_automatically_generated: Flag,
    is_bounded: Flag,
    is_ctor_based: Flag,
}

#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct Flag(bool);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HarnessSource {
    file: String,
    start_line: u64,
    end_line: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HarnessAttributes {
    kind: String,
    should_panic: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HarnessContract {
    contracted_function_name: Nullable<String>,
    recursion_tracker: Nullable<String>,
}

#[derive(Debug)]
enum ErrorDetail {
    Clean(CleanErrorDetail),
    AssertionFailure(AssertionFailureDetail),
}

impl<'de> Deserialize<'de> for ErrorDetail {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let Some(object) = value.as_object() else {
            return Err(serde::de::Error::custom("error detail must be an object"));
        };
        let exact = |names: &[&str]| {
            object.len() == names.len() && names.iter().all(|name| object.contains_key(*name))
        };
        if exact(&["harness_id", "has_errors"]) {
            return serde_json::from_value::<CleanErrorDetail>(value)
                .map(Self::Clean)
                .map_err(serde::de::Error::custom);
        }
        if exact(&[
            "harness_id",
            "has_errors",
            "error_type",
            "failed_properties_type",
            "exit_status",
        ]) {
            return serde_json::from_value::<AssertionFailureDetail>(value)
                .map(Self::AssertionFailure)
                .map_err(serde::de::Error::custom);
        }
        Err(serde::de::Error::custom(
            "error detail has neither exact supported shape",
        ))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CleanErrorDetail {
    harness_id: String,
    has_errors: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AssertionFailureDetail {
    harness_id: String,
    has_errors: bool,
    error_type: String,
    failed_properties_type: String,
    exit_status: String,
}

impl ErrorDetail {
    fn harness_id(&self) -> &str {
        match self {
            Self::Clean(detail) => &detail.harness_id,
            Self::AssertionFailure(detail) => &detail.harness_id,
        }
    }

    fn coherent_with(&self, status: PropertyStatus) -> bool {
        match self {
            Self::Clean(detail) => !detail.has_errors && status != PropertyStatus::Failure,
            Self::AssertionFailure(detail) => {
                detail.has_errors
                    && detail.error_type == "assertion_failure"
                    && detail.failed_properties_type == "PanicsOnly"
                    && detail.exit_status == "properties_failed"
                    && status == PropertyStatus::Failure
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyDetail {
    harness_id: String,
    property_details: PropertyCounts,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyCounts {
    total_properties: u64,
    passed: u64,
    failed: u64,
    unreachable: u64,
    undetermined: u64,
    solver_error: u64,
    satisfied: u64,
    unsatisfiable: u64,
    covered: u64,
    uncovered: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cbmc {
    harness_id: String,
    #[serde(rename = "cbmc_metadata")]
    metadata: CbmcMetadata,
    configuration: CbmcConfiguration,
    #[serde(rename = "cbmc_stats")]
    stats: CbmcStats,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CbmcMetadata {
    version: String,
    os_info: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CbmcConfiguration {
    object_bits: u64,
    solver: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CbmcStats {
    runtime_symex_s: Nullable<f64>,
    size_program_expression: u64,
    slicing_removed_assignments: u64,
    vccs_generated: u64,
    vccs_remaining: u64,
    runtime_postprocess_equation_s: Nullable<f64>,
    runtime_convert_ssa_s: Nullable<f64>,
    runtime_post_process_s: Nullable<f64>,
    runtime_solver_s: Nullable<f64>,
    runtime_decision_procedure_s: Nullable<f64>,
}

impl CbmcStats {
    fn coherent(&self) -> bool {
        let timings = [
            self.runtime_symex_s.0,
            self.runtime_postprocess_equation_s.0,
            self.runtime_convert_ssa_s.0,
            self.runtime_post_process_s.0,
            self.runtime_solver_s.0,
            self.runtime_decision_procedure_s.0,
        ];
        timings.into_iter().all(|timing| match timing {
            Some(value) => value.is_finite() && value >= 0.0,
            None => true,
        }) && self.vccs_remaining <= self.vccs_generated
            && self.size_program_expression > 0
            && self.slicing_removed_assignments <= self.size_program_expression
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Coverage {
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerificationResults {
    summary: Summary,
    results: Vec<HarnessResult>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    total_harnesses: u64,
    executed: u64,
    status: SummaryStatus,
    successful: u64,
    failed: u64,
    #[serde(rename = "duration_ms")]
    _duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SummaryStatus {
    Completed,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HarnessResult {
    harness_id: String,
    status: PropertyStatus,
    #[serde(rename = "duration_ms")]
    _duration_ms: u64,
    checks: Vec<Check>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Check {
    id: CheckId,
    function: String,
    status: PropertyStatus,
    description: String,
    location: Location,
    category: String,
}

#[derive(Debug)]
enum CheckId {
    Number(u64),
    Text(String),
}

impl<'de> Deserialize<'de> for CheckId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::Number(number) => number
                .as_u64()
                .map(Self::Number)
                .ok_or_else(|| serde::de::Error::custom("check id must be an unsigned integer")),
            serde_json::Value::String(text) => Ok(Self::Text(text)),
            serde_json::Value::Null
            | serde_json::Value::Bool(_)
            | serde_json::Value::Array(_)
            | serde_json::Value::Object(_) => Err(serde::de::Error::custom(
                "check id must be an unsigned integer or string",
            )),
        }
    }
}

impl CheckId {
    fn coherent(&self) -> bool {
        match self {
            Self::Number(number) => *number > 0,
            Self::Text(text) => !text.trim().is_empty(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Location {
    file: String,
    line: String,
    column: String,
}

/// Re-derives a decision from Kani's raw export.
///
/// Malformed input is a value, never an error path a caller can accidentally discard.
/// The only two affirmative decisions are constructed below from the tagged property and all of its sibling properties.
pub(crate) fn parse(raw: &[u8], expected: Expectation<'_>) -> Parsed {
    let raw_digest = rust_mutants::id::digest(raw);
    let Ok(value) = crate::strictjson::from_slice(raw) else {
        return protocol(
            evidence(expected.harness, None, raw_digest),
            Protocol::Schema,
        );
    };
    let Ok(document) = serde_json::from_value::<Document>(value) else {
        return protocol(
            evidence(expected.harness, None, raw_digest),
            Protocol::Schema,
        );
    };
    let mut evidence = evidence(expected.harness, None, raw_digest);
    evidence.verifier = Some(VerifiedBackend {
        tool: document.tools.kani.clone(),
        backend: KaniBackend {
            export_version: document.metadata.version.clone(),
            build_mode: document.metadata.build_mode.clone(),
            target: document.metadata.target.clone(),
            rustc: document.tools.rustc.clone(),
            cbmc: document.tools.cbmc.clone(),
            goto_cc: document.tools.goto_cc.clone(),
            goto_instrument: document.tools.goto_instrument.clone(),
            solver: document
                .tools
                .solvers
                .first()
                .map_or_else(String::new, |solver| solver.name.clone()),
        },
    });
    let (summary, result) = match validated(&document, expected) {
        Ok(parts) => parts,
        Err(why) => return protocol(evidence, why),
    };
    Parsed {
        decision: properties(summary, result, expected.harness),
        evidence,
    }
}

fn validated<'document>(
    document: &'document Document,
    expected: Expectation<'_>,
) -> Result<(&'document Summary, &'document HarnessResult), Protocol> {
    let (details, errors) = validated_context(document, expected)?;
    let summary = &document.verification_results.summary;
    let classified = summary
        .successful
        .checked_add(summary.failed)
        .ok_or(Protocol::Summary)?;
    if summary.total_harnesses != 1
        || summary.executed != 1
        || summary.status != SummaryStatus::Completed
        || classified != 1
    {
        return Err(Protocol::Summary);
    }
    let [result] = document.verification_results.results.as_slice() else {
        return Err(Protocol::Harness);
    };
    if result.harness_id != expected.harness.harness_name() {
        return Err(Protocol::Harness);
    }
    if !errors.coherent_with(result.status) {
        return Err(Protocol::Contradiction);
    }
    validated_counts(&details.property_details, result)?;
    Ok((summary, result))
}

fn validated_context<'document>(
    document: &'document Document,
    expected: Expectation<'_>,
) -> Result<(&'document PropertyDetail, &'document ErrorDetail), Protocol> {
    if document.metadata.kani_version != KANI_VERSION
        || document.tools.kani != KANI_VERSION
        || document.metadata.kani_version != document.tools.kani
    {
        return Err(Protocol::ToolVersion);
    }
    if document.metadata.version != KANI_EXPORT_VERSION {
        return Err(Protocol::ExportVersion);
    }
    backend_agrees(document, &expected)?;
    let [metadata] = document.harness_metadata.as_slice() else {
        return Err(Protocol::Harness);
    };
    let [errors] = document.error_details.as_slice() else {
        return Err(Protocol::Harness);
    };
    let [details] = document.property_details.as_slice() else {
        return Err(Protocol::Harness);
    };
    let [cbmc] = document.cbmc.as_slice() else {
        return Err(Protocol::Harness);
    };
    let [crate_name] = document.project.crate_name.as_slice() else {
        return Err(Protocol::Backend);
    };
    let expected_crate = expected.package.replace('-', "_");
    let workspace_root = Path::new(&document.project.workspace_root);
    let output_dir = Path::new(&document.project.output_dir);
    let goto_file = Path::new(&metadata.goto_file);
    if crate_name != &expected_crate
        || !same_bound_path(workspace_root, expected.root)?
        || !bound_descendant(output_dir, expected.target_dir)?
        || metadata.pretty_name != expected.harness.harness_name()
        || metadata.mangled_name.trim().is_empty()
        || metadata.crate_name != expected_crate
        || Path::new(&metadata.source.file) != Path::new(super::MODEL_SOURCE_PATH)
        || metadata.source.start_line == 0
        || metadata.source.end_line < metadata.source.start_line
        || !bound_descendant(goto_file, output_dir)?
        || metadata.attributes.kind != "Proof"
        || metadata.attributes.should_panic
        || metadata
            .contract
            .contracted_function_name
            .0
            .as_ref()
            .is_some()
        || metadata.contract.recursion_tracker.0.as_ref().is_some()
        || metadata.has_loop_contracts.0
        || metadata.is_automatically_generated.0
        || metadata.is_bounded.0
        || metadata.is_ctor_based.0
        || errors.harness_id() != expected.harness.harness_name()
        || details.harness_id != expected.harness.harness_name()
        || cbmc.harness_id != expected.harness.harness_name()
        || cbmc.metadata.version != super::KANI_BACKEND_VERSION
        || cbmc.metadata.os_info.trim().is_empty()
        || cbmc.configuration.object_bits != 16
        || cbmc.configuration.solver != KANI_SOLVER
        || !cbmc.stats.coherent()
        || document.coverage.enabled
    {
        return Err(Protocol::Backend);
    }
    Ok((details, errors))
}

/// Compare paths at a protocol boundary without treating macOS's public `/var` spelling and its canonical `/private/var` spelling as different workspaces.
/// Exact clean paths remain comparable in parser-only tests where the retained tree is intentionally absent; an existing path is compared by its filesystem identity.
fn same_bound_path(left: &Path, right: &Path) -> Result<bool, Protocol> {
    if !clean_absolute(left) || !clean_absolute(right) {
        return Ok(false);
    }
    let resolved_left = resolved_path(left).map_err(|_error| Protocol::Backend)?;
    let resolved_right = resolved_path(right).map_err(|_error| Protocol::Backend)?;
    Ok(match (resolved_left, resolved_right) {
        (Some(resolved_left), Some(resolved_right)) => resolved_left == resolved_right,
        (None | Some(_), None) | (None, Some(_)) => left == right,
    })
}

/// Require `child` to be a strict descendant of `parent`, resolving existing symlinks before the containment decision.
/// The lexical fallback is only for absent retained artifacts and accepts no `.` or `..` component.
fn bound_descendant(child: &Path, parent: &Path) -> Result<bool, Protocol> {
    if !clean_absolute(child) || !clean_absolute(parent) {
        return Ok(false);
    }
    let resolved_child = resolved_path(child).map_err(|_error| Protocol::Backend)?;
    let resolved_parent = resolved_path(parent).map_err(|_error| Protocol::Backend)?;
    Ok(match (resolved_child, resolved_parent) {
        (Some(resolved_child), Some(resolved_parent)) => {
            resolved_child != resolved_parent && resolved_child.starts_with(resolved_parent)
        }
        (None | Some(_), None) | (None, Some(_)) => child != parent && child.starts_with(parent),
    })
}

fn resolved_path(path: &Path) -> std::io::Result<Option<std::path::PathBuf>> {
    match std::fs::canonicalize(path) {
        Ok(resolved) => Ok(Some(resolved)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                return Ok(None);
            };
            match std::fs::canonicalize(parent) {
                Ok(resolved_parent) => Ok(Some(resolved_parent.join(name))),
                Err(parent_error) if parent_error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(None)
                }
                Err(parent_error) => Err(parent_error),
            }
        }
        Err(error) => Err(error),
    }
}

fn clean_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path.components().all(|component| {
            matches!(
                component,
                Component::Prefix(_) | Component::RootDir | Component::Normal(_)
            )
        })
}

fn validated_counts(counts: &PropertyCounts, result: &HarnessResult) -> Result<(), Protocol> {
    let checks = u64::try_from(result.checks.len()).map_err(|_error| Protocol::Summary)?;
    let count = |predicate: fn(PropertyStatus) -> bool| {
        u64::try_from(
            result
                .checks
                .iter()
                .filter(|check| predicate(check.status))
                .count(),
        )
        .map_err(|_error| Protocol::Summary)
    };
    let reported = [
        counts.passed,
        counts.failed,
        counts.unreachable,
        counts.undetermined,
        counts.solver_error,
        counts.satisfied,
        counts.unsatisfiable,
        counts.covered,
        counts.uncovered,
    ];
    let observed = [
        count(|status| status == PropertyStatus::Success)?,
        count(|status| status == PropertyStatus::Failure)?,
        count(|status| status == PropertyStatus::Unreachable)?,
        count(|status| {
            matches!(
                status,
                PropertyStatus::Undetermined | PropertyStatus::Unknown
            )
        })?,
        count(|status| status == PropertyStatus::Error)?,
        count(|status| status == PropertyStatus::Satisfied)?,
        count(|status| status == PropertyStatus::Unsatisfiable)?,
        count(|status| status == PropertyStatus::Covered)?,
        count(|status| status == PropertyStatus::Uncovered)?,
    ];
    if counts.total_properties != checks
        || observed != reported
        || result.checks.iter().any(|check| {
            !check.id.coherent()
                || check.function.trim().is_empty()
                || check.location.file.trim().is_empty()
                || check.location.line.trim().is_empty()
                || check.location.column.trim().is_empty()
        })
    {
        return Err(Protocol::Summary);
    }
    Ok(())
}

fn properties(summary: &Summary, result: &HarnessResult, expected: &Harness) -> Decision {
    let matching: Vec<&Check> = result
        .checks
        .iter()
        .filter(|check| check.description == expected.assertion_tag())
        .collect();
    let [assertion] = matching.as_slice() else {
        return Decision::Undecided(Undecided::Protocol(Protocol::Assertion));
    };
    if assertion.category != "assertion" {
        return Decision::Undecided(Undecided::Protocol(Protocol::Assertion));
    }

    if result
        .checks
        .iter()
        .any(|check| check.category == "unwind" && check.status == PropertyStatus::Failure)
    {
        return Decision::Undecided(Undecided::BoundExhausted);
    }

    let other = result
        .checks
        .iter()
        .filter(|check| check.description != expected.assertion_tag())
        .find(|check| check.status != PropertyStatus::Success);
    if let Some(check) = other {
        return Decision::Undecided(nonassertion_failure(check));
    }

    match assertion.status {
        PropertyStatus::Success
            if result.status == PropertyStatus::Success
                && summary.successful == 1
                && summary.failed == 0 =>
        {
            Decision::Proved
        }
        PropertyStatus::Failure
            if result.status == PropertyStatus::Failure
                && summary.successful == 0
                && summary.failed == 1 =>
        {
            Decision::Noticed
        }
        PropertyStatus::Success | PropertyStatus::Failure => {
            Decision::Undecided(Undecided::Protocol(Protocol::Contradiction))
        }
        status @ (PropertyStatus::Covered
        | PropertyStatus::Satisfied
        | PropertyStatus::Undetermined
        | PropertyStatus::Unknown
        | PropertyStatus::Unreachable
        | PropertyStatus::Uncovered
        | PropertyStatus::Unsatisfiable
        | PropertyStatus::Error) => Decision::Undecided(Undecided::Property { status }),
    }
}

fn nonassertion_failure(check: &Check) -> Undecided {
    match check.status {
        PropertyStatus::Failure => Undecided::OtherFailure {
            category: check.category.clone(),
        },
        PropertyStatus::Success => Undecided::Protocol(Protocol::Contradiction),
        status @ (PropertyStatus::Covered
        | PropertyStatus::Satisfied
        | PropertyStatus::Undetermined
        | PropertyStatus::Unknown
        | PropertyStatus::Unreachable
        | PropertyStatus::Uncovered
        | PropertyStatus::Unsatisfiable
        | PropertyStatus::Error) => Undecided::Property { status },
    }
}

const fn protocol(evidence: KaniEvidence, why: Protocol) -> Parsed {
    Parsed {
        decision: Decision::Undecided(Undecided::Protocol(why)),
        evidence,
    }
}

/// Constructs a fail-closed answer from bytes that never reached the JSON decision protocol (for example a timeout or missing artifact).
pub(super) fn undecided(raw: &[u8], expected: &Harness, why: Undecided) -> Parsed {
    Parsed {
        decision: Decision::Undecided(why),
        evidence: evidence(expected, None, rust_mutants::id::digest(raw)),
    }
}

fn evidence(
    expected: &Harness,
    verifier: Option<VerifiedBackend>,
    raw_digest: String,
) -> KaniEvidence {
    KaniEvidence {
        verifier,
        harness: expected.harness_name().to_owned(),
        assertion: expected.assertion_tag().to_owned(),
        unwind: expected.unwind(),
        timeout_ms: expected.timeout_ms(),
        source_digest: expected.source_digest().to_owned(),
        rendered_digest: expected.rendered_digest().to_owned(),
        crate_digest: expected.crate_digest().to_owned(),
        mutant_id: expected.mutant_id().to_owned(),
        path: expected.path().to_owned(),
        rule: expected.rule().to_owned(),
        rule_version: expected.rule_version(),
        start_byte: expected.span().start,
        end_byte: expected.span().end,
        original_hex: expected.original_hex().to_owned(),
        replacement_hex: expected.replacement_hex().to_owned(),
        raw_digest,
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::indexing_slicing,
        reason = "protocol tests report malformed fixtures by panicking and edit fixed JSON objects"
    )]

    use serde_json::{Value, json};

    use super::{Decision, Expectation, Parsed, Protocol, Undecided, parse};
    use crate::assure::model::tests::{generated, simple_source};
    use crate::assure::model::{
        Harness, KANI_CBMC_VERSION, KANI_GOTO_CC_VERSION, KANI_GOTO_INSTRUMENT_VERSION,
        KANI_RUSTC_VERSION, KANI_SOLVER, KANI_VERSION, MODEL_PACKAGE,
    };

    fn document(harness: &Harness, status: &str) -> Value {
        let succeeded = u64::from(status == "Success");
        let failed = u64::from(status == "Failure");
        let mut value = json!({
            "metadata": {
                "version": "1.0", "timestamp": "2026-09-20T00:00:00Z",
                "kani_version": KANI_VERSION, "target": "test-target", "build_mode": "release"
            },
            "project": {
                "crate_name": ["njutest_verified_model"], "workspace_root": "/fixture",
                "output_dir": "/fixture/target/kani/test-target/release/build/njutest_verified_model/out"
            },
            "tools": {
                "kani": KANI_VERSION, "rustc": KANI_RUSTC_VERSION,
                "cbmc": KANI_CBMC_VERSION, "goto_cc": KANI_GOTO_CC_VERSION,
                "goto_instrument": KANI_GOTO_INSTRUMENT_VERSION,
                "solvers": [{"name": KANI_SOLVER, "version": null}]
            },
            "harness_metadata": [{
                "pretty_name": harness.harness_name(),
                "mangled_name": "mangled", "crate_name": "njutest_verified_model",
                "source": {"file": "src/lib.rs", "start_line": 1, "end_line": 1},
                "goto_file": "/fixture/target/kani/test-target/release/build/njutest_verified_model/out/harness.goto",
                "attributes": {"kind": "Proof", "should_panic": false},
                "contract": {"contracted_function_name": null, "recursion_tracker": null},
                "has_loop_contracts": false, "is_automatically_generated": false,
                "is_bounded": false, "is_ctor_based": false
            }],
            "error_details": [{
                "harness_id": harness.harness_name(), "has_errors": false
            }],
            "property_details": [{
                "harness_id": harness.harness_name(),
                "property_details": {
                    "total_properties": 1, "passed": succeeded, "failed": failed,
                    "unreachable": 0, "undetermined": 0, "solver_error": 0,
                    "satisfied": 0, "unsatisfiable": 0, "covered": 0, "uncovered": 0
                }
            }],
            "cbmc": [{
                "harness_id": harness.harness_name(),
                "cbmc_metadata": {"version": "6.11.0", "os_info": "fixture unix"},
                "configuration": {"object_bits": 16, "solver": KANI_SOLVER},
                "cbmc_stats": {
                    "runtime_symex_s": 0.0, "size_program_expression": 1,
                    "slicing_removed_assignments": 0, "vccs_generated": 1,
                    "vccs_remaining": 1, "runtime_postprocess_equation_s": 0.0,
                    "runtime_convert_ssa_s": null, "runtime_post_process_s": null,
                    "runtime_solver_s": null, "runtime_decision_procedure_s": null
                }
            }],
            "verification_results": {
                "summary": {
                    "total_harnesses": 1, "executed": 1, "status": "completed",
                    "successful": succeeded, "failed": failed, "duration_ms": 1
                },
                "results": [{
                    "harness_id": harness.harness_name(),
                    "status": status, "duration_ms": 1,
                    "checks": [{
                        "id": 1, "function": "f", "status": status,
                        "description": harness.assertion_tag(),
                        "location": { "file": "src/lib.rs", "line": "1", "column": "1" },
                        "category": "assertion"
                    }]
                }]
            },
            "coverage": { "enabled": false }
        });
        if status == "Failure" {
            value["error_details"] = json!([{
                "harness_id": harness.harness_name(),
                "has_errors": true,
                "error_type": "assertion_failure",
                "failed_properties_type": "PanicsOnly",
                "exit_status": "properties_failed"
            }]);
        }
        value
    }

    fn parsed(document: &Value, harness: &Harness) -> Parsed {
        parse(
            &serde_json::to_vec(document).expect("fixture serializes"),
            Expectation {
                harness,
                target: "test-target",
                root: std::path::Path::new("/fixture"),
                target_dir: std::path::Path::new("/fixture/target/kani"),
                package: MODEL_PACKAGE,
            },
        )
    }

    #[test]
    fn only_the_tagged_property_decides_difference_or_equivalence() {
        let harness = generated(simple_source(), ">", ">=");
        assert_eq!(
            parsed(&document(&harness, "Success"), &harness).decision,
            Decision::Proved
        );
        assert_eq!(
            parsed(&document(&harness, "Failure"), &harness).decision,
            Decision::Noticed
        );
    }

    #[test]
    fn a_failed_unwind_dominates_undetermined_properties() {
        let harness = generated(simple_source(), ">", ">=");
        let mut value = document(&harness, "Failure");
        let checks = value["verification_results"]["results"][0]["checks"]
            .as_array_mut()
            .expect("checks");
        checks[0]["status"] = json!("Undetermined");
        checks.push(json!({
            "id": 2, "function": "f", "status": "Failure",
            "description": "unwinding assertion loop 0",
            "location": { "file": "src/lib.rs", "line": "1", "column": "1" },
            "category": "unwind"
        }));
        value["property_details"][0]["property_details"]["total_properties"] = json!(2);
        value["property_details"][0]["property_details"]["failed"] = json!(1);
        value["property_details"][0]["property_details"]["undetermined"] = json!(1);
        assert_eq!(
            parsed(&value, &harness).decision,
            Decision::Undecided(Undecided::BoundExhausted)
        );
    }

    #[test]
    fn every_ambiguous_or_contradictory_shape_fails_closed() {
        let harness = generated(simple_source(), ">", ">=");
        let base = document(&harness, "Success");
        let mut cases = Vec::new();

        cases.push(json!(null));
        let mut wrong_tool = base.clone();
        wrong_tool["tools"]["kani"] = json!("0.69.0");
        cases.push(wrong_tool);
        let mut no_result = base.clone();
        no_result["verification_results"]["results"] = json!([]);
        cases.push(no_result);
        let mut two_results = base.clone();
        let result = two_results["verification_results"]["results"][0].clone();
        two_results["verification_results"]["results"] = json!([result.clone(), result]);
        cases.push(two_results);
        let mut wrong_harness = base.clone();
        wrong_harness["verification_results"]["results"][0]["harness_id"] = json!("other");
        cases.push(wrong_harness);
        let mut alternate_prefix = base.clone();
        let forged = format!("other::{}", harness.harness_name());
        alternate_prefix["harness_metadata"][0]["pretty_name"] = json!(forged);
        alternate_prefix["error_details"][0]["harness_id"] = json!(forged);
        alternate_prefix["property_details"][0]["harness_id"] = json!(forged);
        alternate_prefix["cbmc"][0]["harness_id"] = json!(forged);
        alternate_prefix["verification_results"]["results"][0]["harness_id"] = json!(forged);
        cases.push(alternate_prefix);
        let mut wrong_source = base.clone();
        wrong_source["harness_metadata"][0]["source"]["file"] = json!("src/other.rs");
        cases.push(wrong_source);
        let mut wrong_root = base.clone();
        wrong_root["project"]["workspace_root"] = json!("/other");
        cases.push(wrong_root);
        let mut escaped_output = base.clone();
        escaped_output["project"]["output_dir"] = json!("/other/target");
        cases.push(escaped_output);
        let mut wrong_crate = base.clone();
        wrong_crate["harness_metadata"][0]["crate_name"] = json!("other");
        cases.push(wrong_crate);
        let mut duplicate_tag = base.clone();
        let check = duplicate_tag["verification_results"]["results"][0]["checks"][0].clone();
        duplicate_tag["verification_results"]["results"][0]["checks"] =
            json!([check.clone(), check]);
        cases.push(duplicate_tag);
        let mut contradiction = base;
        contradiction["verification_results"]["results"][0]["status"] = json!("Failure");
        cases.push(contradiction);

        let mut missing_failure_detail = document(&harness, "Failure");
        missing_failure_detail["error_details"] = json!([{
            "harness_id": harness.harness_name(),
            "has_errors": false
        }]);
        cases.push(missing_failure_detail);

        let mut wrong_failure_kind = document(&harness, "Failure");
        wrong_failure_kind["error_details"][0]["error_type"] = json!("different");
        cases.push(wrong_failure_kind);

        let mut open_error_detail = document(&harness, "Success");
        open_error_detail["error_details"][0]["unexpected"] = json!(null);
        cases.push(open_error_detail);

        let mut null_failure_field = document(&harness, "Failure");
        null_failure_field["error_details"][0]["error_type"] = json!(null);
        cases.push(null_failure_field);

        let mut ambiguous_check_id = document(&harness, "Success");
        ambiguous_check_id["verification_results"]["results"][0]["checks"][0]["id"] = json!(true);
        cases.push(ambiguous_check_id);

        let mut contradictory_counts = document(&harness, "Success");
        contradictory_counts["property_details"][0]["property_details"]["passed"] = json!(0);
        contradictory_counts["property_details"][0]["property_details"]["failed"] = json!(1);
        cases.push(contradictory_counts);

        for value in cases {
            assert!(
                matches!(parsed(&value, &harness).decision, Decision::Undecided(_)),
                "{value}"
            );
        }
    }

    #[test]
    fn duplicate_known_document_key_is_a_schema_violation() {
        let harness = generated(simple_source(), ">", ">=");
        let encoded =
            serde_json::to_string(&document(&harness, "Success")).expect("fixture serializes");
        let duplicated = encoded.replacen("\"metadata\":{", "\"metadata\":{},\"metadata\":{", 1);
        assert_ne!(duplicated, encoded, "fixture must contain metadata key");
        assert_eq!(
            parse(
                duplicated.as_bytes(),
                Expectation {
                    harness: &harness,
                    target: "test-target",
                    root: std::path::Path::new("/fixture"),
                    target_dir: std::path::Path::new("/fixture/target/kani"),
                    package: MODEL_PACKAGE,
                }
            )
            .decision,
            Decision::Undecided(Undecided::Protocol(Protocol::Schema))
        );
    }

    #[test]
    fn path_resolution_distinguishes_absence_from_other_io_failures() {
        let temporary = tempfile::tempdir().expect("path protocol fixture");
        let parent = temporary.path().join("parent");
        std::fs::create_dir_all(&parent).expect("existing parent");
        let missing_leaf = parent.join("missing");
        let expected = std::fs::canonicalize(&parent)
            .expect("existing parent resolves")
            .join("missing");
        assert_eq!(
            super::resolved_path(&missing_leaf).expect("a missing leaf is typed absence"),
            Some(expected)
        );

        let not_directory = temporary.path().join("ordinary-file");
        std::fs::write(&not_directory, b"file").expect("ordinary file");
        assert!(
            super::resolved_path(&not_directory.join("child")).is_err(),
            "NotADirectory and permission failures must not become lexical absence"
        );
    }

    #[test]
    fn export_schema_build_mode_and_backend_are_pinned() {
        let harness = generated(simple_source(), ">", ">=");
        for path in [
            ["metadata", "version"].as_slice(),
            ["metadata", "build_mode"].as_slice(),
            ["tools", "cbmc"].as_slice(),
            ["tools", "goto_cc"].as_slice(),
            ["tools", "goto_instrument"].as_slice(),
        ] {
            let mut value = document(&harness, "Success");
            value[path[0]][path[1]] = json!("different");
            assert!(
                matches!(
                    parsed(&value, &harness).decision,
                    Decision::Undecided(Undecided::Protocol(
                        Protocol::ExportVersion | Protocol::Backend
                    ))
                ),
                "{}::{}",
                path[0],
                path[1]
            );
        }
    }

    #[test]
    fn every_non_success_sibling_prevents_a_proof() {
        let harness = generated(simple_source(), ">", ">=");
        for status in [
            "Failure",
            "Covered",
            "Satisfied",
            "Undetermined",
            "Unknown",
            "Unreachable",
            "Uncovered",
            "Unsatisfiable",
            "Error",
        ] {
            let mut value = document(&harness, "Success");
            value["verification_results"]["results"][0]["checks"]
                .as_array_mut()
                .expect("checks")
                .push(json!({
                    "id": 2, "function": "f", "status": status,
                    "description": "a sibling property",
                    "location": { "file": "src/lib.rs", "line": "1", "column": "1" },
                    "category": "safety"
                }));
            assert!(
                matches!(parsed(&value, &harness).decision, Decision::Undecided(_)),
                "{status}"
            );
        }
    }

    #[test]
    fn arbitrary_bytes_never_become_an_affirmative_decision() {
        let harness = generated(simple_source(), ">", ">=");
        proptest::proptest!(|(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..4096))| {
            let decision = parse(
                &bytes,
                Expectation {
                    harness: &harness,
                    target: "test-target",
                    root: std::path::Path::new("/fixture"),
                    target_dir: std::path::Path::new("/fixture/target/kani"),
                    package: MODEL_PACKAGE,
                },
            )
            .decision;
            proptest::prop_assert!(matches!(decision, Decision::Undecided(_)));
        });
    }
}
