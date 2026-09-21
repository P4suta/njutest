// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Independent, fail-closed audit of the pinned Kani law export.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

const KANI_VERSION: &str = "0.68.0";
const EXPORT_VERSION: &str = "1.0";
const RUSTC_VERSION: &str = "rustc 1.100.0-nightly (8925ea358 2026-08-20)";
const CBMC_VERSION: &str = "6.11.0 (cbmc-6.11.0)";
const GOTO_CC_VERSION: &str = "clang version 21.0.0 (goto-cc 6.11.0 (cbmc-6.11.0))";
const GOTO_INSTRUMENT_VERSION: &str = "6.11.0 (cbmc-6.11.0)";
const CRATE_NAME: &str = "rust_mutants";
const SOLVER: &str = "cadical";
const REACHED_COVER: &str = "njutest-law-reached";
const BRANCH_COVER_PREFIX: &str = "njutest-law-branch:";
const ASSERTION_PREFIX: &str = "njutest-law-assertion:";
const TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"];

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, njutest_macros::AllVariants,
)]
pub(crate) enum Harness {
    #[serde(rename = "instrument::runtime::kani_laws::a_dormant_checkpoint_cannot_spend")]
    DormantCheckpoint,
    #[serde(rename = "instrument::runtime::kani_laws::activation_is_idempotent")]
    Activation,
    #[serde(
        rename = "instrument::runtime::kani_laws::an_active_checkpoint_advances_or_reaches_the_exact_boundary"
    )]
    ActiveCheckpoint,
    #[serde(rename = "instrument::runtime::kani_laws::stopping_is_absorbing")]
    Stopping,
    #[serde(rename = "session::kani_laws::attempt_duration_is_the_checked_sum_of_every_execution")]
    AttemptDuration,
    #[serde(rename = "session::kani_laws::attempt_ledger_is_nonempty_and_its_result_is_derived")]
    AttemptLedger,
    #[serde(rename = "session::kani_laws::cancellation_before_retry_cannot_create_a_retry")]
    CancellationBeforeRetry,
    #[serde(rename = "session::kani_laws::cancelled_retry_is_structurally_not_run")]
    CancelledRetry,
    #[serde(
        rename = "session::kani_laws::equal_outcomes_choose_the_canonical_target_in_every_order"
    )]
    EqualOutcomes,
    #[serde(rename = "session::kani_laws::killed_is_absorbing")]
    Killed,
    #[serde(rename = "session::kani_laws::retry_reconciliation_is_closed")]
    RetryReconciliation,
    #[serde(rename = "session::kani_laws::survived_means_every_nonempty_target_survived")]
    Survived,
    #[serde(rename = "session::kani_laws::target_join_is_associative")]
    Associative,
    #[serde(rename = "session::kani_laws::target_join_is_commutative")]
    Commutative,
    #[serde(rename = "session::kani_laws::target_join_is_idempotent")]
    Idempotent,
}

