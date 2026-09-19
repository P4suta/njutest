// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a completed verification says, and what a durable one must satisfy.

pub mod across;
pub mod audit;
pub mod hollow;
pub mod html;
pub mod json;
pub mod junit;
pub mod lines;
pub mod merge;
pub mod sarif;
pub mod spec;

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
#[non_exhaustive]
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

/// Whether facts were established by the run that wrote them down, or read back from another.
///
/// The wire carries a flag beside a name, which can spell two things nothing
/// means: read back from nobody, and established here and also somewhere
/// else. A reader who met either could not tell which half to believe, so
/// both were refused when a report was written and both are now unwritable.
/// The name of a source is never empty for the same reason: a source run with
/// no name is no source at all.
///
/// One thing this cannot refuse is a report naming its own run as the one it
/// read back from, because that needs the run's identity and this holds only
/// the source's. `report::audit` still says so.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Established {
    /// This run established them.
    #[default]
    Here,
    /// They were read back from the run named.
    ReadBackFrom(String),
}

impl Established {
    /// The run these were read back from, where they were read back at all.
    #[must_use]
    pub const fn read_back(&self) -> Option<&String> {
        match self {
            Self::Here => None,
            Self::ReadBackFrom(run) => Some(run),
        }
    }

    /// The pair the wire carries: whether it was read back, and from where.
    #[must_use]
    pub fn pair(&self) -> (bool, Option<String>) {
        match self {
            Self::Here => (false, None),
            Self::ReadBackFrom(run) => (true, Some(run.clone())),
        }
    }

    /// The one thing that pair can mean, where it means anything at all.
    #[must_use]
    pub fn of(read_back: bool, source: Option<String>) -> Option<Self> {
        match (read_back, source) {
            (false, None) => Some(Self::Here),
            (true, Some(run)) if !run.is_empty() => Some(Self::ReadBackFrom(run)),
            (true, None | Some(_)) | (false, Some(_)) => None,
        }
    }

    /// What a reader is told when a document pairs the flag with a name that cannot go with it.
    #[must_use]
    pub fn unreadable(what: &str) -> String {
        format!(
            "{what} says it was read back from an earlier run and names no run, or names one \
             and says it was established here, or names one with no name at all. A reader \
             meeting any of those cannot tell which half to believe"
        )
    }
}

/// Where one mutation's disposition came from, spelled the way a mutation row spells it.
///
/// The same two facts as [`Provenance`] carries about a whole report, under
/// the names the wire has always used here. One type so that the pair can
/// only ever mean one thing in either place, and one place to look when a
/// third spelling of it turns up.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reuse(pub Established);

/// The shape the wire has always carried on a mutation row.
#[derive(Serialize, Deserialize)]
struct PairedReuse {
    reused: bool,
    source_run_id: Option<String>,
}

impl Serialize for Reuse {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (reused, source_run_id) = self.0.pair();
        PairedReuse {
            reused,
            source_run_id,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Reuse {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let held = PairedReuse::deserialize(deserializer)?;
        Established::of(held.reused, held.source_run_id)
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom(Established::unreadable("a mutation")))
    }
}

/// Where a report's facts came from, and the identity they were computed under, so a reader can check the claim rather than take it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Provenance {
    /// The evidence identity of the inputs, which is what a cached answer is keyed on.
    pub identity: String,
    /// Whether this run established them, or read them back from another.
    pub facts: Established,
}

/// The shape the wire has always carried, which is the pair rather than what it means.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairedProvenance {
    identity: String,
    cached: bool,
    source_run_id: Option<String>,
}

impl Serialize for Provenance {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (cached, source_run_id) = self.facts.pair();
        PairedProvenance {
            identity: self.identity.clone(),
            cached,
            source_run_id,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Provenance {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let held = PairedProvenance::deserialize(deserializer)?;
        let facts = Established::of(held.cached, held.source_run_id)
            .ok_or_else(|| serde::de::Error::custom(Established::unreadable("a report")))?;
        Ok(Self {
            identity: held.identity,
            facts,
        })
    }
}

/// What git said about the tree, or that it could not be asked.
///
/// This was six fields with a flag over them and a sentinel string inside.
/// Between them they could spell a tree git could not be asked about that
/// nonetheless has a commit, a branch, uncommitted changes, a list of changed
/// files and a base those were taken against. `report::audit` walked all five
/// and refused each in turn, which meant a reader of an already-written
/// document had to run the audit to find out whether the five agreed. None of
/// the five is writable now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Git {
    /// Git could not be asked, so the run cannot name the commit it verified.
    #[default]
    Unavailable,
    /// What git said about a tree it could be asked about.
    Said(Said),
}

