// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a completed verification says, and what a durable one must satisfy.

pub mod audit;
pub mod html;
pub mod json;
pub mod junit;
pub mod lines;
pub mod merge;
pub mod sarif;

use serde::{Deserialize, Serialize};

/// Names the contract, and names the toolchain so a reader never confuses it with goatest's report of the same shape.
pub const SCHEMA: &str = "njutest-assurance-report-v1";

/// The version of that shape.
pub const SCHEMA_VERSION: u32 = 1;

/// The sentinel a report uses where a fact was not available. An empty string would read as "nothing to say"; this reads as "we asked".
pub const UNAVAILABLE: &str = "unavailable";

/// What a run concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    /// Everything in scope was verified and nothing was found.
    Assured,
    /// Only the changed code was verified, and nothing was found in it.
    ChangeAssured,
    /// Only the requested scope was verified, and nothing was found in it.
    ScopeAssured,
    /// Something is wrong with the code under test.
    Defect,
    /// The evidence does not support a claim either way.
    #[default]
    Insufficient,
    /// This run judged one part of a catalog and found nothing in it. A part assures nothing on its own, and `njutest merge` is what carries the verdict.
    Partial,
    /// The run could not establish anything.
    Error,
}

impl Verdict {
    /// Every verdict, in declaration order.
    pub const ALL: [Self; 7] = [
        Self::Assured,
        Self::ChangeAssured,
        Self::ScopeAssured,
        Self::Defect,
        Self::Insufficient,
        Self::Partial,
        Self::Error,
    ];

    /// The word a report carries and a person reads.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Assured => "ASSURED",
            Self::ChangeAssured => "CHANGE_ASSURED",
            Self::ScopeAssured => "SCOPE_ASSURED",
            Self::Defect => "DEFECT",
            Self::Insufficient => "INSUFFICIENT",
            Self::Partial => "PARTIAL",
            Self::Error => "ERROR",
        }
    }

    /// Whether this verdict says the code was assured, in whatever scope.
    #[must_use]
    pub const fn is_assurance(self) -> bool {
        matches!(
            self,
            Self::Assured | Self::ChangeAssured | Self::ScopeAssured
        )
    }

    /// The exit code this verdict earns.
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Assured | Self::ChangeAssured | Self::ScopeAssured | Self::Partial => {
                crate::cli::EXIT_ASSURED
            }
            Self::Defect => crate::cli::EXIT_DEFECT,
            Self::Insufficient => crate::cli::EXIT_INSUFFICIENT,
            Self::Error => crate::cli::EXIT_ERROR,
        }
    }
}

/// How much of the workspace a run looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunKind {
    /// Everything in the workspace.
    #[default]
    Full,
    /// Only what changed.
    Changed,
    /// Only what the caller asked for.
    Scoped,
}

/// A place in a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    /// The 1-based line.
    pub line: u32,
    /// The 1-based UTF-8 byte column.
    pub column: u32,
    /// The 1-based Unicode scalar column.
    pub character_column: u32,
}

impl Position {
    /// The position of byte `offset` within `line_text`, on line `line`.
    #[must_use]
    pub fn of(line_text: &str, line: u32, offset: usize) -> Self {
        let prefix = line_text.get(..offset).unwrap_or(line_text);
        Self {
            line,
            column: u32::try_from(prefix.len())
                .unwrap_or(u32::MAX)
                .saturating_add(1),
            character_column: u32::try_from(prefix.chars().count())
                .unwrap_or(u32::MAX)
                .saturating_add(1),
        }
    }
}

/// What produced the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    /// The runner's version.
    pub njutest: String,
    /// The engine's version.
    pub rust_mutants: String,
}

impl Default for Tool {
    fn default() -> Self {
        Self {
            njutest: crate::VERSION.to_owned(),
            rust_mutants: rust_mutants::VERSION.to_owned(),
        }
    }
}