impl Harness {
    const fn name(self) -> &'static str {
        match self {
            Self::DormantCheckpoint => {
                "instrument::runtime::kani_laws::a_dormant_checkpoint_cannot_spend"
            }
            Self::Activation => "instrument::runtime::kani_laws::activation_is_idempotent",
            Self::ActiveCheckpoint => {
                "instrument::runtime::kani_laws::an_active_checkpoint_advances_or_reaches_the_exact_boundary"
            }
            Self::Stopping => "instrument::runtime::kani_laws::stopping_is_absorbing",
            Self::AttemptDuration => {
                "session::kani_laws::attempt_duration_is_the_checked_sum_of_every_execution"
            }
            Self::AttemptLedger => {
                "session::kani_laws::attempt_ledger_is_nonempty_and_its_result_is_derived"
            }
            Self::CancellationBeforeRetry => {
                "session::kani_laws::cancellation_before_retry_cannot_create_a_retry"
            }
            Self::CancelledRetry => "session::kani_laws::cancelled_retry_is_structurally_not_run",
            Self::EqualOutcomes => {
                "session::kani_laws::equal_outcomes_choose_the_canonical_target_in_every_order"
            }
            Self::Killed => "session::kani_laws::killed_is_absorbing",
            Self::RetryReconciliation => "session::kani_laws::retry_reconciliation_is_closed",
            Self::Survived => "session::kani_laws::survived_means_every_nonempty_target_survived",
            Self::Associative => "session::kani_laws::target_join_is_associative",
            Self::Commutative => "session::kani_laws::target_join_is_commutative",
            Self::Idempotent => "session::kani_laws::target_join_is_idempotent",
        }
    }

    const fn source(self) -> &'static str {
        match self {
            Self::DormantCheckpoint
            | Self::Activation
            | Self::ActiveCheckpoint
            | Self::Stopping => "crates/rust-mutants/src/instrument/runtime.rs",
            Self::AttemptDuration
            | Self::AttemptLedger
            | Self::CancellationBeforeRetry
            | Self::CancelledRetry
            | Self::EqualOutcomes
            | Self::Killed
            | Self::RetryReconciliation
            | Self::Survived
            | Self::Associative
            | Self::Commutative
            | Self::Idempotent => "crates/rust-mutants/src/session/mod.rs",
        }
    }

    const fn expected_covers(self) -> &'static [&'static str] {
        match self {
            Self::ActiveCheckpoint => &[
                "njutest-law-branch:advance",
                "njutest-law-branch:boundary",
                REACHED_COVER,
            ],
            Self::Stopping => &[
                "njutest-law-branch:activate",
                "njutest-law-branch:checkpoint",
                REACHED_COVER,
            ],
            Self::AttemptDuration => &[
                "njutest-law-branch:sum",
                "njutest-law-branch:overflow",
                REACHED_COVER,
            ],
            Self::AttemptLedger => &[
                "njutest-law-branch:retried",
                "njutest-law-branch:single",
                REACHED_COVER,
            ],
            Self::RetryReconciliation => &[
                "njutest-law-branch:cancelled",
                "njutest-law-branch:preserved",
                "njutest-law-branch:downgraded",
                REACHED_COVER,
            ],
            Self::Survived => &[
                "njutest-law-branch:survived",
                "njutest-law-branch:not-survived",
                REACHED_COVER,
            ],
            Self::Idempotent => &[
                "njutest-law-branch:empty",
                "njutest-law-branch:killed",
                REACHED_COVER,
            ],
            Self::DormantCheckpoint
            | Self::Activation
            | Self::CancellationBeforeRetry
            | Self::CancelledRetry
            | Self::EqualOutcomes
            | Self::Killed
            | Self::Associative
            | Self::Commutative => &[REACHED_COVER],
        }
    }

    const fn expected_assertions(self) -> &'static [&'static str] {
        match self {
            Self::DormantCheckpoint => &["njutest-law-assertion:dormant-inert"],
            Self::Activation => &["njutest-law-assertion:activation-idempotent"],
            Self::ActiveCheckpoint => &[
                "njutest-law-assertion:active-advance",
                "njutest-law-assertion:active-boundary",
            ],
            Self::Stopping => &["njutest-law-assertion:stopping-absorbing"],
            Self::AttemptDuration => &[
                "njutest-law-assertion:duration-sum-constructs",
                "njutest-law-assertion:duration-sum-exact",
                "njutest-law-assertion:duration-overflow-refused",
            ],
            Self::AttemptLedger => &[
                "njutest-law-assertion:attempt-retry-constructs",
                "njutest-law-assertion:attempt-count-closed",
                "njutest-law-assertion:attempt-retried-derived",
                "njutest-law-assertion:attempt-result-derived",
            ],
            Self::CancellationBeforeRetry => &[
                "njutest-law-assertion:pre-retry-cancel-final",
                "njutest-law-assertion:pre-retry-cancel-single",
                "njutest-law-assertion:pre-retry-count-one",
                "njutest-law-assertion:pre-retry-result-retained",
            ],
            Self::CancelledRetry => &[
                "njutest-law-assertion:cancelled-retry-constructs",
                "njutest-law-assertion:cancelled-retry-not-run",
                "njutest-law-assertion:cancelled-retry-retained",
            ],
            Self::EqualOutcomes => &[
                "njutest-law-assertion:tie-forward-present",
                "njutest-law-assertion:tie-reverse-present",
                "njutest-law-assertion:tie-forward-canonical",
                "njutest-law-assertion:tie-reverse-canonical",
            ],
            Self::Killed => &[
                "njutest-law-assertion:killed-right-absorbing",
                "njutest-law-assertion:killed-left-absorbing",
            ],
            Self::RetryReconciliation => &[
                "njutest-law-assertion:retry-cancelled",
                "njutest-law-assertion:retry-preserved",
                "njutest-law-assertion:retry-downgraded",
            ],
            Self::Survived => &["njutest-law-assertion:survived-iff-all"],
            Self::Associative => &["njutest-law-assertion:join-associative"],
            Self::Commutative => &["njutest-law-assertion:join-commutative"],
            Self::Idempotent => &["njutest-law-assertion:join-idempotent"],
        }
    }
}