/// What git said about a tree it could be asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    /// The commit, which is never [`UNAVAILABLE`] and never empty.
    pub commit: String,
    /// The branch, which is never [`UNAVAILABLE`] and never empty.
    pub branch: String,
    /// Whether the tree had uncommitted changes.
    pub dirty: bool,
    /// What a changed-scope run was taken against, where the run was one.
    pub against: Option<Against>,
}

/// The revision a changed-scope run was taken against, and the files it found.
///
/// One value, because a base with no list and a list with no base are each
/// half a fact and a reader cannot act on either. A run taken against a base
/// that found nothing changed has a base and an empty list, which is whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Against {
    /// The merge base.
    pub merge_base: String,
    /// The files that differ from it.
    pub changed_files: Vec<String>,
}

impl Git {
    /// What git said, where it was asked.
    #[must_use]
    pub const fn said(&self) -> Option<&Said> {
        match self {
            Self::Unavailable => None,
            Self::Said(said) => Some(said),
        }
    }

    /// The commit, or [`UNAVAILABLE`] where git was not asked.
    ///
    /// The word is made here rather than stored, so nothing can hold a commit
    /// and say it was not asked at the same time.
    #[must_use]
    pub fn commit(&self) -> &str {
        self.said().map_or(UNAVAILABLE, |said| said.commit.as_str())
    }

    /// The branch, or [`UNAVAILABLE`] where git was not asked.
    #[must_use]
    pub fn branch(&self) -> &str {
        self.said().map_or(UNAVAILABLE, |said| said.branch.as_str())
    }

    /// Whether the tree had uncommitted changes, which a tree nobody could ask about does not.
    #[must_use]
    pub fn dirty(&self) -> bool {
        self.said().is_some_and(|said| said.dirty)
    }

    /// What a changed-scope run was taken against, where git was asked and the run was one.
    #[must_use]
    pub fn against(&self) -> Option<&Against> {
        self.said().and_then(|said| said.against.as_ref())
    }
}

/// The six fields the wire has always carried, which is the shape rather than what it means.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairedGit {
    available: bool,
    commit: String,
    branch: String,
    dirty: bool,
    merge_base: Option<String>,
    changed_files: Vec<String>,
}

impl Serialize for Git {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let against = self.against();
        PairedGit {
            available: self.said().is_some(),
            commit: self.commit().to_owned(),
            branch: self.branch().to_owned(),
            dirty: self.dirty(),
            merge_base: against.map(|taken| taken.merge_base.clone()),
            changed_files: against.map_or_else(Vec::new, |taken| taken.changed_files.clone()),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Git {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let held = PairedGit::deserialize(deserializer)?;
        let refuse = |because: &str| serde::de::Error::custom(because.to_owned());
        let against = match (held.merge_base, held.changed_files) {
            (None, files) if files.is_empty() => None,
            (None, _) => {
                return Err(refuse(
                    "a tree says which files differ from a base and names no base; which \
                     revision they differ from is the half a reader would act on",
                ));
            }
            (Some(base), _) if base.is_empty() => {
                return Err(refuse("a base with no name is no base at all"));
            }
            (Some(merge_base), changed_files) => Some(Against {
                merge_base,
                changed_files,
            }),
        };
        if !held.available {
            if held.commit != UNAVAILABLE
                || held.branch != UNAVAILABLE
                || held.dirty
                || against.is_some()
            {
                return Err(refuse(
                    "a tree git could not be asked about carries a fact only git could have \
                     said; a reader cannot tell whether the fact or the refusal is the true \
                     half",
                ));
            }
            return Ok(Self::Unavailable);
        }
        if held.commit.trim().is_empty()
            || held.branch.trim().is_empty()
            || held.commit == UNAVAILABLE
            || held.branch == UNAVAILABLE
        {
            return Err(refuse(
                "a tree git was asked about names the commit and the branch it was asked \
                 about; a run that cannot say which commit it verified has verified nothing \
                 anybody can go and look at",
            ));
        }
        Ok(Self::Said(Said {
            commit: held.commit,
            branch: held.branch,
            dirty: held.dirty,
            against,
        }))
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
    /// How many stopped the program terminating, which counts as noticed.
    pub runaway: u32,
    /// How many this machine stopped waiting for, which counts as nothing.
    #[serde(default)]
    pub waited: u32,
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
    /// Who decided each of them.
    pub observers: ObserverAccounting,
}

/// One target a proof removed from a mutation's question, and the proof that removed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Discharged {
    /// The target, by the identity a report names it with.
    pub target: String,
    /// The proof that removed it.
    pub proof: rust_mutants::session::Proof,
}

/// One target a run actually asked about a mutation, and what it answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answered {
    /// The target, by the identity a report names it with.
    pub target: String,
    /// What it said.
    pub outcome: Outcome,
}

