// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The run report: one completed run of every mutant, as a document and as lines a person reads.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use jiff::Timestamp;
use rust_mutants::session::Session;
use serde::{Deserialize, Serialize};

use crate::report::{
    MutantDocument, RejectionDocument, SelectionDocument, SkipDocument, WorkspaceDocument,
};
use crate::run::{Finding, Run, Standing};

/// The name of the shape, so a reader can tell versions apart.
pub const DOCUMENT_TYPE: &str = "rust-mutants/run-report";

/// The version of that shape.
pub const SCHEMA_VERSION: u32 = 1;

/// The file one run writes under its own directory.
pub const FILE_NAME: &str = "run-report-v1.json";

/// The pointer file that names the newest run.
pub const LATEST_FILE_NAME: &str = "latest.json";

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
    pub score: Option<ScoreDocument>,
    /// One record per cataloged mutant, in catalog order.
    pub mutants: Vec<RunMutantDocument>,
    /// Every candidate the compiler refused.
    pub rejections: Vec<RejectionDocument>,
    /// Every place discovery passed over.
    pub skips: Vec<SkipDocument>,
    /// The claims a reviewer declared, as the run left them.
    pub expectations: Vec<ExpectationDocument>,
    /// Everything that stops the run from being clean.
    pub findings: Vec<FindingDocument>,
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
    pub shard: Option<String>,
}

/// What a run counted. Every mutant is in exactly one of the outcome columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accounting {
    /// How many mutants the compiler accepted.
    pub cataloged: u32,
    /// How many candidates the compiler refused.
    pub refused: u32,
    /// How many places discovery passed over.
    pub skipped: u32,
    /// How many mutants an execution reached a verdict on.
    pub executed: u32,
    /// How many a test failed on.
    pub killed: u32,
    /// How many every test passed on.
    pub survived: u32,
    /// How many exceeded the budget twice.
    pub timed_out: u32,
    /// How many the run could not decide.
    pub inconclusive: u32,
    /// How many the harness itself failed on.
    pub errored: u32,
    /// How many never ran.
    pub not_run: u32,
    /// How many survivors a reviewer had declared, and the run confirmed.
    pub expected: u32,
}

/// The share of decided mutants the tests noticed.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreDocument {
    /// Killed plus confirmed timeouts.
    pub detected: u32,
    /// Detected plus survived.
    pub decided: u32,
    /// `detected / decided`.
    pub value: f64,
}

/// One cataloged mutant and what the run established about it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// The rule's version, which enters the identity.
    pub rule_version: u32,
    /// The 1-based line of the edit.
    pub line: u32,
    /// The 1-based byte column of the edit.
    pub column: u32,
    /// The bytes the edit replaces.
    pub original: String,
    /// What they become.
    pub replacement: String,
    /// What the execution says.
    pub outcome: String,
    /// The target that ran, empty when none did.
    pub target: String,
    /// The exit status of the last execution.
    pub exit_code: i32,
    /// How long every execution of this mutant took together.
    pub duration_ms: u64,
    /// How many tests ran, when the harness said.
    pub tests_run: Option<u32>,
    /// Whether a first timeout was retried serially before the outcome was believed.
    pub retried: bool,
    /// Whether a reviewer declared this outcome in advance and the run confirmed the claim.
    pub expected: bool,
    /// The run that established this, when it was not this one.
    pub source_run_id: Option<String>,
}

/// One declared expectation, as the run left it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectationDocument {
    /// The identity or prefix the file wrote.
    pub id: String,
    /// Why the reviewer claims the outcome.
    pub reason: String,
    /// The outcome claimed.
    pub outcome: String,
    /// The mutant it resolved to, when it resolved.
    pub mutant: Option<String>,
    /// Whether the claim held: `met`, `stale`, or `unmatched`.
    pub standing: String,
    /// What the run established instead, when the claim was contradicted.
    pub actual: Option<String>,
    /// Why the identity resolved to nothing, when it did not resolve.
    pub why: Option<String>,
}

/// One thing that stops a run from being clean.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindingDocument {
    /// What kind of hole it is.
    pub kind: String,
    /// The mutant it is about, when it is about one.
    pub mutant: Option<String>,
    /// One sentence a reader can act on.
    pub detail: String,
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