impl std::fmt::Display for Harness {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AuditError {
    #[error("cannot read Kani export {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Kani export is not the closed 0.68 JSON schema: {0}")]
    Json(serde_json::Error),
    #[error("Kani export metadata is not the pinned 0.68 release contract")]
    Metadata,
    #[error("Kani export project paths do not bind this rust-mutants workspace")]
    Project,
    #[error("Kani export toolchain is not the pinned backend")]
    Toolchain,
    #[error("Kani export {0} ledger is missing, duplicated, or substituted")]
    Ledger(Ledger),
    #[error("Kani metadata for {0} is not the selected production proof harness")]
    Harness(Harness),
    #[error("Kani reported an execution error for {0}")]
    Execution(Harness),
    #[error("Kani property counts for {0} do not equal its exact check ledger")]
    Properties(Harness),
    #[error("Kani backend evidence for {0} is malformed")]
    Backend(Harness),
    #[error("Kani aggregate verification summary is incomplete or contradictory")]
    Summary,
    #[error("Kani verification result for {0} is not successful and exact")]
    Result(Harness),
    #[error("Kani assertion ledger for {0} is empty, unreachable, or unsuccessful")]
    Assertion(Harness),
    #[error("Kani cover ledger for {0} is missing, unsatisfied, duplicated, or malformed")]
    Cover(Harness),
    #[error("Kani check identifiers for {0} are duplicated or noncanonical")]
    CheckId(Harness),
    #[error("Kani result arithmetic exceeded its evidence type for {0}")]
    Arithmetic(Harness),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Ledger {
    HarnessMetadata,
    ErrorDetails,
    PropertyDetails,
    Cbmc,
    VerificationResults,
}

impl std::fmt::Display for Ledger {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::HarnessMetadata => "harness-metadata",
            Self::ErrorDetails => "error-details",
            Self::PropertyDetails => "property-details",
            Self::Cbmc => "cbmc",
            Self::VerificationResults => "verification-results",
        })
    }
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
struct Metadata {
    version: String,
    timestamp: String,
    kani_version: String,
    target: String,
    build_mode: String,
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

#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct Nullable<T>(Option<T>);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HarnessMetadata {
    pretty_name: Harness,
    mangled_name: String,
    crate_name: String,
    source: Source,
    goto_file: String,
    attributes: Attributes,
    contract: Contract,
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
struct Source {
    file: String,
    start_line: u64,
    end_line: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Attributes {
    kind: String,
    should_panic: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    contracted_function_name: Nullable<String>,
    recursion_tracker: Nullable<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorDetail {
    harness_id: Harness,
    has_errors: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PropertyDetail {
    harness_id: Harness,
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
    harness_id: Harness,
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
    duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SummaryStatus {
    Completed,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HarnessResult {
    harness_id: Harness,
    status: Status,
    duration_ms: u64,
    checks: Vec<Check>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
enum Status {
    Failure,
    Covered,
    Satisfied,
    Success,
    Undetermined,
    Unknown,
    Unreachable,
    Uncovered,
    Unsatisfiable,
    Error,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Check {
    id: u64,
    function: String,
    status: Status,
    description: String,
    location: Location,
    category: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Location {
    file: String,
    line: String,
    column: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Coverage {
    enabled: bool,
}

/// Audits a complete Kani 0.68 export against every production law.
///
/// # Errors
/// Returns a typed refusal for unreadable bytes, schema drift, a missing or duplicated harness, an unreachable assertion, or an unsatisfied cover.
pub(crate) fn audit(path: &Path, workspace: &Path) -> Result<(), AuditError> {
    let bytes = std::fs::read(path).map_err(|source| AuditError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    audit_bytes(&bytes, workspace)
}

fn audit_bytes(bytes: &[u8], workspace: &Path) -> Result<(), AuditError> {
    let value = crate::strictjson::from_slice(bytes).map_err(AuditError::Json)?;
    let document = serde_json::from_value::<Document>(value).map_err(AuditError::Json)?;
    validate_context(&document, workspace)?;
    validate_ledgers(&document)?;
    validate_harnesses(&document)?;
    validate_summary(&document.verification_results.summary)?;
    for harness in Harness::ALL {
        let properties = entry(
            &document.property_details,
            |item| item.harness_id,
            harness,
            Ledger::PropertyDetails,
        )?;
        let backend = entry(
            &document.cbmc,
            |item| item.harness_id,
            harness,
            Ledger::Cbmc,
        )?;
        let result = entry(
            &document.verification_results.results,
            |item| item.harness_id,
            harness,
            Ledger::VerificationResults,
        )?;
        validate_result(properties, backend, result)?;
    }
    Ok(())
}

fn validate_context(document: &Document, workspace: &Path) -> Result<(), AuditError> {
    if document.metadata.version != EXPORT_VERSION
        || document.metadata.kani_version != KANI_VERSION
        || document.metadata.timestamp.trim().is_empty()
        || !TARGETS.contains(&document.metadata.target.as_str())
        || document.metadata.build_mode != "release"
        || document.coverage.enabled
    {
        return Err(AuditError::Metadata);
    }
    let workspace_text = workspace.to_str().ok_or(AuditError::Project)?;
    let output = Path::new(&document.project.output_dir);
    if !matches!(document.project.crate_name.as_slice(), [name] if name == CRATE_NAME)
        || document.project.workspace_root != workspace_text
        || !clean_absolute(output)
        || !output.starts_with(workspace.join("target/kani"))
    {
        return Err(AuditError::Project);
    }
    if document.tools.kani != KANI_VERSION
        || document.tools.rustc != RUSTC_VERSION
        || document.tools.cbmc != CBMC_VERSION
        || document.tools.goto_cc != GOTO_CC_VERSION
        || document.tools.goto_instrument != GOTO_INSTRUMENT_VERSION
        || !matches!(document.tools.solvers.as_slice(), [Solver { name, version: Nullable(None) }] if name == SOLVER)
    {
        return Err(AuditError::Toolchain);
    }
    Ok(())
}

fn validate_ledgers(document: &Document) -> Result<(), AuditError> {
    exact_ledger(
        &document.harness_metadata,
        |entry| entry.pretty_name,
        Ledger::HarnessMetadata,
    )?;
    exact_ledger(
        &document.error_details,
        |entry| entry.harness_id,
        Ledger::ErrorDetails,
    )?;
    exact_ledger(
        &document.property_details,
        |entry| entry.harness_id,
        Ledger::PropertyDetails,
    )?;
    exact_ledger(&document.cbmc, |entry| entry.harness_id, Ledger::Cbmc)?;
    exact_ledger(
        &document.verification_results.results,
        |entry| entry.harness_id,
        Ledger::VerificationResults,
    )
}

fn exact_ledger<T>(
    entries: &[T],
    harness: impl Fn(&T) -> Harness,
    ledger: Ledger,
) -> Result<(), AuditError> {
    let observed = entries.iter().map(harness).collect::<BTreeSet<_>>();
    let expected = Harness::ALL.into_iter().collect::<BTreeSet<_>>();
    if entries.len() != Harness::ALL.len() || observed != expected {
        return Err(AuditError::Ledger(ledger));
    }
    Ok(())
}

fn entry<T>(
    entries: &[T],
    harness_of: impl Fn(&T) -> Harness,
    harness: Harness,
    ledger: Ledger,
) -> Result<&T, AuditError> {
    entries
        .iter()
        .find(|entry| harness_of(entry) == harness)
        .ok_or(AuditError::Ledger(ledger))
}

fn validate_harnesses(document: &Document) -> Result<(), AuditError> {
    let output = Path::new(&document.project.output_dir);
    for expected in Harness::ALL {
        let metadata = entry(
            &document.harness_metadata,
            |item| item.pretty_name,
            expected,
            Ledger::HarnessMetadata,
        )?;
        let errors = entry(
            &document.error_details,
            |item| item.harness_id,
            expected,
            Ledger::ErrorDetails,
        )?;
        let goto = Path::new(&metadata.goto_file);
        if metadata.pretty_name != expected
            || metadata.mangled_name.trim().is_empty()
            || metadata.crate_name != CRATE_NAME
            || metadata.source.file != expected.source()
            || metadata.source.start_line == 0
            || metadata.source.end_line < metadata.source.start_line
            || !clean_absolute(goto)
            || !goto.starts_with(output)
            || metadata.attributes.kind != "Proof"
            || metadata.attributes.should_panic
            || metadata.contract.contracted_function_name.0.is_some()
            || metadata.contract.recursion_tracker.0.is_some()
            || metadata.has_loop_contracts.0
            || metadata.is_automatically_generated.0
            || metadata.is_bounded.0
            || metadata.is_ctor_based.0
        {
            return Err(AuditError::Harness(expected));
        }
        if errors.harness_id != expected || errors.has_errors {
            return Err(AuditError::Execution(expected));
        }
    }
    Ok(())
}

fn validate_summary(summary: &Summary) -> Result<(), AuditError> {
    let expected =
        u64::try_from(Harness::ALL.len()).map_err(|_conversion_failure| AuditError::Summary)?;
    if summary.total_harnesses != expected
        || summary.executed != expected
        || summary.status != SummaryStatus::Completed
        || summary.successful != expected
        || summary.failed != 0
        || summary.duration_ms == 0
    {
        return Err(AuditError::Summary);
    }
    Ok(())
}

fn validate_result(
    properties: &PropertyDetail,
    backend: &Cbmc,
    result: &HarnessResult,
) -> Result<(), AuditError> {
    let harness = result.harness_id;
    if properties.harness_id != harness || backend.harness_id != harness {
        return Err(AuditError::Result(harness));
    }
    validate_backend(backend, harness)?;
    if result.status != Status::Success || result.duration_ms == 0 || result.checks.is_empty() {
        return Err(AuditError::Result(harness));
    }
    let observations = validate_checks(result, harness)?;
    validate_counts(&properties.property_details, observations, harness)
}

fn validate_backend(backend: &Cbmc, harness: Harness) -> Result<(), AuditError> {
    let timings = [
        backend.stats.runtime_symex_s.0,
        backend.stats.runtime_postprocess_equation_s.0,
        backend.stats.runtime_convert_ssa_s.0,
        backend.stats.runtime_post_process_s.0,
        backend.stats.runtime_solver_s.0,
        backend.stats.runtime_decision_procedure_s.0,
    ];
    if backend.metadata.version != "6.11.0"
        || backend.metadata.os_info.trim().is_empty()
        || backend.configuration.object_bits != 16
        || backend.configuration.solver != SOLVER
        || backend.stats.size_program_expression == 0
        || backend.stats.slicing_removed_assignments > backend.stats.size_program_expression
        || backend.stats.vccs_remaining > backend.stats.vccs_generated
        || timings.into_iter().any(|timing| match timing {
            Some(value) => !value.is_finite() || value < 0.0,
            None => false,
        })
    {
        return Err(AuditError::Backend(harness));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct ObservedCounts {
    passed: u64,
    satisfied: u64,
}

fn validate_checks(result: &HarnessResult, harness: Harness) -> Result<ObservedCounts, AuditError> {
    let mut ids = BTreeSet::new();
    let mut covers = BTreeSet::new();
    let mut law_assertions = BTreeSet::new();
    let mut passed = 0u64;
    let mut satisfied = 0u64;
    for check in &result.checks {
        if check.id == 0 || !ids.insert(check.id) {
            return Err(AuditError::CheckId(harness));
        }
        if check.category.trim().is_empty()
            || check.function.trim().is_empty()
            || check.description.trim().is_empty()
            || !valid_location(&check.location)
        {
            return Err(AuditError::Result(harness));
        }
        if check.category == "cover" {
            if check.status != Status::Satisfied
                || check.function != harness.name()
                || !law_location(&check.location, harness)
                || (!check.description.starts_with(BRANCH_COVER_PREFIX)
                    && check.description != REACHED_COVER)
                || !covers.insert(check.description.as_str())
            {
                return Err(AuditError::Cover(harness));
            }
            satisfied = increment(satisfied, harness)?;
            continue;
        }
        if check.status != Status::Success {
            return Err(AuditError::Result(harness));
        }
        passed = increment(passed, harness)?;
        if check.description.starts_with(ASSERTION_PREFIX)
            && (check.category != "assertion"
                || check.function != harness.name()
                || !law_location(&check.location, harness)
                || !law_assertions.insert(check.description.as_str()))
        {
            return Err(AuditError::Assertion(harness));
        }
    }
    let expected_assertions = harness
        .expected_assertions()
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if law_assertions != expected_assertions {
        return Err(AuditError::Assertion(harness));
    }
    let expected_covers = harness
        .expected_covers()
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if covers != expected_covers {
        return Err(AuditError::Cover(harness));
    }
    Ok(ObservedCounts { passed, satisfied })
}

fn validate_counts(
    counts: &PropertyCounts,
    observed: ObservedCounts,
    harness: Harness,
) -> Result<(), AuditError> {
    let classified = observed
        .passed
        .checked_add(observed.satisfied)
        .ok_or(AuditError::Arithmetic(harness))?;
    if counts.total_properties != classified
        || counts.passed != observed.passed
        || counts.satisfied != observed.satisfied
        || counts.failed != 0
        || counts.unreachable != 0
        || counts.undetermined != 0
        || counts.solver_error != 0
        || counts.unsatisfiable != 0
        || counts.covered != 0
        || counts.uncovered != 0
    {
        return Err(AuditError::Properties(harness));
    }
    Ok(())
}

fn increment(value: u64, harness: Harness) -> Result<u64, AuditError> {
    value.checked_add(1).ok_or(AuditError::Arithmetic(harness))
}

fn valid_location(location: &Location) -> bool {
    !location.file.trim().is_empty()
        && valid_coordinate(&location.line)
        && valid_coordinate(&location.column)
}

fn valid_coordinate(coordinate: &str) -> bool {
    coordinate == "unknown" || coordinate.parse::<u64>().is_ok()
}

fn law_location(location: &Location, harness: Harness) -> bool {
    let line = location.line.parse::<u64>();
    let column = location.column.parse::<u64>();
    location.file == harness.source()
        && matches!((line, column), (Ok(line), Ok(column)) if line > 0 && column > 0)
}

fn clean_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path.components().all(|component| {
            matches!(
                component,
                Component::RootDir | Component::Prefix(_) | Component::Normal(_)
            )
        })
}

#[cfg(test)]
mod tests {
    use njutest_devkit::result::{ResultState, result_state};
    use serde_json::{Value, json};

    use super::{AuditError, Harness, Status, audit_bytes};

    const WORKSPACE: &str = "/workspace";

    fn metadata_entries() -> Vec<Value> {
        Harness::ALL
            .into_iter()
            .enumerate()
            .map(|(index, harness)| {
                json!({
                    "pretty_name": harness.name(),
                    "mangled_name": format!("mangled-{index}"),
                    "crate_name": "rust_mutants",
                    "source": {"file": harness.source(), "start_line": 1, "end_line": 2},
                    "goto_file": format!("{WORKSPACE}/target/kani/out/{index}.symtab.out"),
                    "attributes": {"kind": "Proof", "should_panic": false},
                    "contract": {"contracted_function_name": null, "recursion_tracker": null},
                    "has_loop_contracts": false,
                    "is_automatically_generated": false,
                    "is_bounded": false,
                    "is_ctor_based": false
                })
            })
            .collect()
    }

    fn error_entries() -> Vec<Value> {
        Harness::ALL
            .into_iter()
            .map(|harness| json!({"harness_id": harness.name(), "has_errors": false}))
            .collect()
    }

    fn property_entries() -> Vec<Value> {
        Harness::ALL
            .into_iter()
            .map(|harness| {
                let passed = harness.expected_assertions().len();
                let satisfied = harness.expected_covers().len();
                let total = harness
                    .expected_assertions()
                    .iter()
                    .chain(harness.expected_covers())
                    .count();
                json!({
                    "harness_id": harness.name(),
                    "property_details": {
                        "total_properties": total,
                        "passed": passed,
                        "failed": 0,
                        "unreachable": 0,
                        "undetermined": 0,
                        "solver_error": 0,
                        "satisfied": satisfied,
                        "unsatisfiable": 0,
                        "covered": 0,
                        "uncovered": 0
                    }
                })
            })
            .collect()
    }

    fn backend_entries() -> Vec<Value> {
        Harness::ALL
            .into_iter()
            .map(|harness| {
                json!({
                    "harness_id": harness.name(),
                    "cbmc_metadata": {"version": "6.11.0", "os_info": "fixture unix"},
                    "configuration": {"object_bits": 16, "solver": "cadical"},
                    "cbmc_stats": {
                        "runtime_symex_s": 0.1,
                        "size_program_expression": 2,
                        "slicing_removed_assignments": 1,
                        "vccs_generated": 2,
                        "vccs_remaining": 1,
                        "runtime_postprocess_equation_s": 0.1,
                        "runtime_convert_ssa_s": 0.1,
                        "runtime_post_process_s": 0.1,
                        "runtime_solver_s": 0.1,
                        "runtime_decision_procedure_s": 0.1
                    }
                })
            })
            .collect()
    }

    fn result_entries() -> Vec<Value> {
        Harness::ALL
            .into_iter()
            .map(|harness| {
                let assertions = harness
                    .expected_assertions()
                    .iter()
                    .map(|description| (*description, "Success", "1", "assertion"));
                let covers = harness
                    .expected_covers()
                    .iter()
                    .map(|description| (*description, "Satisfied", "2", "cover"));
                let checks = (1u64..)
                    .zip(assertions.chain(covers))
                    .map(|(id, (description, status, line, category))| {
                        json!({
                            "id": id,
                            "function": harness.name(),
                            "status": status,
                            "description": description,
                            "location": {"file": harness.source(), "line": line, "column": "1"},
                            "category": category
                        })
                    })
                    .collect::<Vec<_>>();
                json!({
                    "harness_id": harness.name(),
                    "status": "Success",
                    "duration_ms": 1,
                    "checks": checks
                })
            })
            .collect()
    }

    fn fixture() -> Value {
        json!({
            "metadata": {
                "version": "1.0",
                "timestamp": "2026-09-20T00:00:00Z",
                "kani_version": "0.68.0",
                "target": "aarch64-apple-darwin",
                "build_mode": "release"
            },
            "project": {
                "crate_name": ["rust_mutants"],
                "workspace_root": WORKSPACE,
                "output_dir": format!("{WORKSPACE}/target/kani/out")
            },
            "tools": {
                "kani": "0.68.0",
                "rustc": "rustc 1.100.0-nightly (8925ea358 2026-08-20)",
                "cbmc": "6.11.0 (cbmc-6.11.0)",
                "goto_cc": "clang version 21.0.0 (goto-cc 6.11.0 (cbmc-6.11.0))",
                "goto_instrument": "6.11.0 (cbmc-6.11.0)",
                "solvers": [{"name": "cadical", "version": null}]
            },
            "harness_metadata": metadata_entries(),
            "error_details": error_entries(),
            "property_details": property_entries(),
            "cbmc": backend_entries(),
            "verification_results": {
                "summary": {
                    "total_harnesses": 15,
                    "executed": 15,
                    "status": "completed",
                    "successful": 15,
                    "failed": 0,
                    "duration_ms": 15
                },
                "results": result_entries()
            },
            "coverage": {"enabled": false}
        })
    }

    fn outcome(value: &Value) -> Result<(), AuditError> {
        let encoded = serde_json::to_vec(value).map_err(AuditError::Json)?;
        audit_bytes(&encoded, std::path::Path::new(WORKSPACE))
    }

    fn replace(value: &mut Value, pointer: &str, replacement: Value) {
        let selected = value.pointer_mut(pointer);
        assert!(selected.is_some(), "fixture pointer {pointer} must exist");
        let Some(selected) = selected else { return };
        *selected = replacement;
    }

    fn remove(value: &mut Value, pointer: &str) {
        let split = pointer.rsplit_once('/');
        assert!(split.is_some(), "fixture pointer has a parent");
        let Some((parent, field)) = split else { return };
        let object = value.pointer_mut(parent).and_then(Value::as_object_mut);
        assert!(
            object.is_some(),
            "fixture pointer parent {parent} must be an object"
        );
        let Some(object) = object else { return };
        let removed = object.remove(field);
        assert!(removed.is_some(), "fixture field {field} must exist");
    }

    fn assert_refused(value: &Value) {
        let audited = outcome(value);
        assert_eq!(
            result_state(&audited),
            ResultState::Refused,
            "hostile Kani export was accepted"
        );
    }

    #[test]
    fn exact_complete_export_is_accepted() {
        let audited = outcome(&fixture());
        assert_eq!(result_state(&audited), ResultState::Returned, "{audited:?}");
    }

    #[test]
    fn duplicate_keys_and_schema_drift_are_refused_before_redecision() {
        for bytes in [
            br#"{"metadata":{},"metadata":{}}"#.as_slice(),
            br#"{"metadata":{"version":"1","version":"2"}}"#.as_slice(),
        ] {
            let audited = audit_bytes(bytes, std::path::Path::new(WORKSPACE));
            assert!(
                matches!(audited, Err(AuditError::Json(error)) if error.to_string().contains("duplicate JSON object key"))
            );
        }

        let mut extra = fixture();
        let object = extra.as_object_mut();
        assert!(object.is_some());
        let Some(object) = object else { return };
        let replaced = object.insert("future_schema_field".to_owned(), Value::Bool(true));
        assert!(replaced.is_none());
        assert_refused(&extra);

        let mut missing = fixture();
        remove(&mut missing, "/verification_results/summary/executed");
        assert_refused(&missing);
    }

    #[test]
    fn harness_ledgers_are_exactly_once_and_order_independent() {
        let mut missing = fixture();
        let array = missing
            .pointer_mut("/property_details")
            .and_then(Value::as_array_mut);
        assert!(array.is_some());
        let Some(array) = array else { return };
        let removed = array.pop();
        assert!(removed.is_some());
        assert_refused(&missing);

        let mut duplicated = fixture();
        let array = duplicated
            .pointer_mut("/verification_results/results")
            .and_then(Value::as_array_mut);
        assert!(array.is_some());
        let Some(array) = array else { return };
        let first = array.first().cloned();
        assert!(first.is_some());
        let Some(first) = first else { return };
        array.push(first);
        assert_refused(&duplicated);

        let mut swapped = fixture();
        let array = swapped
            .pointer_mut("/harness_metadata")
            .and_then(Value::as_array_mut);
        assert!(array.is_some());
        let Some(array) = array else { return };
        array.swap(0, 1);
        let audited = outcome(&swapped);
        assert_eq!(result_state(&audited), ResultState::Returned, "{audited:?}");
    }

    #[test]
    fn unreachable_unknown_and_failed_assertions_are_refused() {
        for status in [
            Status::Unreachable,
            Status::Undetermined,
            Status::Unknown,
            Status::Failure,
            Status::Error,
        ] {
            let mut hostile = fixture();
            replace(
                &mut hostile,
                "/verification_results/results/0/checks/0/status",
                json!(format!("{status:?}")),
            );
            assert_refused(&hostile);
        }

        let mut absent = fixture();
        let checks = absent
            .pointer_mut("/verification_results/results/0/checks")
            .and_then(Value::as_array_mut);
        assert!(checks.is_some());
        let Some(checks) = checks else { return };
        checks.retain(|check| check.get("category") != Some(&json!("assertion")));
        replace(
            &mut absent,
            "/property_details/0/property_details/total_properties",
            json!(1),
        );
        replace(
            &mut absent,
            "/property_details/0/property_details/passed",
            json!(0),
        );
        assert_refused(&absent);
    }

    #[test]
    fn every_harness_needs_its_exact_named_cover_set_and_every_cover_must_be_sat() {
        for status in [
            Status::Unsatisfiable,
            Status::Uncovered,
            Status::Covered,
            Status::Success,
        ] {
            let mut hostile = fixture();
            replace(
                &mut hostile,
                "/verification_results/results/0/checks/1/status",
                json!(format!("{status:?}")),
            );
            assert_refused(&hostile);
        }

        let mut missing_reached = fixture();
        replace(
            &mut missing_reached,
            "/verification_results/results/0/checks/1/description",
            json!("njutest-law-branch:only"),
        );
        assert_refused(&missing_reached);

        let mut duplicate_reached = fixture();
        let checks = duplicate_reached
            .pointer_mut("/verification_results/results/0/checks")
            .and_then(Value::as_array_mut);
        assert!(checks.is_some());
        let Some(checks) = checks else { return };
        let reached = checks.get(1).cloned();
        assert!(reached.is_some());
        let Some(mut reached) = reached else { return };
        replace(&mut reached, "/id", json!(3));
        checks.push(reached);
        assert_refused(&duplicate_reached);
    }

    #[test]
    fn equal_counts_cannot_hide_a_substituted_assertion_or_branch_cover() {
        let mut assertion = fixture();
        replace(
            &mut assertion,
            "/verification_results/results/2/checks/0/description",
            json!("njutest-law-assertion:substituted"),
        );
        assert_refused(&assertion);

        let mut cover = fixture();
        replace(
            &mut cover,
            "/verification_results/results/2/checks/2/description",
            json!("njutest-law-branch:substituted"),
        );
        assert_refused(&cover);
    }

    #[test]
    fn an_unbound_target_triple_is_refused() {
        let mut hostile = fixture();
        replace(
            &mut hostile,
            "/metadata/target",
            json!("x86_64-unknown-freebsd"),
        );
        assert_refused(&hostile);
    }

    #[test]
    fn contradictory_property_totals_and_solver_failures_are_refused() {
        for (pointer, replacement) in [
            ("/property_details/0/property_details/unreachable", json!(1)),
            (
                "/property_details/0/property_details/unsatisfiable",
                json!(1),
            ),
            (
                "/property_details/0/property_details/solver_error",
                json!(1),
            ),
            (
                "/property_details/0/property_details/total_properties",
                json!(3),
            ),
        ] {
            let mut hostile = fixture();
            replace(&mut hostile, pointer, replacement);
            assert_refused(&hostile);
        }
    }
}