/// Which targets could have noticed one mutation, and what removed the ones that could not.
///
/// The trace carried this and the report did not, so the predicate the
/// assurance contract states — a mutation goes to the tests that reached it,
/// less the ones a proof discharged — could only be checked against
/// diagnostics. ADR 0002 says a trace is never evidence, so a reader holding
/// a survivor to that predicate was holding it to something the run does not
/// answer for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Routing {
    /// How narrowly the run chose.
    pub granularity: rust_mutants::session::Granularity,
    /// The targets that could have noticed it.
    pub reaching: Vec<String>,
    /// The targets a proof removed, each with the proof that removed it.
    pub discharged: Vec<Discharged>,
    /// What widened the question, when the run could not narrow it.
    pub fallback: Option<rust_mutants::session::Fallback>,
    /// The targets the run actually asked, in the order it asked them, with what each answered. A target in `reaching` and not here reached the mutation and was never given the chance, because one asked before it noticed.
    #[serde(default)]
    pub answered: Vec<Answered>,
}

impl Routing {
    /// What a route says, as a report records it.
    #[must_use]
    pub fn of(route: &rust_mutants::session::Route) -> Self {
        Self {
            granularity: route.granularity(),
            reaching: route
                .reaching()
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
            discharged: route
                .discharged()
                .iter()
                .map(|one| Discharged {
                    target: one.target.clone(),
                    proof: one.proof,
                })
                .collect(),
            fallback: route.fallback(),
            answered: Vec::new(),
        }
    }
}

/// Who decided one mutation, which is what stands behind the verdict it feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// The compiler refused the program.
    Types,
    /// A test noticed.
    Tests,
    /// No test of any kind could have noticed, proved rather than run.
    Proved,
    /// It ran and nothing noticed.
    Unnoticed,
    /// Nothing ran at all.
    Unreached,
    /// The mutation stopped the program terminating, and a count said so. No test asserted anything.
    Steps,
    /// A bound expired before anything finished, which establishes nothing about the mutation.
    Waited,
    /// Nothing could be measured: a harness that would not start, or a pair that did not agree.
    Errored,
}

impl Decision {
    /// Every way a mutation can be decided, in the order a reader adds them up.
    pub const ALL: [Self; 8] = [
        Self::Types,
        Self::Tests,
        Self::Steps,
        Self::Proved,
        Self::Unnoticed,
        Self::Unreached,
        Self::Waited,
        Self::Errored,
    ];

    /// What decided a mutation a report records under this outcome, or nothing if no outcome is spelled that way.
    #[must_use]
    pub fn of_outcome(outcome: &str) -> Option<Self> {
        Outcome::parse(outcome).map(Outcome::decision)
    }

    /// How much a mutation decided this way stands on, where less is a weaker run.
    ///
    /// The first four are holes in the verification and the last three are
    /// not; the order inside each group decides only which sentence a reader
    /// is given when two builds disagree.
    #[must_use]
    pub const fn standing(self) -> u8 {
        match self {
            Self::Errored => 0,
            Self::Waited => 1,
            Self::Unnoticed => 2,
            Self::Unreached => 3,
            Self::Types => 4,
            Self::Steps => 5,
            Self::Tests => 6,
            Self::Proved => 7,
        }
    }

    /// Whether this is a gap in the verification rather than something that stands behind the verdict.
    #[must_use]
    pub const fn is_a_hole(self) -> bool {
        self.blind().is_some()
    }

