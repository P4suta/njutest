// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run report: one completed run of every mutant, as a document and as lines a person reads.

use std::collections::{BTreeMap, BTreeSet};

use crate::session::Session;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::outcome::Outcome;
use crate::report::catalog::{
    MutantDocument, RejectionDocument, SelectionDocument, SkipDocument, WorkspaceDocument,
};
use crate::run::{
    Finding, FindingKind, NotRunReason, Run, Standing, stale_detail, unmatched_detail,
};

/// The name of the shape, so a reader can tell versions apart.
pub const DOCUMENT_TYPE: &str = "rust-mutants/run-report";

/// The version of that shape.
pub const SCHEMA_VERSION: u32 = 3;

/// The file one run writes under its own directory.
pub const FILE_NAME: &str = "run-report-v1.json";

/// The pointer file that names the newest run.
pub const LATEST_FILE_NAME: &str = "latest.json";

/// Reads one complete run document without allowing a repeated object key to replace an earlier fact.
///
/// # Errors
/// The input is not one exact current run document, contains trailing data,
/// or repeats an object key at any depth.
pub fn parse(text: &str) -> Result<RunDocument, serde_json::Error> {
    crate::strictjson::decode_str(text)
}

/// One completed run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunDocument {
    /// Names the shape.
    pub document_type: String,
    /// The version of that shape.
    pub schema_version: u32,
    /// The engine that produced it.
    pub tool_version: String,
    /// When it ran and how it ended.
    pub run: RunMeta,
    /// The tree that was read.
    pub workspace: WorkspaceDocument,
    /// What the run asked for.
    pub selection: SelectionDocument,
    /// What the run counted.
    pub accounting: Accounting,
    /// The share of decided mutants the tests noticed, absent when the run decided nothing.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub score: Option<ScoreDocument>,
    /// The test targets the run built, which is what every mutant could have been asked against.
    pub targets: Vec<TargetDocument>,
    /// How many tests the run started to establish that a set of them answers on its own, which is work no mutation asked for and every narrowed execution rests on.
    pub established_tests: u64,
    /// One record per non-refused candidate the run accounts for, in catalog order.
    pub mutants: Vec<RunMutantDocument>,
    /// Every candidate the compiler refused.
    pub rejections: Vec<RejectionDocument>,
    /// Every place discovery passed over.
    pub skips: Vec<SkipDocument>,
    /// The claims a reviewer declared, as the run left them.
    pub expectations: Vec<ExpectationDocument>,
    /// Everything that stops the run from being clean.
    pub findings: Vec<FindingDocument>,
    /// What the build's own rustc says its target is, kept to the names a target alone decides, sorted, which every claim's `where` was judged against (ADR 0042).
    pub facts: Vec<String>,
}

/// When a run ran and how it ended.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunMeta {
    /// The run's identity, which is also its directory name.
    pub id: String,
    /// When it started.
    pub started_at: String,
    /// When it finished.
    pub finished_at: String,
    /// How long the executions took together.
    pub duration_ms: u64,
    /// Whether the run stopped because it was asked to.
    pub interrupted: bool,
    /// The exit code the run earned.
    pub exit_code: u8,
    /// Which part of the catalog the run was about, absent for the whole of it.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub shard: Option<String>,
    /// How wide the run measured.
    pub jobs: JobsDocument,
}

/// How wide a run measured, as a report says it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobsDocument {
    /// What was asked for, as a person writes it: a count, `auto`, or `all`.
    pub asked: String,
    /// How many mutants were measured at once.
    pub used: u32,
}

/// What a run counted.
/// Every mutant is in exactly one of the outcome columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accounting {
    /// How many candidate rows the run accounts for, excluding compiler refusals.
    pub cataloged: u32,
    /// How many candidates the compiler refused.
    pub refused: Beside,
    /// How many places discovery passed over.
    pub skipped: Beside,
    /// How many mutants an execution reached a verdict on.
    pub executed: Beside,
    /// How many a test failed on.
    pub killed: Of,
    /// How many every test passed on.
    pub survived: Of,
    /// How many reached the per-process guard-take limit without deciding the mutation.
    pub step_limit_reached: Of,
    /// How many this machine stopped waiting for, twice over.
    pub waited: Of,
    /// How many the run could not decide.
    pub inconclusive: Of,
    /// How many the harness itself failed on.
    pub errored: Of,
    /// How many never ran.
    pub not_run: Of,
    /// How many of those never ran because no measured target reaches them.
    pub unreached: Within,
    /// How many of those never ran because a proof removed every target that could have noticed them.
    pub discharged: Within,
    /// How many survivors a reviewer had declared, and the run confirmed.
    pub expected: Within,
}

/// One count of a set that adds up to a stated whole.
///
/// The three counts in an accounting are three different things, and every renderer that put them in one list got the same defect: a reader adding a column of them gets more than there are.
/// They are told apart here rather than in each renderer, because there were four renderers and they disagreed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Of(u32);

/// One count of *some of* another count.
/// Adding it to that count's siblings counts it twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Within(u32);

/// One count of something outside the whole: neither a part of it nor a part of a part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Beside(u32);

/// A report accounting value exceeded its durable `u32` representation or contradicted a subset relation required by that representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("run accounting exceeds or contradicts its durable counters")]
pub struct CountOverflowError;

/// Declares the three kinds with the same shape, and no `Display`.
///
/// None of them can be written into a message by `{}`.
/// That is the point: a renderer reaching for one has to say which kind it is holding, and the type is what carries the answer to the place the columns are laid out.
macro_rules! counted {
    ($($name:ident),+) => {
        $(impl $name {
            /// The number, for arithmetic and for a renderer that has said which kind it is.
            #[must_use]
            pub const fn count(self) -> u32 {
                self.0
            }

            /// One of these.
            #[must_use]
            pub const fn new(count: u32) -> Self {
                Self(count)
            }

            /// Counts one more without fabricating a maximum value.
            ///
            /// # Errors
            /// Refuses when the durable counter is exhausted.
            pub const fn raise(&mut self) -> Result<(), CountOverflowError> {
                match self.0.checked_add(1) {
                    Some(raised) => {
                        self.0 = raised;
                        Ok(())
                    }
                    None => Err(CountOverflowError),
                }
            }
        }

        impl From<u32> for $name {
            fn from(count: u32) -> Self {
                Self(count)
            }
        })+
    };
}

counted!(Of, Within, Beside);