/// What compiled and ran the code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Toolchain {
    /// The whole of `rustc -vV`'s first line.
    pub rustc: String,
    /// The whole of `cargo -vV`'s first line.
    pub cargo: String,
    /// The target triple everything was built for.
    pub target: String,
    /// The operating system.
    pub os: String,
    /// The architecture.
    pub arch: String,
}

impl Default for Toolchain {
    fn default() -> Self {
        Self {
            rustc: UNAVAILABLE.to_owned(),
            cargo: UNAVAILABLE.to_owned(),
            target: UNAVAILABLE.to_owned(),
            os: UNAVAILABLE.to_owned(),
            arch: UNAVAILABLE.to_owned(),
        }
    }
}

/// What the repository was when the run started.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    /// The name of the workspace root directory.
    pub root_name: String,
    /// Every package the workspace holds.
    pub packages: Vec<String>,
    /// The frozen digest of the tree that was verified.
    pub workspace_digest: String,
    /// The SHA-256 of the effective configuration.
    pub configuration_digest: String,
    /// What git said, or that it could not be asked.
    pub git: Git,
}

/// Where a report's facts came from. A run that read an earlier run's answer says so, names the run that established it, and carries the identity both were computed under, so a reader can check the claim rather than take it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// The evidence identity of the inputs, which is what a cached answer is keyed on.
    pub identity: String,
    /// Whether every fact here was read back rather than established.
    pub cached: bool,
    /// The run that established them, when it was not this one.
    pub source_run_id: Option<String>,
}

/// What git said about the tree, or that it could not be asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Git {
    /// Whether git could be asked at all.
    pub available: bool,
    /// The commit, or [`UNAVAILABLE`].
    pub commit: String,
    /// The branch, or [`UNAVAILABLE`].
    pub branch: String,
    /// Whether the tree had uncommitted changes.
    pub dirty: bool,
    /// The merge base a changed-scope run was taken against.
    pub merge_base: Option<String>,
    /// The files a changed-scope run found.
    pub changed_files: Vec<String>,
}

impl Git {
    /// The state of a tree git could not be asked about.
    #[must_use]
    pub fn unavailable() -> Self {
        Self {
            available: false,
            commit: UNAVAILABLE.to_owned(),
            branch: UNAVAILABLE.to_owned(),
            dirty: false,
            merge_base: None,
            changed_files: Vec::new(),
        }
    }
}

impl Default for Git {
    fn default() -> Self {
        Self::unavailable()
    }
}

/// What the run was asked to verify and what it settled on.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    /// The packages the caller asked for; empty is the whole workspace.
    pub requested_packages: Vec<String>,
    /// The packages the run settled on.
    pub resolved_packages: Vec<String>,
    /// The patterns a file had to match for anything in it to be mutated.
    #[serde(default)]
    pub included: Vec<String>,
    /// The patterns that removed files from the scope.
    pub excluded: Vec<String>,
    /// The file these came from, so a reader knows which of two configurations they are looking at.
    #[serde(default)]
    pub configuration: String,
    /// Which part of the catalog this run judged, as `K/N`, or nothing when it judged every one.
    #[serde(default)]
    pub shard: Option<String>,
}

/// How many targets there were and what became of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetAccounting {
    /// How many the run selected.
    pub selected: u32,
    /// How many passed.
    pub passed: u32,
    /// How many failed.
    pub failed: u32,
    /// How many were skipped, by libtest or by the run.
    pub skipped: u32,
    /// How many could not be found at all, which is fail-closed rather than a pass.
    pub missing: u32,
}

impl TargetAccounting {
    /// The sum of the terminal states, which must equal `selected`.
    #[must_use]
    pub const fn accounted(self) -> u32 {
        self.passed
            .saturating_add(self.failed)
            .saturating_add(self.skipped)
            .saturating_add(self.missing)
    }
}