    /// Which way this is a hole, or nothing where somebody answered.
    ///
    /// Matched without a catch-all, so a decision added later is one the
    /// compiler makes somebody place on one side of the line rather than one
    /// that quietly falls on the answered side.
    #[must_use]
    pub const fn blind(self) -> Option<Blind> {
        match self {
            Self::Unnoticed => Some(Blind::Unnoticed),
            Self::Unreached => Some(Blind::Unreached),
            Self::Waited => Some(Blind::Waited),
            Self::Errored => Some(Blind::Errored),
            Self::Types | Self::Tests | Self::Steps | Self::Proved => None,
        }
    }

    /// The wire name a report records.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Types => "types",
            Self::Tests => "tests",
            Self::Steps => "steps",
            Self::Proved => "proved",
            Self::Unnoticed => "unnoticed",
            Self::Unreached => "unreached",
            Self::Waited => "waited",
            Self::Errored => "errored",
        }
    }
}

/// What a run established about one mutation, as the record spells it.
///
/// A closed set rather than a name. The mapping from an outcome to who decided
/// it used to live in two places, and they disagreed: one called a timeout a
/// detection while the run's own finding said an expired budget establishes
/// nothing. One table, read through one function, is what stops that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// The compiler refused the mutated program.
    #[serde(rename = "compile-rejected")]
    CompileRejected,
    /// A test noticed.
    Killed,
    /// The mutation stopped the program terminating, and a count every machine agrees on says so.
    Runaway,
    /// A bound expired before anything finished, which is a fact about the machine that watched.
    Waited,
    /// Every test that could notice ran and none did.
    Survived,
    /// Nothing ran it.
    Unreached,
    /// The compiler rendered it identically, so no observer could tell.
    Equivalent,
    /// A pair did not agree, so nothing was established.
    Unconfirmed,
    /// Nothing could be measured.
    Errored,
}

impl Outcome {
    /// Every outcome a report records.
    pub const ALL: [Self; 9] = [
        Self::CompileRejected,
        Self::Killed,
        Self::Runaway,
        Self::Waited,
        Self::Survived,
        Self::Unreached,
        Self::Equivalent,
        Self::Unconfirmed,
        Self::Errored,
    ];

    /// The name a report records.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CompileRejected => "compile-rejected",
            Self::Killed => "killed",
            Self::Runaway => "runaway",
            Self::Waited => "waited",
            Self::Survived => "survived",
            Self::Unreached => "unreached",
            Self::Equivalent => "equivalent",
            Self::Unconfirmed => "unconfirmed",
            Self::Errored => "errored",
        }
    }

    /// The outcome of that name, or nothing where no outcome is spelled that way.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|one| one.name() == name)
    }

    /// Who decided a mutation this outcome is recorded for.
    ///
    /// `Runaway` is `Steps` and never `Tests`: no test asserted anything, and
    /// a suite with no assertions at all would otherwise collect credit for
    /// every mutation that loops forever. `Waited` is neither — a bound is a
    /// budget, ADR 0004 says a result resting on one is not a proof, and what
    /// the run established is that this machine stopped watching.
    #[must_use]
    pub const fn decision(self) -> Decision {
        match self {
            Self::CompileRejected => Decision::Types,
            Self::Killed => Decision::Tests,
            Self::Runaway => Decision::Steps,
            Self::Waited => Decision::Waited,
            Self::Survived => Decision::Unnoticed,
            Self::Unreached => Decision::Unreached,
            Self::Equivalent => Decision::Proved,
            Self::Unconfirmed | Self::Errored => Decision::Errored,
        }
    }
}

/// Who can decide a question about a seam, which is everybody who could be watching bytes on a socket.
///
/// A closed set of four rather than a [`Decision`], because the type system
/// never sees a fault injected into a socket: `types` is nonsense here, and a
/// report that could spell it is a report that could say something no run
/// could mean.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum SeamDecision {
    /// A test noticed, and this is the one that did.
    Tests {
        /// The target that noticed.
        noticed_by: String,
    },
    /// No observer could have noticed, and this is the proof that says so.
    Proved {
        /// The proof's name.
        proof: String,
    },
    /// The suite ran with it in place and nothing noticed.
    Unnoticed,
    /// The run could not put the question, so it established nothing.
    Unreached,
}