/// The share of decided mutants the tests noticed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreDocument {
    /// Mutants a test killed.
    pub detected: u32,
    /// Detected plus survived.
    pub decided: u32,
    /// `detected / decided`.
    pub value: f64,
}

/// One non-refused candidate and what the run established about it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is an independent fact about one judged mutant that the published report states"
)]
pub struct RunMutantDocument {
    /// The dense catalog index the guards name.
    pub index: u32,
    /// The full identity.
    pub id: String,
    /// The short identity a person types.
    pub display_id: String,
    /// The workspace-relative path.
    pub path: String,
    /// The package that owns the file.
    pub package: String,
    /// The family the rule belongs to.
    pub family: String,
    /// The rule's name.
    pub rule: String,
    /// The item the mutation sits in, which is how a finding is matched across an edit.
    pub item: String,
    /// The rule's version, which enters the identity.
    pub rule_version: u32,
    /// The 1-based line of the edit.
    pub line: u32,
    /// The 1-based byte column of the edit.
    pub column: u32,
    /// The first byte of the edit.
    pub start_byte: u32,
    /// One past the last byte of the edit.
    pub end_byte: u32,
    /// The SHA-256 of the file the edit was cut from, which is what re-minting the identity needs.
    pub source_digest: String,
    /// The bytes the edit replaces.
    pub original: String,
    /// What they become.
    pub replacement: String,
    /// What the execution says.
    pub outcome: Outcome,
    /// The verified runtime notice when this execution reached its step limit.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub step_notice: Option<crate::execute::StepLimitNotice>,
    /// The target that ran, empty when none did.
    pub target: String,
    /// The exit status of the last execution.
    pub exit_code: i32,
    /// How long every execution of this mutant took together.
    pub duration_ms: u64,
    /// How many tests ran, when the harness said.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub tests_run: Option<u32>,
    /// Every test that failed with the mutant active, which is what noticed it.
    pub killed_by: Vec<String>,
    /// The signal the last execution died from, on the platforms that have them.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub signal: Option<i32>,
    /// Whether a first timeout was retried serially before the outcome was believed.
    pub retried: bool,
    /// Whether the harness had already answered when the clock ended the process: a verdict the harness gave, from a process that would not end.
    pub lingered: bool,
    /// Why it was never executed, when it was not: `unreached`, `discharged`, or `interrupted`.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub not_run_reason: Option<NotRunReason>,
    /// Which targets could have noticed it, and which of them ran.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub route: Option<RouteDocument>,
    /// What comparison of the compiler artifacts established.
    /// Never a claim that the mutation is equivalent.
    pub identical: crate::run::CodegenIdentity,
    /// Whether a reviewer declared this outcome in advance and the run confirmed the claim.
    pub expected: bool,
    /// Whether no measured target reaches it, which is why it never ran.
    pub unreached: bool,
    /// The run that established this, when it was not this one.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub source_run_id: Option<String>,
}

/// One declared expectation, as the run left it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectationDocument {
    /// The identity, prefix, or locator the file wrote, as a reader reads it.
    pub id: String,
    /// The locator the file wrote, when it wrote one.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub locator: Option<LocatorDocument>,
    /// Why the reviewer claims the outcome.
    pub reason: String,
    /// The outcome claimed.
    pub outcome: Outcome,
    /// The mutant it resolved to, when it resolved.
    /// The one that decided the standing, when it named several.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub mutant: Option<String>,
    /// How many mutants the claim was resolved against, when the locator stated a count.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub covered: Option<u32>,
    /// Whether the claim held: `met`, `stale`, or `unmatched`.
    pub standing: String,
    /// What the run established instead, when the claim was contradicted.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub actual: Option<Outcome>,
    /// Why the identity resolved to nothing, or which fact did not hold, when either is so.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub why: Option<String>,
    /// Where the claim is judged, when the file said.
    #[serde(
        rename = "where",
        deserialize_with = "crate::strictjson::required_option"
    )]
    pub holds: Option<WhereDocument>,
}

/// The facts a claim was established under, as a report writes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WhereDocument {
    /// The predicate over the target, as it reads back.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub cfg: Option<String>,
    /// Each name of the tests' environment the claim names, with the value it holds under.
    pub env: BTreeMap<String, String>,
}

/// One expectation locator in the current run-report wire shape.
///
/// This is deliberately distinct from the configuration locator: configuration may omit its optional hints, while a v1 report writes those keys explicitly as either a value or `null` and rejects a missing key on read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocatorDocument {
    /// The workspace-relative path.
    pub path: String,
    /// The item suffix.
    pub item: String,
    /// The rule name.
    pub rule: String,
    /// The text the rule replaces.
    pub original: String,
    /// The optional line hint, represented by an explicit nullable key.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub line: Option<u32>,
    /// The optional cardinality hint, represented by an explicit nullable key.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub count: Option<u32>,
}

impl From<&crate::session::Locator> for LocatorDocument {
    fn from(locator: &crate::session::Locator) -> Self {
        Self {
            path: locator.path.clone(),
            item: locator.item.clone(),
            rule: locator.rule.clone(),
            original: locator.original.clone(),
            line: locator.line,
            count: locator.count,
        }
    }
}

/// One thing that stops a run from being clean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindingDocument {
    /// What kind of hole it is.
    pub kind: FindingKind,
    /// The mutant it is about, when it is about one.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub mutant: Option<String>,
    /// One sentence a reader can act on.
    pub detail: String,
}