/// The run as one document.
#[must_use]
pub fn document(
    session: &Session,
    run: &Run,
    selection: SelectionDocument,
    meta: &Meta<'_>,
) -> RunDocument {
    let tally = run.tally();
    RunDocument {
        document_type: DOCUMENT_TYPE.to_owned(),
        schema_version: SCHEMA_VERSION,
        tool_version: rust_mutants::VERSION.to_owned(),
        run: RunMeta {
            id: meta.id.to_owned(),
            started_at: meta.started_at.to_string(),
            finished_at: meta.finished_at.to_string(),
            duration_ms: millis(run.duration),
            interrupted: run.interrupted,
            exit_code: run.exit_code(),
            shard: run.shard.map(|shard| shard.to_string()),
        },
        workspace: crate::report::workspace_document(session),
        selection,
        accounting: Accounting {
            cataloged: tally.cataloged,
            refused: tally.refused,
            skipped: tally.skipped,
            executed: tally.executed,
            killed: tally.killed,
            survived: tally.survived,
            timed_out: tally.timed_out,
            inconclusive: tally.inconclusive,
            errored: tally.errored,
            not_run: tally.not_run,
            expected: tally.expected,
        },
        score: run.score().map(|score| ScoreDocument {
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
                    .map(|mutant| crate::report::mutant_document(session, mutant));
                mutant(one, catalog)
            })
            .collect(),
        rejections: crate::report::rejection_documents(session),
        skips: crate::report::skip_documents(session),
        expectations: run
            .expectations
            .iter()
            .map(|verified| ExpectationDocument {
                id: verified.id.clone(),
                reason: verified.reason.clone(),
                outcome: verified.outcome.name().to_owned(),
                mutant: verified.mutant.clone(),
                standing: standing_name(&verified.standing).to_owned(),
                actual: match &verified.standing {
                    Standing::Stale { actual } => Some(actual.name().to_owned()),
                    Standing::Met | Standing::Unmatched { .. } => None,
                },
                why: match &verified.standing {
                    Standing::Unmatched { why } => Some(why.clone()),
                    Standing::Met | Standing::Stale { .. } => None,
                },
            })
            .collect(),
        findings: run.findings().iter().map(finding).collect(),
    }
}

const fn standing_name(standing: &Standing) -> &'static str {
    match standing {
        Standing::Met => "met",
        Standing::Stale { .. } => "stale",
        Standing::Unmatched { .. } => "unmatched",
    }
}

fn finding(finding: &Finding) -> FindingDocument {
    FindingDocument {
        kind: finding.kind.name().to_owned(),
        mutant: finding.mutant.clone(),
        detail: finding.detail.clone(),
    }
}

fn mutant(one: &crate::run::Judged, catalog: Option<MutantDocument>) -> RunMutantDocument {
    let catalog = catalog.unwrap_or_else(|| MutantDocument {
        index: one.index,
        id: one.id.clone(),
        display_id: one.display_id.clone(),
        path: String::new(),
        package: String::new(),
        family: String::new(),
        rule: String::new(),
        rule_version: 0,
        line: 0,
        column: 0,
        start_byte: 0,
        end_byte: 0,
        original: String::new(),
        replacement: String::new(),
    });
    RunMutantDocument {
        index: catalog.index,
        id: catalog.id,
        display_id: catalog.display_id,
        path: catalog.path,
        package: catalog.package,
        family: catalog.family,
        rule: catalog.rule,
        rule_version: catalog.rule_version,
        line: catalog.line,
        column: catalog.column,
        original: catalog.original,
        replacement: catalog.replacement,
        outcome: one.outcome.name().to_owned(),
        target: one.target.clone(),
        exit_code: one.exit_code,
        duration_ms: millis(one.duration),
        tests_run: one.tests_run,
        retried: one.retried,
        expected: one.expected,
        source_run_id: one.source_run_id.clone(),
    }
}

fn millis(value: std::time::Duration) -> u64 {
    u64::try_from(value.as_millis()).unwrap_or(u64::MAX)
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
    /// One mutant appears in more than one part, so the parts overlap and the counts would say more happened than did.
    #[error(
        "{mutant} is in more than one of these reports, so they are not the parts of one whole"
    )]
    Overlapping {
        /// The mutant two reports both claim.
        mutant: String,
    },
}