impl SeamDecision {
    /// The decision this is one of.
    #[must_use]
    pub const fn decision(&self) -> Decision {
        match self {
            Self::Tests { .. } => Decision::Tests,
            Self::Proved { .. } => Decision::Proved,
            Self::Unnoticed => Decision::Unnoticed,
            Self::Unreached => Decision::Unreached,
        }
    }

    /// The wire name a report records.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.decision().name()
    }

    /// How much a question decided this way stands on, where less is a weaker run.
    #[must_use]
    pub const fn standing(&self) -> u8 {
        self.decision().standing()
    }

    /// The target that noticed, or the proof that discharged it, where either did.
    #[must_use]
    pub fn by(&self) -> Option<&str> {
        match self {
            Self::Tests { noticed_by } => Some(noticed_by),
            Self::Proved { proof } => Some(proof),
            Self::Unnoticed | Self::Unreached => None,
        }
    }
}

/// What a run established about one mutation, together with whoever established it.
///
/// `outcome` and `killed_by` used to sit beside each other, so a record could
/// say a mutation survived and name the target that killed it, or say a test
/// noticed and name nobody. Worse, the name was wrong three times in four: a
/// timeout, a pair that did not agree and a harness that would not start all
/// filled a field called `killed_by` with a target that killed nothing.
///
/// Each way of being decided names its own payload, so the pairing is a thing
/// the compiler holds and the naming comes out right as a consequence. The
/// wire is unchanged: this writes and reads the same two fields it always did,
/// and refuses a document that pairs them in a way no run could mean.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Decided {
    /// The compiler refused the mutated program.
    CompileRejected,
    /// A test noticed, and this is the one that did.
    Killed {
        /// The target that noticed.
        by: String,
    },
    /// It stopped the program terminating, and a count said so while this target ran it.
    Runaway {
        /// The target it was running under, which is where to go and look rather than what noticed.
        on: String,
    },
    /// A bound expired before anything finished, on this target.
    Waited {
        /// The target it was running against.
        on: String,
    },
    /// Every test that could notice ran and none did.
    Survived,
    /// Nothing ran it.
    Unreached,
    /// The compiler rendered it identically, so no observer could tell.
    Equivalent,
    /// A pair did not agree, on this target.
    Unconfirmed {
        /// The target it was running against.
        on: String,
    },
    /// Nothing could be measured, on this target.
    Errored {
        /// The target it was running against.
        on: String,
    },
}

impl Decided {
    /// Which of the nine this is, without who it was.
    #[must_use]
    pub const fn outcome(&self) -> Outcome {
        match self {
            Self::CompileRejected => Outcome::CompileRejected,
            Self::Killed { .. } => Outcome::Killed,
            Self::Runaway { .. } => Outcome::Runaway,
            Self::Waited { .. } => Outcome::Waited,
            Self::Survived => Outcome::Survived,
            Self::Unreached => Outcome::Unreached,
            Self::Equivalent => Outcome::Equivalent,
            Self::Unconfirmed { .. } => Outcome::Unconfirmed,
            Self::Errored { .. } => Outcome::Errored,
        }
    }