/// Why a run document cannot be trusted as one coherent answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DocumentError {
    /// The document was written to another version of this schema, which is a matter of when rather than a contradiction.
    #[error(
        "the run report was written as schema version {found}, and this release reads {}; run again to write one it reads",
        SCHEMA_VERSION
    )]
    SchemaVersion {
        /// The version the document says it was written to.
        found: u32,
    },
    /// The document names another shape.
    #[error("the run report header has an invalid {field}")]
    Header {
        /// The invalid header field.
        field: &'static str,
    },
    /// A catalog row's identity does not follow from the mutation it describes.
    #[error("catalog row {mutant} has an invalid identity, display identity, or index")]
    CatalogRow {
        /// The row's claimed identity.
        mutant: String,
    },
    /// The catalog cardinality cannot fit the fixed-width index wire.
    #[error("the catalog contains more than u32::MAX rows")]
    CatalogTooLarge,
    /// Exact accounting exceeded a durable counter or contradicted a subset relation.
    #[error(transparent)]
    Count(#[from] CountOverflowError),
    /// The shard header is not a valid `K/N` selection.
    #[error("the run report has an invalid shard {shard:?}")]
    Shard {
        /// The invalid shard spelling.
        shard: String,
    },
    /// An accepted row does not belong to the shard that claims it.
    #[error("catalog row {mutant} does not belong to shard {shard}")]
    ShardRow {
        /// The row outside the claimed shard.
        mutant: String,
        /// The parsed shard spelling.
        shard: String,
    },
    /// The accounting columns are not the fold of the rows beside them.
    #[error("the run report's {field} accounting does not equal its rows")]
    Accounting {
        /// The contradictory accounting field.
        field: &'static str,
    },
    /// The score is not killed over killed plus survived.
    #[error("the run report's score does not equal its decided rows")]
    Score,
    /// The exit code is not the one the interruption and findings earn.
    #[error("the run report exits {actual}, but its findings and interruption earn {expected}")]
    ExitCode {
        /// The code derived from the report's facts.
        expected: u8,
        /// The code carried by the report.
        actual: u8,
    },
    /// Step evidence was absent from a step-bound result, or attached to another outcome.
    #[error("mutant {mutant} carries step evidence that contradicts its outcome")]
    StepEvidence {
        /// The contradictory mutant.
        mutant: String,
    },
    /// Step evidence names another row, catalog, or configured allowance.
    #[error("mutant {mutant} carries step evidence with a mismatched {field:?}")]
    StepEvidenceMismatch {
        /// The row carrying the evidence.
        mutant: String,
        /// Which execution identity field disagrees.
        field: StepEvidenceField,
    },
    /// Reuse provenance was attached to an outcome the durable cache cannot establish.
    #[error("mutant {mutant} names a source run for an outcome that is not reusable")]
    ReuseProvenance {
        /// The contradictory mutant.
        mutant: String,
    },
    /// A not-run reason was attached to a result that did run.
    #[error("mutant {mutant} carries a not-run reason that contradicts its outcome")]
    NotRunReason {
        /// The contradictory mutant.
        mutant: String,
    },
    /// The legacy convenience flag and the typed reason disagree.
    #[error("mutant {mutant} gives two different answers about whether it was unreached")]
    Unreached {
        /// The contradictory mutant.
        mutant: String,
    },
    /// An interrupted reason appeared in a report that says the run was not interrupted.
    #[error(
        "mutant {mutant} says interruption stopped it, but the run says it was not interrupted"
    )]
    Interruption {
        /// The contradictory mutant.
        mutant: String,
    },
    /// The finding derived from a verdict is missing, duplicated, or of another kind.
    #[error("mutant {mutant} has findings that contradict its verdict")]
    FindingVerdict {
        /// The contradictory mutant.
        mutant: String,
    },
    /// A verdict finding names no row whose verdict could imply it.
    #[error("verdict finding {kind} names no judged mutant {mutant}")]
    UnknownFindingMutant {
        /// The finding kind.
        kind: FindingKind,
        /// The missing mutant identity, or `<none>`.
        mutant: String,
    },
    /// An expectation states a standing its other fields do not carry.
    #[error("the expectation for {id:?} is {standing:?}, but its fields say otherwise")]
    Claim {
        /// The identity the expectation wrote.
        id: String,
        /// The standing it states.
        standing: String,
    },
    /// A finding not derived from a verdict has no corresponding report fact.
    #[error("finding {kind} is not implied by the report")]
    FindingFact {
        /// The unsupported finding kind.
        kind: FindingKind,
    },
}

/// Which part of a verified step notice disagrees with its report row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepEvidenceField {
    /// The notice names another mutant.
    Mutant,
    /// The notice names another catalog.
    Catalog,
    /// The notice was produced under another configured allowance.
    Limit,
}

impl RunDocument {
    /// Checks the cross-field facts that a schema cannot express: step evidence,
    /// not-run reasons, and the finding each verdict necessarily implies.
    ///
    /// # Errors
    /// The first pair of facts that cannot both be true.
    pub fn validate(&self) -> Result<(), DocumentError> {
        self.validate_header()?;
        self.validate_catalog_rows()?;
        for one in &self.mutants {
            self.validate_mutant(one)?;
        }
        self.validate_verdict_finding_references()?;
        self.validate_nonverdict_findings()?;
        let expected_accounting = accounting_from_document(self)?;
        if let Some(field) = accounting_difference(&self.accounting, &expected_accounting) {
            return Err(DocumentError::Accounting { field });
        }
        if self.score != score_of(&expected_accounting)? {
            return Err(DocumentError::Score);
        }
        let expected_exit = exit_code_of(self);
        if self.run.exit_code != expected_exit {
            return Err(DocumentError::ExitCode {
                expected: expected_exit,
                actual: self.run.exit_code,
            });
        }
        Ok(())
    }

    fn validate_mutant(&self, one: &RunMutantDocument) -> Result<(), DocumentError> {
        if one.source_run_id.is_some()
            && !matches!(one.outcome, Outcome::Killed | Outcome::Survived)
        {
            return Err(DocumentError::ReuseProvenance {
                mutant: one.id.clone(),
            });
        }
        let has_step_evidence = one.step_notice.is_some();
        if has_step_evidence != (one.outcome == Outcome::StepLimitReached) {
            return Err(DocumentError::StepEvidence {
                mutant: one.id.clone(),
            });
        }
        if let Some(notice) = &one.step_notice {
            let field = if notice.mutant() != one.id {
                Some(StepEvidenceField::Mutant)
            } else if notice.catalog() != self.workspace.catalog_digest {
                Some(StepEvidenceField::Catalog)
            } else if self.selection.mutant_steps != Some(notice.limit()) {
                Some(StepEvidenceField::Limit)
            } else {
                None
            };
            if let Some(field) = field {
                return Err(DocumentError::StepEvidenceMismatch {
                    mutant: one.id.clone(),
                    field,
                });
            }
        }
        if one.outcome != Outcome::NotRun && one.not_run_reason.is_some() {
            return Err(DocumentError::NotRunReason {
                mutant: one.id.clone(),
            });
        }
        if one.unreached != (one.not_run_reason == Some(NotRunReason::Unreached)) {
            return Err(DocumentError::Unreached {
                mutant: one.id.clone(),
            });
        }
        if one.not_run_reason == Some(NotRunReason::Interrupted) && !self.run.interrupted {
            return Err(DocumentError::Interruption {
                mutant: one.id.clone(),
            });
        }
        let expected = verdict_finding(one, self.run.interrupted);
        let actual: Vec<FindingKind> = self
            .findings
            .iter()
            .filter(|finding| {
                finding.kind.is_verdict() && finding.mutant.as_deref() == Some(one.id.as_str())
            })
            .map(|finding| finding.kind)
            .collect();
        if actual.as_slice() != expected.as_slice() {
            return Err(DocumentError::FindingVerdict {
                mutant: one.id.clone(),
            });
        }
        Ok(())
    }