/// The report the whole of a catalog would have written, from the reports of its parts.
///
/// The parts are checked for being parts: they must be about one catalog, and
/// no mutant may appear in two of them. A merge that let them overlap would
/// count one execution twice and report a score no run ever established.
///
/// # Errors
/// See [`MergeError`].
pub fn merge(parts: &[RunDocument]) -> Result<RunDocument, MergeError> {
    let first = parts.first().ok_or(MergeError::Nothing)?;
    for part in parts {
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
    let accounting = accounting_of(&mutants, first);
    let mut merged = first.clone();
    merged.run = RunMeta {
        duration_ms: parts.iter().map(|part| part.run.duration_ms).sum(),
        interrupted: parts.iter().any(|part| part.run.interrupted),
        shard: None,
        ..first.run.clone()
    };
    merged.score = score_of(&accounting);
    merged.accounting = accounting;
    merged.mutants = mutants;
    merged.expectations = parts
        .iter()
        .flat_map(|part| part.expectations.iter().cloned())
        .collect();
    merged.findings = parts
        .iter()
        .flat_map(|part| part.findings.iter().cloned())
        .collect();
    merged.findings.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.mutant.cmp(&b.mutant))
            .then_with(|| a.detail.cmp(&b.detail))
    });
    merged.findings.dedup();
    merged.run.exit_code = exit_code_of(&merged);
    Ok(merged)
}

/// The columns the merged records add up to. What no part executed — refusals and skips — is a fact about the catalog rather than about a part, so it is taken from one of them rather than summed.
fn accounting_of(mutants: &[RunMutantDocument], first: &RunDocument) -> Accounting {
    let mut counted = Accounting {
        cataloged: count(mutants.len()),
        refused: first.accounting.refused,
        skipped: first.accounting.skipped,
        ..Accounting::default()
    };
    for one in mutants {
        let slot = match one.outcome.as_str() {
            "killed" => &mut counted.killed,
            "survived" => &mut counted.survived,
            "timed_out" => &mut counted.timed_out,
            "inconclusive" => &mut counted.inconclusive,
            "not_run" => &mut counted.not_run,
            _ => &mut counted.errored,
        };
        *slot = slot.saturating_add(1);
        if one.expected {
            counted.expected = counted.expected.saturating_add(1);
        }
    }
    counted.executed = counted.cataloged.saturating_sub(counted.not_run);
    counted
}

fn score_of(accounting: &Accounting) -> Option<ScoreDocument> {
    let detected = accounting.killed.saturating_add(accounting.timed_out);
    let decided = detected.saturating_add(accounting.survived);
    (decided > 0).then(|| ScoreDocument {
        detected,
        decided,
        value: f64::from(detected) / f64::from(decided),
    })
}

/// The exit code the whole earns, which is the code the whole would have earned rather than the worst of its parts.
const fn exit_code_of(merged: &RunDocument) -> u8 {
    if merged.run.interrupted {
        return crate::run::EXIT_INTERRUPTED;
    }
    let broken = merged.accounting.errored > 0 || merged.accounting.not_run > 0;
    if broken {
        return crate::EXIT_USAGE;
    }
    if merged.findings.is_empty() {
        crate::run::EXIT_DETECTED
    } else {
        crate::run::EXIT_UNDETECTED
    }
}

use crate::run::count;

/// The run as lines a person reads: the tally, the score, and every finding.
#[must_use]
pub fn lines(document: &RunDocument) -> String {
    let a = &document.accounting;
    let mut text = String::new();
    let written = write!(
        text,
        "run       {}\nworkspace {}\ncatalog   {}\n\n\
         MUTANTS   cataloged={} refused={} skipped={} executed={}\n\
         OUTCOMES  killed={} survived={} timed_out={} inconclusive={} errored={} not_run={} \
         expected={}\n",
        document.run.id,
        document.workspace.workspace_digest,
        document.workspace.catalog_digest,
        a.cataloged,
        a.refused,
        a.skipped,
        a.executed,
        a.killed,
        a.survived,
        a.timed_out,
        a.inconclusive,
        a.errored,
        a.not_run,
        a.expected,
    );
    debug_assert!(written.is_ok(), "writing to a String cannot fail");
    match &document.score {
        Some(score) => {
            let percent = score.value * 100.0;
            let written = writeln!(
                text,
                "SCORE     {percent:.1}%  ({} detected of {} decided)",
                score.detected, score.decided
            );
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
        None => text.push_str("SCORE     none; the run decided nothing\n"),
    }
    if !document.findings.is_empty() {
        text.push('\n');
        for one in &document.findings {
            let written = writeln!(text, "{:<22} {}", one.kind, one.detail);
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
    }
    if document.run.interrupted {
        text.push_str("\nINTERRUPTED  the run stopped before every mutant was executed\n");
    }
    text
}