    /// The name a report records.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.outcome().name()
    }

    /// Who decided a mutation this outcome is recorded for.
    #[must_use]
    pub const fn decision(&self) -> Decision {
        self.outcome().decision()
    }

    /// The target this was established against, where it was established against one.
    #[must_use]
    pub fn decided_by(&self) -> Option<&str> {
        match self {
            Self::Killed { by } => Some(by),
            Self::Runaway { on }
            | Self::Waited { on }
            | Self::Unconfirmed { on }
            | Self::Errored { on } => Some(on),
            Self::CompileRejected | Self::Survived | Self::Unreached | Self::Equivalent => None,
        }
    }

    /// One of each, for a test that has to speak for all of them.
    ///
    /// The target in every arm that carries one is that arm's own name, so a
    /// sentence that binds the wrong payload names the wrong target rather
    /// than still reading. A copy-paste between two arms that both carry an
    /// `on` produces English either way; it produces the wrong string only
    /// here.
    #[must_use]
    pub fn every() -> [Self; 9] {
        [
            Self::CompileRejected,
            Self::Killed {
                by: "killed-by".to_owned(),
            },
            Self::Runaway {
                on: "runaway-on".to_owned(),
            },
            Self::Waited {
                on: "waited-on".to_owned(),
            },
            Self::Survived,
            Self::Unreached,
            Self::Equivalent,
            Self::Unconfirmed {
                on: "unconfirmed-on".to_owned(),
            },
            Self::Errored {
                on: "errored-on".to_owned(),
            },
        ]
    }

    /// One of each, every arm that has a target established against the same one.
    ///
    /// The shared name is what makes two arms sharing a sentence show up as
    /// one string rather than two that merely differ in the target. Two arms
    /// can describe the same fact and still read apart when each is handed its
    /// own name, which is how a collapsed sentence survives a distinctness
    /// test built on [`Self::every`].
    #[must_use]
    pub fn every_against(target: &str) -> [Self; 9] {
        [
            Self::CompileRejected,
            Self::Killed {
                by: target.to_owned(),
            },
            Self::Runaway {
                on: target.to_owned(),
            },
            Self::Waited {
                on: target.to_owned(),
            },
            Self::Survived,
            Self::Unreached,
            Self::Equivalent,
            Self::Unconfirmed {
                on: target.to_owned(),
            },
            Self::Errored {
                on: target.to_owned(),
            },
        ]
    }

    /// What `outcome` and `decided_by` name together, or nothing where no run could mean the pair.
    ///
    /// # Errors
    /// Nothing, as an `Option`: an outcome no run spells, an outcome that is
    /// established against a target with none named, or one that is not with a
    /// target named anyway.
    #[must_use]
    pub fn of(outcome: Outcome, decided_by: Option<String>) -> Option<Self> {
        match (outcome, decided_by) {
            (Outcome::Killed, Some(by)) => Some(Self::Killed { by }),
            (Outcome::Runaway, Some(on)) => Some(Self::Runaway { on }),
            (Outcome::Waited, Some(on)) => Some(Self::Waited { on }),
            (Outcome::Unconfirmed, Some(on)) => Some(Self::Unconfirmed { on }),
            (Outcome::Errored, Some(on)) => Some(Self::Errored { on }),
            (Outcome::CompileRejected, None) => Some(Self::CompileRejected),
            (Outcome::Survived, None) => Some(Self::Survived),
            (Outcome::Unreached, None) => Some(Self::Unreached),
            (Outcome::Equivalent, None) => Some(Self::Equivalent),
            (
                Outcome::Killed
                | Outcome::Runaway
                | Outcome::Waited
                | Outcome::Unconfirmed
                | Outcome::Errored
                | Outcome::CompileRejected
                | Outcome::Survived
                | Outcome::Unreached
                | Outcome::Equivalent,
                _,
            ) => None,
        }
    }
}

/// The two fields a report has always written, which is what [`Decided`] is carried as.
#[derive(Serialize, Deserialize)]
struct Paired {
    outcome: Outcome,
    killed_by: Option<String>,
}

impl Serialize for Decided {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Paired {
            outcome: self.outcome(),
            killed_by: self.decided_by().map(ToOwned::to_owned),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Decided {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let held = Paired::deserialize(deserializer)?;
        let said = held.outcome.name();
        Self::of(held.outcome, held.killed_by).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "a mutation recorded as {said} is paired with a target it could not have \
                 been established against, or with none where it was"
            ))
        })
    }
}

/// The ways a mutation can be a hole, which is every way short of somebody answering for it.
///
/// A closed set of exactly the decisions that leave a hole. `blind_in` carries
/// this rather than a [`Decision`] because a build that answered is not one
/// anybody is blind in: writing that down is a state a report must never hold,
/// and a type that cannot spell it is a proof where a check would have been a
/// promise somebody keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Blind {
    /// The tests ran it and nothing noticed.
    Unnoticed,
    /// Nothing ran it.
    Unreached,
    /// A bound expired before anything finished.
    Waited,
    /// Nothing could be measured: a harness that would not start, or a pair that did not agree.
    Errored,
}

impl Blind {
    /// Every way a mutation can be a hole.
    pub const ALL: [Self; 4] = [
        Self::Unnoticed,
        Self::Unreached,
        Self::Waited,
        Self::Errored,
    ];