    fn validate_verdict_finding_references(&self) -> Result<(), DocumentError> {
        for finding in self
            .findings
            .iter()
            .filter(|finding| finding.kind.is_verdict())
        {
            let Some(mutant) = finding.mutant.as_deref() else {
                return Err(DocumentError::UnknownFindingMutant {
                    kind: finding.kind,
                    mutant: "<none>".to_owned(),
                });
            };
            if !self.mutants.iter().any(|one| one.id == mutant) {
                return Err(DocumentError::UnknownFindingMutant {
                    kind: finding.kind,
                    mutant: mutant.to_owned(),
                });
            }
        }
        Ok(())
    }

    fn validate_header(&self) -> Result<(), DocumentError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(DocumentError::SchemaVersion {
                found: self.schema_version,
            });
        }
        let width_is_one = match crate::run::Jobs::parse(&self.run.jobs.asked) {
            Ok(_asked) => self.run.jobs.used > 0,
            Err(_unknown) => false,
        };
        for (valid, field) in [
            (width_is_one, "jobs"),
            (self.document_type == DOCUMENT_TYPE, "document_type"),
            (!self.tool_version.is_empty(), "tool_version"),
            (
                crate::id::is_digest(&self.workspace.workspace_digest),
                "workspace_digest",
            ),
            (
                crate::id::is_digest(&self.workspace.catalog_digest),
                "catalog_digest",
            ),
        ] {
            if !valid {
                return Err(DocumentError::Header { field });
            }
        }
        Ok(())
    }

    fn validate_catalog_rows(&self) -> Result<(), DocumentError> {
        let mut indexes = BTreeSet::new();
        let mut identities = BTreeSet::new();
        let mut display_ids = BTreeSet::new();
        let mut previous = None;
        let registry = crate::rule::Registry::canonical();
        let shard = self
            .run
            .shard
            .as_deref()
            .map(crate::run::Shard::parse)
            .transpose()
            .map_err(|_error| DocumentError::Shard {
                shard: self.run.shard.clone().unwrap_or_default(),
            })?;
        for one in &self.mutants {
            if let Some(shard) = shard
                && !shard.holds(one.index)
            {
                return Err(DocumentError::ShardRow {
                    mutant: one.id.clone(),
                    shard: shard.to_string(),
                });
            }
            let identity = crate::id::Identity {
                path: one.path.clone(),
                rule_name: one.rule.clone(),
                rule_version: one.rule_version,
                span: crate::span::Span {
                    start: one.start_byte,
                    end: one.end_byte,
                },
                source_digest: one.source_digest.clone(),
                original_digest: crate::id::digest(one.original.as_bytes()),
                replacement_digest: crate::id::digest(one.replacement.as_bytes()),
            };
            let id_matches = identity.id().is_ok_and(|id| id.as_str() == one.id);
            let ordered = previous.is_none_or(|index| index < one.index);
            let unique = indexes.insert(one.index)
                && identities.insert(one.id.as_str())
                && display_ids.insert(one.display_id.as_str());
            let display_matches = crate::id::display_id_of(&one.id)
                .is_ok_and(|display_id| display_id == one.display_id);
            let rule_matches = registry.lookup(&one.rule).is_some_and(|rule| {
                rule.version == one.rule_version && rule.family.name() == one.family
            });
            if !(id_matches && ordered && unique && display_matches && rule_matches) {
                return Err(DocumentError::CatalogRow {
                    mutant: one.id.clone(),
                });
            }
            previous = Some(one.index);
        }
        for rejection in &self.rejections {
            let unique = indexes.insert(rejection.index)
                && identities.insert(rejection.id.as_str())
                && display_ids.insert(rejection.display_id.as_str());
            let valid = crate::id::is_id(&rejection.id)
                && crate::id::display_id_of(&rejection.id)
                    .is_ok_and(|display_id| display_id == rejection.display_id)
                && registry.lookup(&rejection.rule).is_some();
            if !(unique && valid) {
                return Err(DocumentError::CatalogRow {
                    mutant: rejection.id.clone(),
                });
            }
        }
        let total = self
            .mutants
            .len()
            .checked_add(self.rejections.len())
            .ok_or(DocumentError::CatalogTooLarge)?;
        let total = u32::try_from(total).map_err(|_too_wide| DocumentError::CatalogTooLarge)?;
        let dense = indexes.iter().copied().eq(0..total);
        if shard.is_none_or(|shard| shard.of == 1) && !dense {
            return Err(DocumentError::CatalogRow {
                mutant: "<catalog>".to_owned(),
            });
        }
        Ok(())
    }

    fn validate_nonverdict_findings(&self) -> Result<(), DocumentError> {
        let mut earned: Vec<FindingDocument> = self
            .expectations
            .iter()
            .filter_map(|claim| claim.finding().transpose())
            .collect::<Result<_, _>>()?;
        let mut stated: Vec<FindingDocument> = self
            .findings
            .iter()
            .filter(|finding| finding.kind.restates_a_claim())
            .cloned()
            .collect();
        earned.sort_by(finding_order);
        stated.sort_by(finding_order);
        if let Some(kind) = earned
            .iter()
            .zip(&stated)
            .find(|(earned, stated)| earned != stated)
            .map(|(earned, _)| earned.kind)
            .or_else(|| {
                earned
                    .get(stated.len())
                    .or_else(|| stated.get(earned.len()))
                    .map(|finding| finding.kind)
            })
        {
            return Err(DocumentError::FindingFact { kind });
        }
        for finding in &self.findings {
            let valid_mutant = match finding.kind {
                FindingKind::UnmatchedSkip => finding.mutant.is_none(),
                FindingKind::StaleExpectation
                | FindingKind::UnmatchedExpectation
                | FindingKind::SurvivingMutant
                | FindingKind::InconclusiveMutant
                | FindingKind::StepLimitReachedMutant
                | FindingKind::WaitedMutant
                | FindingKind::ErroredMutant
                | FindingKind::NotRunMutant
                | FindingKind::UnreachedMutant
                | FindingKind::DischargedMutant => true,
            };
            if !valid_mutant || finding.detail.trim().is_empty() {
                return Err(DocumentError::FindingFact { kind: finding.kind });
            }
        }
        Ok(())
    }
}