/// How many mutants there were and what became of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutantAccounting {
    /// How many the catalog held.
    pub cataloged: u32,
    /// How many the compiler refused.
    pub rejected: u32,
    /// How many the run executed.
    pub executed: u32,
    /// How many a test noticed.
    pub killed: u32,
    /// How many nothing noticed.
    pub survived: u32,
    /// How many timed out, which counts as noticed.
    pub timed_out: u32,
    /// How many no test could reach.
    pub unreached: u32,
    /// How many the compiler renders identically to the code they mutate, which no test could have noticed.
    #[serde(default)]
    pub equivalent: u32,
    /// How many a reviewer accepted with a reason.
    pub accepted: u32,
    /// How many of `killed` came from a previous run.
    pub reused_killed: u32,
    /// How many of `survived` came from a previous run.
    pub reused_survived: u32,
}

/// The soundness phase's inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SoundnessAccounting {
    /// How many `unsafe` blocks, functions, impls, and traits were found.
    pub unsafe_items: u32,
    /// How many packages hold one.
    pub packages_with_unsafe: u32,
    /// Whether anything was executed about them.
    pub executed: bool,
}

/// Everything a run counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accounting {
    /// The targets.
    pub targets: TargetAccounting,
    /// The mutants.
    pub mutants: MutantAccounting,
    /// The soundness inventory.
    pub soundness: SoundnessAccounting,
}

/// What became of one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetStatus {
    /// It ran and passed.
    Passed,
    /// It ran and failed.
    Failed,
    /// It did not run: libtest ignored it, or the run did.
    Skipped,
    /// It could not be found, which is fail-closed rather than a pass.
    Missing,
}

/// One target the run selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRecord {
    /// The stable identity.
    pub id: String,
    /// What a person calls it.
    pub name: String,
    /// The package that owns it.
    pub package: String,
    /// What became of it.
    pub status: TargetStatus,
    /// How long it took.
    pub duration_ms: u64,
    /// What it said, when that matters.
    pub message: Option<String>,
}

/// What became of one mutant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutantRecord {
    /// The full identity.
    pub id: String,
    /// The short identity a person types.
    pub display_id: String,
    /// The workspace-relative path.
    pub path: String,
    /// Where the edit is.
    pub position: Position,
    /// The rule that proposed it.
    pub rule: String,
    /// What the run established.
    pub outcome: String,
    /// The target that noticed it, when one did.
    pub killed_by: Option<String>,
    /// Whether this came from a previous run.
    pub reused: bool,
    /// Which run it came from.
    pub source_run_id: Option<String>,
}

/// What kind of thing a run found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum FindingKind {
    /// The workspace does not compile.
    BuildFailure,
    /// A test of the workspace fails.
    FailingTest,
    /// A test target could not be found, so nothing was observed about it.
    TargetMissing,
    /// A mutant nothing noticed.
    SurvivingMutant,
    /// A target, or a mutation of one, that ran out of time.
    Timeout,
    /// Something a run could not measure, so it claims nothing about it.
    NotMeasured,
    /// An unexpired acceptance does not name exactly one mutant in this catalog.
    UnmatchedAcceptance,
    /// The interpreter found unsoundness in what the compiler cannot check.
    UndefinedBehaviour,
}

impl FindingKind {
    /// Every kind, in declaration order.
    pub const ALL: [Self; 8] = [
        Self::BuildFailure,
        Self::FailingTest,
        Self::TargetMissing,
        Self::SurvivingMutant,
        Self::Timeout,
        Self::NotMeasured,
        Self::UnmatchedAcceptance,
        Self::UndefinedBehaviour,
    ];