    /// The decision this is one of.
    #[must_use]
    pub const fn decision(self) -> Decision {
        match self {
            Self::Unnoticed => Decision::Unnoticed,
            Self::Unreached => Decision::Unreached,
            Self::Waited => Decision::Waited,
            Self::Errored => Decision::Errored,
        }
    }

    /// The wire name a report records.
    #[must_use]
    pub const fn name(self) -> &'static str {
        self.decision().name()
    }

    /// Whether nothing answered here, as against the tests having been there and missed it.
    ///
    /// A run that read these two together would count a broken harness among
    /// the chances a suite failed to take, and put the count behind the
    /// accusation.
    #[must_use]
    pub const fn is_unanswered(self) -> bool {
        match self {
            Self::Waited | Self::Errored => true,
            Self::Unnoticed | Self::Unreached => false,
        }
    }
}

/// One build a mutation is a hole in, and what that build established about it.
///
/// The name alone would make a reader believe the same thing happened in every
/// build it lists. A build whose tests ran and noticed nothing wants a test
/// written; a build that established nothing wants somebody to find out why
/// first, and telling them to write a test sends them looking for an assertion
/// that is not what is missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlindIn {
    /// The build, as `[[configuration]]` names it.
    pub build: String,
    /// What that build established, which is one of the ways a mutation is a hole and cannot be anything else.
    pub decision: Blind,
}

/// One question a seam's recording licensed, and what the run made of it.
///
/// A `wire-unnoticed` finding names a question by its identity, and a reader
/// who cannot look that identity up has been handed a name and no way to know
/// what it stands for. This is what they look it up in: ADR 0002 keeps a finding
/// off the recording, so what the finding rests on has to be in the report.
///
/// Not `deny_unknown_fields`: serde cannot refuse an unknown field and flatten
/// one in the same breath, and who decided a question has to be a field of the
/// record rather than a table under it. The published schema is what refuses a
/// document with something extra in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeamRecord {
    /// The question's identity, which is what a finding names.
    pub id: String,
    /// The capability the seam serves, as `[resources.<name>]` names it.
    pub capability: String,
    /// Which exchange on that seam, from zero.
    pub seq: u64,
    /// What the caller asked, where the wire says how to read one. Empty where it does not.
    pub asked: String,
    /// What the upstream answered, where the wire says how to read one. `None` where it does not.
    pub answered: Option<u16>,
    /// What the question asks of the exchange.
    pub rule: crate::wire::rule::Rule,
    /// Who decided it, and — where somebody did — who that was.
    #[serde(flatten)]
    pub decision: SeamDecision,
}

/// Who noticed each mutation the run catalogued, and what became of the ones nobody did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObserverAccounting {
    /// How many the compiler refused, which is the type system noticing.
    pub types: u32,
    /// How many a test noticed.
    pub tests: u32,
    /// How many stopped the program terminating, which a count established and no test asserted.
    pub steps: u32,
    /// How many no test of any kind could have noticed, proved rather than run.
    pub proved: u32,
    /// How many ran with nothing noticing.
    pub unnoticed: u32,
    /// How many nothing ran at all.
    pub unreached: u32,
    /// How many a bound expired on before anything finished, which establishes nothing about them.
    pub waited: u32,
    /// How many nothing could be measured about, which is a gap in the verification rather than in the project.
    pub errored: u32,
}

impl ObserverAccounting {
    /// Count one more mutation decided this way.
    pub const fn counted(&mut self, decision: Decision) {
        let column = match decision {
            Decision::Types => &mut self.types,
            Decision::Tests => &mut self.tests,
            Decision::Steps => &mut self.steps,
            Decision::Proved => &mut self.proved,
            Decision::Unnoticed => &mut self.unnoticed,
            Decision::Unreached => &mut self.unreached,
            Decision::Waited => &mut self.waited,
            Decision::Errored => &mut self.errored,
        };
        *column = column.saturating_add(1);
    }

    /// How many mutations these columns account for.
    #[must_use]
    pub fn total(&self) -> u32 {
        Decision::ALL
            .into_iter()
            .fold(0, |sum, one| sum.saturating_add(self.of(one)))
    }