impl ExpectationDocument {
    /// The finding this claim's standing earns, when it earns one.
    ///
    /// # Errors
    /// Refuses a claim whose fields do not carry the standing it states.
    pub fn finding(&self) -> Result<Option<FindingDocument>, DocumentError> {
        match (self.standing.as_str(), self.actual, &self.why) {
            ("met", None, _) | ("unjudged", None, None) | ("inapplicable", None, Some(_)) => {
                Ok(None)
            }
            ("stale", Some(actual), None) => Ok(Some(FindingDocument {
                kind: FindingKind::StaleExpectation,
                mutant: self.mutant.clone(),
                detail: stale_detail(&self.id, self.outcome, actual, &self.reason),
            })),
            ("unmatched", None, Some(why)) => Ok(Some(FindingDocument {
                kind: FindingKind::UnmatchedExpectation,
                mutant: None,
                detail: unmatched_detail(&self.id, why),
            })),
            _ => Err(DocumentError::Claim {
                id: self.id.clone(),
                standing: self.standing.clone(),
            }),
        }
    }
}

/// The order a report writes its findings in: by kind, then mutant, then detail.
fn finding_order(a: &FindingDocument, b: &FindingDocument) -> std::cmp::Ordering {
    a.kind
        .name()
        .cmp(b.kind.name())
        .then_with(|| a.mutant.cmp(&b.mutant))
        .then_with(|| a.detail.cmp(&b.detail))
}

impl FindingKind {
    /// Whether the finding restates an expectation's standing rather than a mutant's verdict.
    const fn restates_a_claim(self) -> bool {
        matches!(self, Self::StaleExpectation | Self::UnmatchedExpectation)
    }

    const fn is_verdict(self) -> bool {
        matches!(
            self,
            Self::SurvivingMutant
                | Self::InconclusiveMutant
                | Self::StepLimitReachedMutant
                | Self::WaitedMutant
                | Self::ErroredMutant
                | Self::NotRunMutant
                | Self::UnreachedMutant
                | Self::DischargedMutant
        )
    }
}

const fn verdict_finding(one: &RunMutantDocument, interrupted: bool) -> Option<FindingKind> {
    match one.outcome {
        Outcome::Killed => None,
        Outcome::Survived if one.expected => None,
        Outcome::Survived => Some(FindingKind::SurvivingMutant),
        Outcome::StepLimitReached => Some(FindingKind::StepLimitReachedMutant),
        Outcome::Waited => Some(FindingKind::WaitedMutant),
        Outcome::Inconclusive => Some(FindingKind::InconclusiveMutant),
        Outcome::Errored => Some(FindingKind::ErroredMutant),
        Outcome::NotRun => match one.not_run_reason {
            Some(NotRunReason::Unreached) => Some(FindingKind::UnreachedMutant),
            Some(NotRunReason::Discharged) => Some(FindingKind::DischargedMutant),
            Some(
                NotRunReason::Interrupted | NotRunReason::Unselected | NotRunReason::StoppedEarly,
            ) => None,
            None if interrupted => None,
            None => Some(FindingKind::NotRunMutant),
        },
    }
}

fn accounting_from_document(document: &RunDocument) -> Result<Accounting, DocumentError> {
    let mut accounting = Accounting {
        cataloged: document_count(document.mutants.len())?,
        refused: document_count(document.rejections.len())?.into(),
        skipped: document
            .skips
            .iter()
            .try_fold(0u32, |total, skip| {
                total
                    .checked_add(skip.count)
                    .ok_or(DocumentError::CatalogTooLarge)
            })?
            .into(),
        ..Accounting::default()
    };
    for one in &document.mutants {
        let outcome = match one.outcome {
            Outcome::Killed => &mut accounting.killed,
            Outcome::Survived => &mut accounting.survived,
            Outcome::StepLimitReached => &mut accounting.step_limit_reached,
            Outcome::Waited => &mut accounting.waited,
            Outcome::Inconclusive => &mut accounting.inconclusive,
            Outcome::Errored => &mut accounting.errored,
            Outcome::NotRun => &mut accounting.not_run,
        };
        outcome.raise()?;
        if one.not_run_reason == Some(NotRunReason::Unreached) {
            accounting.unreached.raise()?;
        }
        if one.not_run_reason == Some(NotRunReason::Discharged) {
            accounting.discharged.raise()?;
        }
        if one.expected {
            accounting.expected.raise()?;
        }
    }
    accounting.executed = Beside::new(
        accounting
            .cataloged
            .checked_sub(accounting.not_run.count())
            .ok_or(CountOverflowError)?,
    );
    Ok(accounting)
}

fn document_count(count: usize) -> Result<u32, DocumentError> {
    u32::try_from(count).map_err(|_outside_range| DocumentError::CatalogTooLarge)
}

fn accounting_difference(actual: &Accounting, expected: &Accounting) -> Option<&'static str> {
    [
        ("cataloged", actual.cataloged, expected.cataloged),
        ("refused", actual.refused.count(), expected.refused.count()),
        ("skipped", actual.skipped.count(), expected.skipped.count()),
        (
            "executed",
            actual.executed.count(),
            expected.executed.count(),
        ),
        ("killed", actual.killed.count(), expected.killed.count()),
        (
            "survived",
            actual.survived.count(),
            expected.survived.count(),
        ),
        (
            "step_limit_reached",
            actual.step_limit_reached.count(),
            expected.step_limit_reached.count(),
        ),
        ("waited", actual.waited.count(), expected.waited.count()),
        (
            "inconclusive",
            actual.inconclusive.count(),
            expected.inconclusive.count(),
        ),
        ("errored", actual.errored.count(), expected.errored.count()),
        ("not_run", actual.not_run.count(), expected.not_run.count()),
        (
            "unreached",
            actual.unreached.count(),
            expected.unreached.count(),
        ),
        (
            "discharged",
            actual.discharged.count(),
            expected.discharged.count(),
        ),
        (
            "expected",
            actual.expected.count(),
            expected.expected.count(),
        ),
    ]
    .into_iter()
    .find_map(|(name, actual, expected)| (actual != expected).then_some(name))
}