    /// The name this carries in a report, which is the one a person greps for.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::BuildFailure => "build-failure",
            Self::FailingTest => "failing-test",
            Self::TargetMissing => "target-missing",
            Self::SurvivingMutant => "surviving-mutant",
            Self::Timeout => "timeout",
            Self::NotMeasured => "not-measured",
            Self::UnmatchedAcceptance => "unmatched-acceptance",
            Self::UndefinedBehaviour => "undefined-behaviour",
        }
    }

    /// Whether this is a fault in the code under test rather than a gap in what was established.
    #[must_use]
    pub const fn is_defect(self) -> bool {
        match self {
            Self::BuildFailure | Self::FailingTest | Self::UndefinedBehaviour => true,
            Self::TargetMissing
            | Self::SurvivingMutant
            | Self::Timeout
            | Self::NotMeasured
            | Self::UnmatchedAcceptance => false,
        }
    }
}

/// One actionable problem a run found in the project or its verification configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    /// What kind of thing it is.
    pub kind: FindingKind,
    /// What it is about: a target identity, a mutant, a package, or a configured acceptance.
    pub subject: String,
    /// One sentence a person can act on.
    pub detail: String,
    /// The file it is in, when the run knows, relative to the workspace root.
    #[serde(default)]
    pub path: Option<String>,
    /// Where it is, when the run knows.
    pub position: Option<Position>,
}

impl Finding {
    /// The wire name of this finding's kind.
    #[must_use]
    pub fn kind_name(&self) -> String {
        serde_json::to_value(self.kind)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| UNAVAILABLE.to_owned())
    }

    /// A finding of `kind` about `subject`.
    #[must_use]
    pub fn new(kind: FindingKind, subject: &str, detail: &str) -> Self {
        Self {
            kind,
            subject: subject.to_owned(),
            detail: detail.to_owned(),
            path: None,
            position: None,
        }
    }
}

/// One integration resource a run held while its tests ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceRecord {
    /// The capability the resource provides.
    pub capability: String,
    /// The instance its provider named.
    pub instance: String,
    /// The variable names its provider set for every test process, in name order. Never a value: a report is read by people who may not hold the secret in it.
    pub environment: Vec<String>,
}

/// One repair a generation provider offered, and what putting it to the tests established.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateRecord {
    /// The finding it would close.
    pub finding: String,
    /// The mutant that finding is about.
    pub mutant: String,
    /// `patch` or `corpus`.
    pub kind: String,
    /// Where it would be written, workspace-relative.
    pub path: String,
    /// The SHA-256 of its content, which is also where the run kept it.
    pub digest: String,
    /// The SHA-256 of the file it patches, absent when it creates one.
    pub preimage: Option<String>,
    /// How many times the patched tree passed with nothing active.
    pub stability_runs: u32,
    /// How many times the patched tree noticed the mutant.
    pub kill_runs: u32,
    /// Whether it may be applied.
    pub accepted: bool,
    /// Why it may not, when it may not.
    pub why: Option<String>,
}

/// One thing a report cannot claim, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limitation {
    /// The stable name a reader can grep for.
    pub name: String,
    /// One sentence saying what is not claimed.
    pub detail: String,
}

impl Limitation {
    /// A limitation named `name`.
    #[must_use]
    pub fn new(name: &str, detail: &str) -> Self {
        Self {
            name: name.to_owned(),
            detail: detail.to_owned(),
        }
    }
}

/// When a run happened and how long it took.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    /// When it started, RFC 3339.
    pub started: String,
    /// When it finished, RFC 3339.
    pub finished: String,
    /// How long it took.
    pub duration_ms: u64,
}