    /// The column one decision is counted in.
    ///
    /// Read through here rather than by adding the fields up by hand. A chain
    /// over the struct is told nothing when the set of decisions grows: the
    /// eighth column was added and the sum kept six, and only a law caught it
    /// — a runtime test standing in for a compile error. Matched against the
    /// closed set, a ninth variant breaks this one function and everything
    /// that adds them up follows for free (ADR 0023).
    #[must_use]
    pub const fn of(&self, decision: Decision) -> u32 {
        match decision {
            Decision::Types => self.types,
            Decision::Tests => self.tests,
            Decision::Steps => self.steps,
            Decision::Proved => self.proved,
            Decision::Unnoticed => self.unnoticed,
            Decision::Unreached => self.unreached,
            Decision::Waited => self.waited,
            Decision::Errored => self.errored,
        }
    }
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
#[non_exhaustive]
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
///
/// Not `deny_unknown_fields`: serde cannot refuse an unknown field and flatten
/// one in the same breath, and what the run established has to be two fields of
/// the record rather than a table under it. The published schema is what
/// refuses a document with something extra in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// The item the mutation sits in, which is how a reader names it after editing the file.
    #[serde(default)]
    pub item: String,
    /// The bytes the edit replaces, which narrow a locator to one of several on a line.
    #[serde(default)]
    pub original: String,
    /// The bytes it puts there instead, which is what a reader has to see to know what was asked of their tests.
    #[serde(default)]
    pub replacement: String,
    /// What the run established, and the target it was established against where there was one.
    #[serde(flatten)]
    pub outcome: Decided,
    /// The builds it is a hole in, each with what that build established. Empty when the run measured one build or every build answered for it.
    #[serde(default)]
    pub blind_in: Vec<BlindIn>,
    /// Which targets could have noticed it, and what removed the rest. `null` where the run never asked.
    #[serde(default)]
    pub routing: Option<Routing>,
    /// Whether this run established the disposition, or read it back from another.
    #[serde(flatten)]
    pub reuse: Reuse,
}

/// What kind of thing a run found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingKind {
    /// The workspace does not compile.
    BuildFailure,
    /// A test of the workspace fails.
    FailingTest,
    /// A test target could not be found, so nothing was observed about it.
    TargetMissing,
    /// A mutant nothing noticed.
    SurvivingMutant,
    /// A target that ran out of the time it was given.
    Timeout,
    /// A mutation's bound expired before anything finished, so nothing was established about it.
    WaitedMutant,
    /// Something a run could not measure, so it claims nothing about it.
    NotMeasured,
    /// An unexpired acceptance does not name exactly one mutant in this catalog.
    UnmatchedAcceptance,
    /// The interpreter found unsoundness in what the compiler cannot check.
    UndefinedBehaviour,
    /// A target was put to mutations and noticed none of them.
    HollowTarget,
    /// The suite carried on through a question a seam licensed: a fault nothing noticed.
    WireUnnoticed,
}

impl FindingKind {
    /// Every kind, in declaration order.
    pub const ALL: [Self; 11] = [
        Self::BuildFailure,
        Self::FailingTest,
        Self::TargetMissing,
        Self::SurvivingMutant,
        Self::Timeout,
        Self::WaitedMutant,
        Self::NotMeasured,
        Self::UnmatchedAcceptance,
        Self::UndefinedBehaviour,
        Self::HollowTarget,
        Self::WireUnnoticed,
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
            Self::WaitedMutant => "waited-mutant",
            Self::NotMeasured => "not-measured",
            Self::UnmatchedAcceptance => "unmatched-acceptance",
            Self::UndefinedBehaviour => "undefined-behaviour",
            Self::HollowTarget => "hollow-target",
            Self::WireUnnoticed => "wire-unnoticed",
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
            | Self::WaitedMutant
            | Self::NotMeasured
            | Self::UnmatchedAcceptance
            | Self::HollowTarget
            | Self::WireUnnoticed => false,
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
    /// Every question a watched seam licensed, and what became of it.
    #[serde(default)]
    pub seams: Vec<SeamRecord>,
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
                git: Git::Unavailable,
            },
            provenance: Provenance {
                identity: UNAVAILABLE.to_owned(),
                facts: Established::Here,
            },
            scope: Scope::default(),
            timing: Timing::default(),
            accounting: Accounting::default(),
            resources: Vec::new(),
            candidates: Vec::new(),
            seams: Vec::new(),
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