/// What the document needs that the session and the run do not carry.
#[derive(Debug, Clone, Copy)]
pub struct Meta<'a> {
    /// The run's identity.
    pub id: &'a str,
    /// When the run started.
    pub started_at: Timestamp,
    /// When it finished.
    pub finished_at: Timestamp,
}

/// Every test target the run built, as documents.
fn target_documents(session: &Session) -> Vec<TargetDocument> {
    session
        .targets()
        .iter()
        .map(|target| TargetDocument {
            id: target.id.clone(),
            kind: target.kind.name().to_owned(),
            harness: target.harness,
            tests: session.tests_of(&target.id),
            limitations: target.limitations.clone(),
        })
        .collect()
}

/// The run as one document.
///
/// # Errors
/// Returns an engine error when established-test accounting or any exact duration projection cannot be represented by the report schema.
pub fn document(
    session: &Session,
    run: &Run,
    selection: SelectionDocument,
    meta: &Meta<'_>,
) -> Result<RunDocument, crate::EngineError> {
    let tally = run.tally()?;
    Ok(RunDocument {
        document_type: DOCUMENT_TYPE.to_owned(),
        schema_version: SCHEMA_VERSION,
        tool_version: crate::VERSION.to_owned(),
        run: RunMeta {
            id: meta.id.to_owned(),
            started_at: meta.started_at.to_string(),
            finished_at: meta.finished_at.to_string(),
            duration_ms: millis(run.duration)?,
            interrupted: run.interrupted,
            exit_code: run.exit_code(),
            shard: run.shard.map(|shard| shard.to_string()),
            jobs: JobsDocument {
                asked: run.width.asked.name(),
                used: u32::try_from(run.width.used)
                    .map_err(|_too_wide| crate::workspace::SessionError::RunCountOverflow)?,
            },
        },
        workspace: crate::report::catalog::workspace_document(session)?,
        selection,
        targets: target_documents(session),
        established_tests: session.established_tests()?,
        accounting: Accounting {
            cataloged: tally.cataloged,
            refused: tally.refused.into(),
            skipped: tally.skipped.into(),
            executed: tally.executed.into(),
            killed: tally.killed.into(),
            survived: tally.survived.into(),
            step_limit_reached: tally.step_limit_reached.into(),
            waited: tally.waited.into(),
            inconclusive: tally.inconclusive.into(),
            errored: tally.errored.into(),
            not_run: tally.not_run.into(),
            expected: tally.expected.into(),
            unreached: tally.unreached.into(),
            discharged: tally.discharged.into(),
        },
        score: run.score()?.map(|score| ScoreDocument {
            detected: score.detected,
            decided: score.decided,
            value: score.value,
        }),
        mutants: run
            .judged
            .iter()
            .map(|one| {
                let catalog = session
                    .catalog()
                    .by_index(one.index)
                    .map(|mutant| crate::report::catalog::mutant_document(session, mutant))
                    .transpose()?;
                mutant(one, catalog)
            })
            .collect::<Result<Vec<_>, _>>()?,
        rejections: crate::report::catalog::rejection_documents(session),
        skips: crate::report::catalog::skip_documents(session),
        expectations: run.expectations.iter().map(expectation_document).collect(),
        findings: run.findings().iter().map(finding).collect(),
        facts: session.facts().recorded(),
    })
}

/// The document a verified claim is written as, which is the shape [`ExpectationDocument::finding`] reads back.
fn expectation_document(verified: &crate::run::Verified) -> ExpectationDocument {
    ExpectationDocument {
        id: verified.id.clone(),
        locator: verified.locator.as_ref().map(LocatorDocument::from),
        reason: verified.reason.clone(),
        outcome: verified.outcome,
        mutant: verified.mutant.clone(),
        covered: (verified.covered > 1).then_some(verified.covered),
        standing: standing_name(&verified.standing).to_owned(),
        actual: match &verified.standing {
            Standing::Stale { actual } => Some(*actual),
            Standing::Met
            | Standing::Moved { .. }
            | Standing::Unmatched { .. }
            | Standing::Unjudged
            | Standing::Inapplicable { .. } => None,
        },
        why: match &verified.standing {
            Standing::Unmatched { why } => Some(why.clone()),
            Standing::Inapplicable { because } => Some(because.said()),
            Standing::Moved { from, to } => {
                Some(format!("the mutation moved from line {from} to line {to}"))
            }
            Standing::Met | Standing::Stale { .. } | Standing::Unjudged => None,
        },
        holds: (verified.under != crate::run::Where::default()).then(|| WhereDocument {
            cfg: verified.under.cfg.as_ref().map(ToString::to_string),
            env: verified.under.env.clone(),
        }),
    }
}

const fn standing_name(standing: &Standing) -> &'static str {
    match standing {
        Standing::Met | Standing::Moved { .. } => "met",
        Standing::Stale { .. } => "stale",
        Standing::Unmatched { .. } => "unmatched",
        Standing::Unjudged => "unjudged",
        Standing::Inapplicable { .. } => "inapplicable",
    }
}

fn finding(finding: &Finding) -> FindingDocument {
    FindingDocument {
        kind: finding.kind,
        mutant: finding.mutant.clone(),
        detail: finding.detail.clone(),
    }
}

fn mutant(
    one: &crate::run::Judged,
    catalog: Option<MutantDocument>,
) -> Result<RunMutantDocument, crate::EngineError> {
    let catalog = catalog.unwrap_or_else(|| MutantDocument {
        index: one.index,
        id: one.id.clone(),
        display_id: one.display_id.clone(),
        path: String::new(),
        package: String::new(),
        family: String::new(),
        rule: String::new(),
        item: String::new(),
        rule_version: 0,
        line: 0,
        column: 0,
        start_byte: 0,
        end_byte: 0,
        source_digest: String::new(),
        original: String::new(),
        replacement: String::new(),
        branch: None,
    });
    Ok(RunMutantDocument {
        index: catalog.index,
        id: catalog.id,
        display_id: catalog.display_id,
        path: catalog.path,
        package: catalog.package,
        family: catalog.family,
        rule: catalog.rule,
        item: catalog.item,
        rule_version: catalog.rule_version,
        line: catalog.line,
        column: catalog.column,
        start_byte: catalog.start_byte,
        end_byte: catalog.end_byte,
        source_digest: catalog.source_digest,
        original: catalog.original,
        replacement: catalog.replacement,
        outcome: one.outcome,
        step_notice: one.step_notice.clone(),
        target: one.target.clone(),
        exit_code: one.exit_code,
        duration_ms: millis(one.duration)?,
        tests_run: one.tests_run,
        killed_by: match one.outcome {
            Outcome::Killed => one.failed_tests.clone(),
            Outcome::NotRun
            | Outcome::Survived
            | Outcome::StepLimitReached
            | Outcome::Waited
            | Outcome::Inconclusive
            | Outcome::Errored => Vec::new(),
        },
        signal: one.signal,
        retried: one.retried,
        lingered: one.lingered,
        not_run_reason: one.not_run_reason,
        route: one.route.clone(),
        identical: one.identical,
        expected: one.expected,
        unreached: one.not_run_reason == Some(NotRunReason::Unreached),
        source_run_id: one.source_run_id.clone(),
    })
}