/// One completed verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    /// [`SCHEMA`].
    pub schema: String,
    /// [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// The run's identity, which is also its directory name.
    pub run_id: String,
    /// How much of the workspace it looked at.
    pub run_kind: RunKind,
    /// Which contract it answered to.
    pub contract: crate::config::Contract,
    /// What it concluded.
    pub verdict: Verdict,
    /// What produced it.
    pub tool: Tool,
    /// What compiled and ran the code.
    pub toolchain: Toolchain,
    /// What the repository was.
    pub repository: Repository,
    /// Where the facts here came from: this run, or an earlier one of the same inputs.
    pub provenance: Provenance,
    /// What was asked for and what was settled on.
    pub scope: Scope,
    /// When it happened.
    pub timing: Timing,
    /// Everything it counted.
    pub accounting: Accounting,
    /// Every integration resource it started, in the order it started them.
    #[serde(default)]
    pub resources: Vec<ResourceRecord>,
    /// Every repair a provider offered, and what putting it to the tests established.
    #[serde(default)]
    pub candidates: Vec<CandidateRecord>,
    /// Every target it selected, slowest first.
    pub targets: Vec<TargetRecord>,
    /// Every mutant it has something to say about.
    pub mutants: Vec<MutantRecord>,
    /// Every actionable problem it found in the project or its verification configuration.
    pub findings: Vec<Finding>,
    /// Everything it is not claiming.
    pub limitations: Vec<Limitation>,
}

impl Report {
    /// An empty report of one run.
    #[must_use]
    pub fn new(run_id: &str, run_kind: RunKind, contract: crate::config::Contract) -> Self {
        Self {
            schema: SCHEMA.to_owned(),
            schema_version: SCHEMA_VERSION,
            run_id: run_id.to_owned(),
            run_kind,
            contract,
            verdict: Verdict::Insufficient,
            tool: Tool::default(),
            toolchain: Toolchain::default(),
            repository: Repository {
                root_name: UNAVAILABLE.to_owned(),
                packages: Vec::new(),
                workspace_digest: UNAVAILABLE.to_owned(),
                configuration_digest: UNAVAILABLE.to_owned(),
                git: Git::unavailable(),
            },
            provenance: Provenance {
                identity: UNAVAILABLE.to_owned(),
                cached: false,
                source_run_id: None,
            },
            scope: Scope::default(),
            timing: Timing::default(),
            accounting: Accounting::default(),
            resources: Vec::new(),
            candidates: Vec::new(),
            targets: Vec::new(),
            mutants: Vec::new(),
            findings: Vec::new(),
            limitations: Vec::new(),
        }
    }

    /// Puts the targets in the canonical order: slowest first, then by identity, so two runs of the same work produce the same document.
    pub fn sort_targets(&mut self) {
        self.targets.sort_by(|a, b| {
            b.duration_ms
                .cmp(&a.duration_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
    }

    /// Counts the target rows this report holds, which is where its target accounting comes from.
    pub fn count_targets(&mut self) {
        let counts = &mut self.accounting.targets;
        *counts = TargetAccounting {
            selected: u32::try_from(self.targets.len()).unwrap_or(u32::MAX),
            ..TargetAccounting::default()
        };
        for target in &self.targets {
            match target.status {
                TargetStatus::Passed => counts.passed = counts.passed.saturating_add(1),
                TargetStatus::Failed => counts.failed = counts.failed.saturating_add(1),
                TargetStatus::Skipped => counts.skipped = counts.skipped.saturating_add(1),
                TargetStatus::Missing => counts.missing = counts.missing.saturating_add(1),
            }
        }
    }

    /// What these observations support.
    #[must_use]
    pub fn concluded(&self) -> Verdict {
        if self.findings.iter().any(|finding| finding.kind.is_defect()) {
            return Verdict::Defect;
        }
        if !self.findings.is_empty() {
            return Verdict::Insufficient;
        }
        let observed = self.accounting.targets.passed > 0;
        let asked = self.accounting.mutants.executed > 0;
        if !observed || !asked {
            return Verdict::Insufficient;
        }
        if self.scope.shard.is_some() {
            return Verdict::Partial;
        }
        match self.run_kind {
            RunKind::Full => Verdict::Assured,
            RunKind::Changed => Verdict::ChangeAssured,
            RunKind::Scoped => Verdict::ScopeAssured,
        }
    }

    /// Whether a limitation of this name is stated.
    #[must_use]
    pub fn states(&self, name: &str) -> bool {
        self.limitations
            .iter()
            .any(|limitation| limitation.name == name)
    }
}