/// One test target a run built, and what it is beyond its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetDocument {
    /// `package/kind/name`.
    pub id: String,
    /// What kind of target it is.
    pub kind: String,
    /// Whether it is built with the libtest harness, which decides how its silence is read.
    pub harness: bool,
    /// How many tests its baseline ran, which is what asking the whole of it about one mutation costs.
    pub tests: u32,
    /// What a run could not establish about it, each named.
    pub limitations: Vec<String>,
}

/// One route, as a document, with the targets an execution of it actually ran.
#[must_use]
pub fn route_document(route: &crate::session::Route, executed: Vec<String>) -> RouteDocument {
    RouteDocument {
        granularity: route.granularity().name().to_owned(),
        fallback: route.fallback().map(|one| one.name().to_owned()),
        reaching: route
            .reaching()
            .into_iter()
            .map(ToOwned::to_owned)
            .collect(),
        discharged: route
            .discharged()
            .iter()
            .map(|one| DischargeDocument {
                target: one.target.clone(),
                proof: one.proof.name().to_owned(),
            })
            .collect(),
        executed,
        tests: route.tests(),
    }
}

fn millis(value: std::time::Duration) -> Result<u64, crate::workspace::SessionError> {
    u64::try_from(value.as_millis()).map_err(|_overflow| {
        crate::workspace::SessionError::DurationMillisOverflow { duration: value }
    })
}

/// Which targets could have noticed one mutation, and what became of the ones that ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteDocument {
    /// `all`, `test`, `block`, `discharged`, or `unreached`.
    pub granularity: String,
    /// Why the route is wider than the measurement alone would make it.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub fallback: Option<String>,
    /// Every target that could have noticed the mutation.
    pub reaching: Vec<String>,
    /// Every target a proof removed, with the proof that removed it.
    pub discharged: Vec<DischargeDocument>,
    /// Every target that ran, in the order the run asked them.
    pub executed: Vec<String>,
    /// For each target the measurement narrowed to some of its tests, exactly those tests.
    /// A target absent from this ran every test it has.
    pub tests: BTreeMap<String, Vec<String>>,
}

/// One target a proof removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DischargeDocument {
    /// The target.
    pub target: String,
    /// The proof that removed it.
    pub proof: String,
}

/// Why the parts of a catalog could not be put back together.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MergeError {
    /// Nothing was given to combine.
    #[error("a merge of no reports is not a report")]
    Nothing,
    /// Two of the reports are about different trees, so their parts were never parts of one whole.
    #[error(
        "{first} and {other} are about different catalogs, and the parts of one catalog are what a merge is of"
    )]
    Disagree {
        /// The catalog the first report is about.
        first: String,
        /// The one that differs.
        other: String,
    },
    /// Two parts were measured for targets that say different things of themselves, so a claim judged in one is not judged the same way in the other (ADR 0042).
    #[error(
        "one part was measured where the target is {first:?} and another where it is {other:?}, and a claim's where is judged against one target"
    )]
    TargetsDisagree {
        /// What the first part's target says of itself.
        first: String,
        /// What the part that differs says.
        other: String,
    },
    /// One mutant appears in more than one part, so the parts overlap and the counts would say more happened than did.
    #[error(
        "{mutant} is in more than one of these reports, so they are not the parts of one whole"
    )]
    Overlapping {
        /// The mutant two reports both claim.
        mutant: String,
    },
    /// The reports are not every part of one catalog, each once.
    #[error(transparent)]
    Parts(#[from] crate::run::PartsError),
    /// A part names a shard that is not one.
    #[error("a report part names a shard that is not one: {error}")]
    Shard {
        /// Why the shard is not one.
        #[source]
        error: crate::run::ShardError,
    },
    /// One part contradicts itself, so combining it would preserve a lie.
    #[error("a report part contradicts itself: {error}")]
    InvalidPart {
        /// The contradiction.
        #[source]
        error: DocumentError,
    },
    /// The merged catalog contains more rows than its durable counters can represent.
    #[error("the merged catalog exceeds its durable counters")]
    CatalogTooLarge,
    /// Exact merged accounting exceeded a durable counter or contradicted a subset relation.
    #[error(transparent)]
    Count(#[from] CountOverflowError),
}

/// Every claim the parts state, one each, answered as the whole run answers it.
///
/// A claim is the same claim in every part, and each part judged the mutations it held in catalog order and named the first that contradicted the claim, or the first it held when none did.
/// The whole run names the first in catalog order across all of them, so the merged claim is the part's answer that names the earliest contradicting mutation, or failing any, the earliest met one, and is unjudged only where no part decided any of them.
fn claims_of(parts: &[RunDocument], rows: &[RunMutantDocument]) -> Vec<ExpectationDocument> {
    let at = |one: &ExpectationDocument| {
        let rank = match one.standing.as_str() {
            "met" => 1,
            "unjudged" => 2,
            _ => 0,
        };
        let position = match &one.mutant {
            Some(mutant) => match rows.iter().find(|row| row.id == *mutant) {
                Some(row) => row.index,
                None => u32::MAX,
            },
            None if rank == 0 => 0,
            None => u32::MAX,
        };
        (rank, position)
    };
    let mut claims: Vec<ExpectationDocument> = Vec::new();
    for one in parts.iter().flat_map(|part| part.expectations.iter()) {
        let same = |held: &ExpectationDocument| {
            held.id == one.id
                && held.locator == one.locator
                && held.reason == one.reason
                && held.outcome == one.outcome
                && held.covered == one.covered
                && held.holds == one.holds
        };
        match claims.iter_mut().find(|held| same(held)) {
            Some(held) if at(one) < at(held) => *held = one.clone(),
            Some(_) => {}
            None => claims.push(one.clone()),
        }
    }
    claims
}

/// The report the whole of a catalog would have written, from the reports of its parts.
///
/// # Errors
/// See [`MergeError`].
pub fn merge(parts: &[RunDocument]) -> Result<RunDocument, MergeError> {
    let whole = crate::run::Shard { index: 1, of: 1 };
    let shards = parts
        .iter()
        .map(|part| match &part.run.shard {
            Some(text) => crate::run::Shard::parse(text)
                .map(|shard| (shard, part))
                .map_err(|error| MergeError::Shard { error }),
            None => Ok((whole, part)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let ordered: Vec<RunDocument> = crate::run::Shard::every_part(shards)?
        .into_iter()
        .cloned()
        .collect();
    let parts = ordered.as_slice();
    let first = parts.first().ok_or(MergeError::Nothing)?;
    for part in parts {
        part.validate()
            .map_err(|error| MergeError::InvalidPart { error })?;
        if part.facts != first.facts {
            return Err(MergeError::TargetsDisagree {
                first: first.facts.join(" "),
                other: part.facts.join(" "),
            });
        }
        if part.workspace.catalog_digest != first.workspace.catalog_digest {
            return Err(MergeError::Disagree {
                first: first.workspace.catalog_digest.clone(),
                other: part.workspace.catalog_digest.clone(),
            });
        }
    }
    let mut mutants: BTreeMap<u32, RunMutantDocument> = BTreeMap::new();
    for part in parts {
        for one in &part.mutants {
            if mutants.insert(one.index, one.clone()).is_some() {
                return Err(MergeError::Overlapping {
                    mutant: one.id.clone(),
                });
            }
        }
    }
    let mutants: Vec<RunMutantDocument> = mutants.into_values().collect();
    let accounting = accounting_of(&mutants, first)?;
    let mut merged = first.clone();
    merged.run = RunMeta {
        duration_ms: parts.iter().map(|part| part.run.duration_ms).sum(),
        interrupted: parts.iter().any(|part| part.run.interrupted),
        shard: None,
        ..first.run.clone()
    };
    merged.score = score_of(&accounting)?;
    merged.accounting = accounting;
    merged.mutants = mutants;
    merged.expectations = claims_of(parts, &merged.mutants);
    let restated: Vec<FindingDocument> = merged
        .expectations
        .iter()
        .filter_map(|claim| claim.finding().transpose())
        .collect::<Result<_, _>>()
        .map_err(|error| MergeError::InvalidPart { error })?;
    merged.findings = parts
        .iter()
        .flat_map(|part| part.findings.iter())
        .filter(|finding| !finding.kind.restates_a_claim())
        .cloned()
        .chain(restated)
        .collect();
    merged.findings.sort_by(finding_order);
    merged.findings.dedup();
    merged.run.exit_code = exit_code_of(&merged);
    merged
        .validate()
        .map_err(|error| MergeError::InvalidPart { error })?;
    Ok(merged)
}

/// The columns the merged records add up to.
/// What no part executed — refusals and skips — is a fact about the catalog rather than about a part, so it is taken from one of them rather than summed.
fn accounting_of(
    mutants: &[RunMutantDocument],
    first: &RunDocument,
) -> Result<Accounting, MergeError> {
    let mut counted = Accounting {
        cataloged: u32::try_from(mutants.len())
            .map_err(|_outside_range| MergeError::CatalogTooLarge)?,
        refused: first.accounting.refused,
        skipped: first.accounting.skipped,
        ..Accounting::default()
    };
    for one in mutants {
        let slot = match one.outcome {
            Outcome::Killed => &mut counted.killed,
            Outcome::Survived => &mut counted.survived,
            Outcome::StepLimitReached => &mut counted.step_limit_reached,
            Outcome::Waited => &mut counted.waited,
            Outcome::Inconclusive => &mut counted.inconclusive,
            Outcome::NotRun => &mut counted.not_run,
            Outcome::Errored => &mut counted.errored,
        };
        slot.raise()?;
        if one.unreached {
            counted.unreached.raise()?;
        }
        if one.not_run_reason == Some(NotRunReason::Discharged) {
            counted.discharged.raise()?;
        }
        if one.expected {
            counted.expected.raise()?;
        }
    }
    counted.executed = Beside::new(
        counted
            .cataloged
            .checked_sub(counted.not_run.count())
            .ok_or(CountOverflowError)?,
    );
    Ok(counted)
}

fn score_of(accounting: &Accounting) -> Result<Option<ScoreDocument>, CountOverflowError> {
    let detected = accounting.killed.count();
    let decided = detected
        .checked_add(accounting.survived.count())
        .ok_or(CountOverflowError)?;
    Ok((decided > 0).then(|| ScoreDocument {
        detected,
        decided,
        value: f64::from(detected) / f64::from(decided),
    }))
}

/// The exit code the whole earns, which is the code the whole would have earned rather than the worst of its parts.
fn exit_code_of(merged: &RunDocument) -> u8 {
    crate::run::Exit::of(
        merged.run.interrupted,
        merged.findings.iter().map(|finding| finding.kind),
    )
    .code()
}

#[cfg(test)]
mod tests {
    use crate::outcome::Outcome;
    use crate::run::{Standing, Verified};

    use super::expectation_document;

    fn every_standing() -> Vec<Standing> {
        let every = vec![
            Standing::Met,
            Standing::Moved { from: 9, to: 11 },
            Standing::Stale {
                actual: Outcome::Killed,
            },
            Standing::Unmatched {
                why: "the identity names nothing".to_owned(),
            },
            Standing::Unjudged,
            Standing::Inapplicable {
                because: crate::run::Unheld::NotCompiled,
            },
        ];
        for standing in &every {
            match standing {
                Standing::Met
                | Standing::Moved { .. }
                | Standing::Stale { .. }
                | Standing::Unmatched { .. }
                | Standing::Unjudged
                | Standing::Inapplicable { .. } => {}
            }
        }
        every
    }

    #[test]
    fn every_standing_the_writer_can_write_reads_back() {
        for standing in every_standing() {
            let verified = Verified {
                id: "a claim".to_owned(),
                locator: None,
                reason: "a reason".to_owned(),
                outcome: Outcome::Survived,
                mutant: Some("a mutant".to_owned()),
                covered: 1,
                standing: standing.clone(),
                under: crate::run::Where::default(),
            };
            let written = expectation_document(&verified);
            assert!(
                written.finding().is_ok(),
                "{standing:?} is written as {written:?}, which its reader refuses"
            );
        }
    }
}
