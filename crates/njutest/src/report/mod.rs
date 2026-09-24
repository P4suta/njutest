// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a completed verification says, and what a durable one must satisfy.

pub mod across;
pub mod audit;
pub mod drift;
pub mod faults;
pub mod hollow;
pub mod html;
pub mod json;
pub mod junit;
pub mod lines;
pub mod merge;
pub mod sarif;
pub mod spec;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

pub use rust_mutants::cargo::BuildSelection;

/// Names the contract, and names the toolchain so a reader never confuses it with goatest's report of the same shape.
pub const SCHEMA: &str = "njutest-assurance-report-v1";

/// The schema identity of one deliberately partial shard document.
pub const SHARD_SCHEMA: &str = "njutest-assurance-shard-report-v1";

/// The version of that shape.
pub const SCHEMA_VERSION: u32 = 2;

/// The sentinel a report uses where a fact was not available.
/// An empty string would read as "nothing to say"; this reads as "we asked".
pub const UNAVAILABLE: &str = "unavailable";

/// A canonical configured-build name retained by durable evidence.
///
/// Configuration parsing and report deserialization both cross this boundary,
/// so empty, padded, or control-bearing names cannot enter a completed evidence ledger and later acquire a second interpretation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct BuildName(String);

/// Why text cannot identify one configured build.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildNameError {
    /// The name had no characters.
    #[error("a configured build has no name")]
    Empty,
    /// Trimming the name would change its identity.
    #[error("configured build name {name:?} has leading or trailing whitespace")]
    Padded {
        /// The rejected spelling.
        name: String,
    },
    /// The name contained a control character.
    #[error("configured build name {name:?} contains a control character")]
    Control {
        /// The rejected spelling.
        name: String,
    },
}

impl BuildName {
    /// The exact canonical spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BuildName {
    type Error = BuildNameError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        if name.is_empty() {
            return Err(BuildNameError::Empty);
        }
        if name.trim() != name {
            return Err(BuildNameError::Padded { name });
        }
        if name.chars().any(char::is_control) {
            return Err(BuildNameError::Control { name });
        }
        Ok(Self(name))
    }
}

impl TryFrom<&str> for BuildName {
    type Error = BuildNameError;

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        Self::try_from(name.to_owned())
    }
}

impl fmt::Display for BuildName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for BuildName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// What a run concluded.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
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
    Insufficient,
    /// This run judged one part of a catalog and found nothing in it.
    /// A part assures nothing on its own, and `njutest merge` is what carries the verdict.
    Partial,
    /// The run could not establish anything.
    Error,
}

impl Verdict {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunKind {
    /// Everything in the workspace.
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

/// Why an evidence ledger could not be represented by the report's exact counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CountError {
    /// An in-memory ledger held more rows than the v1 wire counter can name.
    #[error("{ledger} contains {count} rows, outside the v1 u32 counter range")]
    Width {
        /// The ledger or field being counted.
        ledger: &'static str,
        /// The exact in-memory count.
        count: usize,
    },
    /// Adding two individually representable facts exceeded their durable counter.
    #[error("{field} exceeds the v1 u32 counter range")]
    Overflow {
        /// The counter whose exact sum could not be represented.
        field: &'static str,
    },
}

fn count_of(ledger: &'static str, count: usize) -> Result<u32, CountError> {
    u32::try_from(count).map_err(|_outside_wire_range| CountError::Width { ledger, count })
}

fn increment(field: &'static str, count: &mut u32) -> Result<(), CountError> {
    *count = count.checked_add(1).ok_or(CountError::Overflow { field })?;
    Ok(())
}

fn add(field: &'static str, left: u32, right: u32) -> Result<u32, CountError> {
    left.checked_add(right)
        .ok_or(CountError::Overflow { field })
}

impl Position {
    /// The position of byte `offset` within `line_text`, on line `line`.
    /// # Errors
    /// Returns [`CountError`] when either one-based column is outside the v1 wire range.
    #[cfg(feature = "testkit")]
    pub fn of(line_text: &str, line: u32, offset: usize) -> Result<Self, CountError> {
        let prefix = line_text.get(..offset).unwrap_or(line_text);
        Ok(Self {
            line,
            column: count_of("position byte column", prefix.len())?
                .checked_add(1)
                .ok_or(CountError::Overflow {
                    field: "position byte column",
                })?,
            character_column: count_of("position character column", prefix.chars().count())?
                .checked_add(1)
                .ok_or(CountError::Overflow {
                    field: "position character column",
                })?,
        })
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
/// The wire carries a flag beside a name, which can spell two things nothing means: read back from nobody, and established here and also somewhere else.
/// A reader who met either could not tell which half to believe, so both were refused when a report was written and both are now unwritable.
/// The name of a source is never empty for the same reason: a source run with no name is no source at all.
///
/// One thing this cannot refuse is a report naming its own run as the one it read back from, because that needs the run's identity and this holds only the source's.
/// `report::audit` still says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Established {
    /// This run established them.
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
/// The same two facts as [`Provenance`] carries about a whole report, under the names the wire has always used here.
/// One type so that the pair can only ever mean one thing in either place, and one place to look when a third spelling of it turns up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reuse(pub Established);

/// The shape the wire has always carried on a mutation row.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairedReuse {
    reused: bool,
    #[serde(deserialize_with = "crate::strictjson::required_option")]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    #[serde(deserialize_with = "crate::strictjson::required_option")]
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
/// Between them they could spell a tree git could not be asked about that nonetheless has a commit, a branch, uncommitted changes, a list of changed files and a base those were taken against.
/// `report::audit` walked all five and refused each in turn, which meant a reader of an already-written document had to run the audit to find out whether the five agreed.
/// None of the five is writable now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Git {
    /// Git could not be asked, so the run cannot name the commit it verified.
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
/// One value, because a base with no list and a list with no base are each half a fact and a reader cannot act on either.
/// A run taken against a base that found nothing changed has a base and an empty list, which is whole.
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
    /// The word is made here rather than stored, so nothing can hold a commit and say it was not asked at the same time.
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
    #[serde(deserialize_with = "crate::strictjson::required_option")]
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
    pub included: Vec<String>,
    /// The patterns that removed files from the scope.
    pub excluded: Vec<String>,
    /// The file these came from, so a reader knows which of two configurations they are looking at.
    pub configuration: String,
    /// The configured builds the request required, in configuration order.
    /// The completed build ledger must name this exact sequence.
    pub configured_builds: Vec<String>,
    /// Which part of the catalog this run judged, as `K/N`, or nothing when it judged every one.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
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
    ///
    /// # Errors
    /// Returns [`CountError`] instead of fabricating a terminal count when contradictory input exceeds the wire range.
    pub fn accounted(self) -> Result<u32, CountError> {
        let terminal = add("target terminal states", self.passed, self.failed)?;
        let terminal = add("target terminal states", terminal, self.skipped)?;
        add("target terminal states", terminal, self.missing)
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
    /// How many reached the configured deterministic step boundary without a control comparison proving that the mutation caused the divergence.
    pub step_limit_reached: u32,
    /// How many this machine stopped waiting for, which counts as nothing.
    pub waited: u32,
    /// How many no test could reach.
    pub unreached: u32,
    /// How many surviving mutations the model checker distinguished with a counterexample.
    pub model_noticed: u32,
    /// How many surviving mutations the model checker proved equivalent for every admitted input.
    pub model_proved: u32,
    /// How many the compiler renders identically to the code they mutate, which no test could have noticed.
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
/// The trace carried this and the report did not, so the predicate the assurance contract states — a mutation goes to the tests that reached it,
/// less the ones a proof discharged — could only be checked against diagnostics.
/// ADR 0002 says a trace is never evidence, so a reader holding a survivor to that predicate was holding it to something the run does not answer for.
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
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub fallback: Option<rust_mutants::session::Fallback>,
    /// The targets the run actually asked, in the order it asked them, with what each answered.
    /// A target in `reaching` and not here reached the mutation and was never given the chance, because one asked before it noticed.
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
///
/// It carries no `Serialize`: nothing wrote it, and the `snake_case` rename it used to carry spelled two of its names differently from `name()`, which is what every report row, every line, every page and every explanation prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, njutest_macros::AllVariants)]
pub enum Decision {
    /// The compiler refused the program.
    Types,
    /// A test noticed.
    Tests,
    /// The model checker produced a counterexample input.
    ModelNoticed,
    /// The model checker proved equality for every input in the closed domain.
    ModelProved,
    /// No test of any kind could have noticed, proved rather than run.
    Proved,
    /// It ran and nothing noticed.
    Unnoticed,
    /// Nothing ran at all.
    Unreached,
    /// The configured step boundary was reached.
    /// This is an execution fact,
    /// not a verdict about the mutation.
    StepLimitReached,
    /// A bound expired before anything finished, which establishes nothing about the mutation.
    Waited,
    /// Nothing could be measured: a harness that would not start, or a pair that did not agree.
    Errored,
}

impl Decision {
    /// What decided a mutation a report records under this outcome, or nothing if no outcome is spelled that way.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn of_outcome(outcome: &str) -> Option<Self> {
        Outcome::parse(outcome).map(Outcome::decision)
    }

    /// How much a mutation decided this way stands on, where less is a weaker run.
    ///
    /// The first four are holes in the verification and the last three are not; the order inside each group decides only which sentence a reader is given when two builds disagree.
    #[must_use]
    pub const fn standing(self) -> u8 {
        match self {
            Self::Errored => 0,
            Self::Waited => 1,
            Self::StepLimitReached => 2,
            Self::Unnoticed => 3,
            Self::Unreached => 4,
            Self::Types => 5,
            Self::Tests => 6,
            Self::ModelNoticed => 7,
            Self::Proved => 8,
            Self::ModelProved => 9,
        }
    }

    /// Which way this is a hole, or nothing where somebody answered.
    ///
    /// Matched without a catch-all, so a decision added later is one the compiler makes somebody place on one side of the line rather than one that quietly falls on the answered side.
    #[must_use]
    pub const fn blind(self) -> Option<Blind> {
        match self {
            Self::Unnoticed => Some(Blind::Unnoticed),
            Self::Unreached => Some(Blind::Unreached),
            Self::StepLimitReached => Some(Blind::StepLimitReached),
            Self::Waited => Some(Blind::Waited),
            Self::Errored => Some(Blind::Errored),
            Self::Types | Self::Tests | Self::ModelNoticed | Self::ModelProved | Self::Proved => {
                None
            }
        }
    }

    /// The wire name a report records.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Types => "types",
            Self::Tests => "tests",
            Self::ModelNoticed => "model-noticed",
            Self::ModelProved => "model-proved",
            Self::StepLimitReached => "step-limit-reached",
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
/// A closed set rather than a name.
/// The mapping from an outcome to who decided it used to live in two places, and they disagreed: one called a timeout a detection while the run's own finding said an expired budget establishes nothing.
/// One table, read through one function, is what stops that.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// The compiler refused the mutated program.
    #[serde(rename = "compile-rejected")]
    CompileRejected,
    /// A test noticed.
    Killed,
    /// The tests missed it and the model checker produced a distinguishing input.
    ModelNoticed,
    /// The model checker proved the original and mutation equal for every admitted input.
    ModelProved,
    /// The configured step boundary was reached.
    /// Without a matched control it establishes neither detection nor survival.
    StepLimitReached,
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
    /// The name a report records.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CompileRejected => "compile-rejected",
            Self::Killed => "killed",
            Self::ModelNoticed => "model-noticed",
            Self::ModelProved => "model-proved",
            Self::StepLimitReached => "step-limit-reached",
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
    /// A step boundary and a clock boundary are both explicit non-answers.
    /// Neither may become detection credit without a separately represented comparison proving that the mutant diverged from its control.
    #[must_use]
    pub const fn decision(self) -> Decision {
        match self {
            Self::CompileRejected => Decision::Types,
            Self::Killed => Decision::Tests,
            Self::ModelNoticed => Decision::ModelNoticed,
            Self::ModelProved => Decision::ModelProved,
            Self::StepLimitReached => Decision::StepLimitReached,
            Self::Waited => Decision::Waited,
            Self::Survived => Decision::Unnoticed,
            Self::Unreached => Decision::Unreached,
            Self::Equivalent => Decision::Proved,
            Self::Unconfirmed | Self::Errored => Decision::Errored,
        }
    }

    /// Whether a review acceptance can answer this outcome.
    ///
    /// This is the single domain boundary used by producers and durable audits.
    /// In particular, an expired clock or a crossed step allowance is an observation that still needs an answer; it cannot be converted into one by accepting it.
    #[must_use]
    pub const fn review_answerable(self) -> bool {
        matches!(self, Self::Survived | Self::Unreached | Self::Equivalent)
    }

    /// Whether this individual row supports a completed answer.
    ///
    /// `accepted` belongs to the row rather than to an aggregate count.
    /// That prevents an acceptance on one mutation from hiding an unanswered survivor or unreached mutation elsewhere in the report.
    #[must_use]
    pub const fn answered(self, accepted: bool) -> bool {
        match self {
            Self::CompileRejected
            | Self::Killed
            | Self::ModelNoticed
            | Self::ModelProved
            | Self::Equivalent => true,
            Self::Survived | Self::Unreached => accepted,
            Self::StepLimitReached | Self::Waited | Self::Unconfirmed | Self::Errored => false,
        }
    }

    /// The actionable finding this row must carry, after any row-local acceptance has been applied.
    #[must_use]
    pub const fn required_finding(self, accepted: bool) -> Option<FindingKind> {
        match self {
            Self::Survived | Self::Unreached if !accepted => Some(FindingKind::SurvivingMutant),
            Self::StepLimitReached => Some(FindingKind::StepLimitReachedMutant),
            Self::Waited => Some(FindingKind::WaitedMutant),
            Self::Unconfirmed => Some(FindingKind::FailingTest),
            Self::Errored => Some(FindingKind::TargetMissing),
            Self::CompileRejected
            | Self::Killed
            | Self::ModelNoticed
            | Self::ModelProved
            | Self::Equivalent
            | Self::Survived
            | Self::Unreached => None,
        }
    }
}

/// Who can decide a question about a seam, which is everybody who could be watching bytes on a socket.
///
/// A closed set of four rather than a [`Decision`], because the type system never sees a fault injected into a socket: `types` is nonsense here, and a report that could spell it is a report that could say something no run could mean.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case", deny_unknown_fields)]
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
    /// The run established nothing about it: it could not put the question, or it put the question and could not read what the suite did with it.
    ///
    /// Which of the two is in the finding, where a reader looks.
    Unreached,
}

impl SeamDecision {
    /// The decision this is one of.
    #[must_use]
    #[cfg(feature = "testkit")]
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
    #[cfg(feature = "testkit")]
    pub const fn name(&self) -> &'static str {
        self.decision().name()
    }

    /// How much a question decided this way stands on, where less is a weaker run.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn standing(&self) -> u8 {
        self.decision().standing()
    }

    /// The target that noticed, or the proof that discharged it, where either did.
    #[must_use]
    #[cfg(feature = "testkit")]
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
/// `outcome` and `killed_by` used to sit beside each other, so a record could say a mutation survived and name the target that killed it, or say a test noticed and name nobody.
/// Worse, the name was wrong three times in four: a timeout, a pair that did not agree and a harness that would not start all filled a field called `killed_by` with a target that killed nothing.
///
/// Each way of being decided names its own payload, so the pairing is a thing the compiler holds and the naming comes out right as a consequence.
/// Current v1 reports nest the paired fields under `decision`; that closed object refuses a pairing no run could mean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decided {
    /// The compiler refused the mutated program.
    CompileRejected,
    /// A test noticed, and this is the one that did.
    Killed {
        /// The target that noticed.
        by: String,
    },
    /// The model checker produced a distinguishing input.
    ModelNoticed,
    /// The model checker proved equality throughout the closed input domain.
    ModelProved,
    /// The verified runtime guard crossed its configured boundary while this target ran.
    /// This is deliberately a non-verdict.
    StepLimitReached {
        /// The target it was running under.
        on: String,
        /// The checked boundary carried by the verified runtime notice.
        boundary: StepBoundary,
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
            Self::ModelNoticed => Outcome::ModelNoticed,
            Self::ModelProved => Outcome::ModelProved,
            Self::StepLimitReached { .. } => Outcome::StepLimitReached,
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
            Self::StepLimitReached { on, .. }
            | Self::Waited { on }
            | Self::Unconfirmed { on }
            | Self::Errored { on } => Some(on),
            Self::CompileRejected
            | Self::Survived
            | Self::Unreached
            | Self::Equivalent
            | Self::ModelNoticed
            | Self::ModelProved => None,
        }
    }

    /// One of each, for a test that has to speak for all of them.
    ///
    /// The target in every arm that carries one is that arm's own name, so a sentence that binds the wrong payload names the wrong target rather than still reading.
    /// A copy-paste between two arms that both carry an `on` produces English either way; it produces the wrong string only here.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn every() -> [Self; 11] {
        Outcome::ALL.map(|outcome| Self::specimen(outcome, outcome.name()))
    }

    /// One of each, every arm that has a target established against the same one.
    ///
    /// The shared name is what makes two arms sharing a sentence show up as one string rather than two that merely differ in the target.
    /// Two arms can describe the same fact and still read apart when each is handed its own name, which is how a collapsed sentence survives a distinctness test built on [`Self::every`].
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn every_against(target: &str) -> [Self; 11] {
        Outcome::ALL.map(|outcome| Self::specimen(outcome, target))
    }

    #[cfg(feature = "testkit")]
    fn specimen(outcome: Outcome, target: &str) -> Self {
        match outcome {
            Outcome::CompileRejected => Self::CompileRejected,
            Outcome::Killed => Self::Killed {
                by: target.to_owned(),
            },
            Outcome::ModelNoticed => Self::ModelNoticed,
            Outcome::ModelProved => Self::ModelProved,
            Outcome::StepLimitReached => Self::StepLimitReached {
                on: target.to_owned(),
                boundary: StepBoundary::specimen(),
            },
            Outcome::Waited => Self::Waited {
                on: target.to_owned(),
            },
            Outcome::Survived => Self::Survived,
            Outcome::Unreached => Self::Unreached,
            Outcome::Equivalent => Self::Equivalent,
            Outcome::Unconfirmed => Self::Unconfirmed {
                on: target.to_owned(),
            },
            Outcome::Errored => Self::Errored {
                on: target.to_owned(),
            },
        }
    }

    /// What `outcome` and `decided_by` name together, or nothing where no run could mean the pair.
    ///
    /// # Errors
    /// Nothing, as an `Option`: an outcome no run spells, an outcome that is established against a target with none named, or one that is not with a target named anyway.
    #[must_use]
    pub fn of(
        outcome: Outcome,
        decided_by: Option<String>,
        step_boundary: Option<StepBoundary>,
    ) -> Option<Self> {
        match (outcome, decided_by, step_boundary) {
            (Outcome::Killed, Some(by), None) => Some(Self::Killed { by }),
            (Outcome::StepLimitReached, Some(on), Some(boundary)) => {
                Some(Self::StepLimitReached { on, boundary })
            }
            (Outcome::Waited, Some(on), None) => Some(Self::Waited { on }),
            (Outcome::Unconfirmed, Some(on), None) => Some(Self::Unconfirmed { on }),
            (Outcome::Errored, Some(on), None) => Some(Self::Errored { on }),
            (Outcome::CompileRejected, None, None) => Some(Self::CompileRejected),
            (Outcome::ModelNoticed, None, None) => Some(Self::ModelNoticed),
            (Outcome::ModelProved, None, None) => Some(Self::ModelProved),
            (Outcome::Survived, None, None) => Some(Self::Survived),
            (Outcome::Unreached, None, None) => Some(Self::Unreached),
            (Outcome::Equivalent, None, None) => Some(Self::Equivalent),
            (
                Outcome::Killed
                | Outcome::StepLimitReached
                | Outcome::Waited
                | Outcome::Unconfirmed
                | Outcome::Errored
                | Outcome::CompileRejected
                | Outcome::ModelNoticed
                | Outcome::ModelProved
                | Outcome::Survived
                | Outcome::Unreached
                | Outcome::Equivalent,
                _,
                _,
            ) => None,
        }
    }
}

/// The only valid boundary a verified step-limit notice can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StepBoundary {
    limit: u64,
    observed: u64,
}

impl StepBoundary {
    /// Constructs the exact first count outside a nonzero allowance.
    #[must_use]
    pub fn new(limit: u64, observed: u64) -> Option<Self> {
        if limit != 0 && limit.checked_add(1) == Some(observed) {
            Some(Self { limit, observed })
        } else {
            None
        }
    }

    /// The configured allowance.
    #[must_use]
    pub const fn limit(self) -> u64 {
        self.limit
    }

    /// The first count outside the allowance.
    #[must_use]
    pub const fn observed(self) -> u64 {
        self.observed
    }

    #[cfg(feature = "testkit")]
    const fn specimen() -> Self {
        Self {
            limit: 10,
            observed: 11,
        }
    }
}

impl<'de> Deserialize<'de> for StepBoundary {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            limit: u64,
            observed: u64,
        }

        let Wire { limit, observed } = Wire::deserialize(deserializer)?;
        Self::new(limit, observed).ok_or_else(|| {
            serde::de::Error::custom(
                "a step boundary must be the first count beyond a nonzero allowance",
            )
        })
    }
}

/// The two fields a report has always written, which is what [`Decided`] is carried as.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Paired {
    outcome: Outcome,
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    killed_by: Option<String>,
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    step_boundary: Option<StepBoundary>,
}

impl Serialize for Decided {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Paired {
            outcome: self.outcome(),
            killed_by: self.decided_by().map(ToOwned::to_owned),
            step_boundary: match self {
                Self::StepLimitReached { boundary, .. } => Some(*boundary),
                Self::CompileRejected
                | Self::Killed { .. }
                | Self::ModelNoticed
                | Self::ModelProved
                | Self::Waited { .. }
                | Self::Survived
                | Self::Unreached
                | Self::Equivalent
                | Self::Unconfirmed { .. }
                | Self::Errored { .. } => None,
            },
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Decided {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let held = Paired::deserialize(deserializer)?;
        let said = held.outcome.name();
        Self::of(held.outcome, held.killed_by, held.step_boundary).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "a mutation recorded as {said} is paired with a target it could not have \
                 been established against, or with none where it was"
            ))
        })
    }
}

/// The ways a mutation can be a hole, which is every way short of somebody answering for it.
///
/// A closed set of exactly the decisions that leave a hole.
/// `blind_in` carries this rather than a [`Decision`] because a build that answered is not one anybody is blind in: writing that down is a state a report must never hold,
/// and a type that cannot spell it is a proof where a check would have been a promise somebody keeps.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum Blind {
    /// The tests ran it and nothing noticed.
    Unnoticed,
    /// Nothing ran it.
    Unreached,
    /// The deterministic guard boundary was reached, without a control proof that the mutation caused divergence.
    StepLimitReached,
    /// A bound expired before anything finished.
    Waited,
    /// Nothing could be measured: a harness that would not start, or a pair that did not agree.
    Errored,
}

impl Blind {
    /// The decision this is one of.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn decision(self) -> Decision {
        match self {
            Self::Unnoticed => Decision::Unnoticed,
            Self::Unreached => Decision::Unreached,
            Self::StepLimitReached => Decision::StepLimitReached,
            Self::Waited => Decision::Waited,
            Self::Errored => Decision::Errored,
        }
    }

    /// The wire name a report records.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn name(self) -> &'static str {
        self.decision().name()
    }

    /// Whether nothing answered here, as against the tests having been there and missed it.
    ///
    /// A run that read these two together would count a broken harness among the chances a suite failed to take, and put the count behind the accusation.
    #[must_use]
    pub const fn is_unanswered(self) -> bool {
        match self {
            Self::StepLimitReached | Self::Waited | Self::Errored => true,
            Self::Unnoticed | Self::Unreached => false,
        }
    }
}

/// One build a mutation is a hole in, and what that build established about it.
///
/// The name alone would make a reader believe the same thing happened in every build it lists.
/// A build whose tests ran and noticed nothing wants a test written; a build that established nothing wants somebody to find out why first, and telling them to write a test sends them looking for an assertion that is not what is missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlindIn {
    /// The build, as `[[configuration]]` names it.
    pub build: BuildName,
    /// What that build established, which is one of the ways a mutation is a hole and cannot be anything else.
    pub decision: Blind,
}

/// The part of a catalog one evidence source measured.
///
/// The shard fields are private and can only be constructed after proving `1 <= index <= of`, so an invalid shard cannot enter a completed report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogPart {
    /// The source measured the whole catalog.
    Whole,
    /// The source measured one valid part of a divided catalog.
    Shard(ShardPart),
}

/// A valid `index/of` shard identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShardPart {
    index: std::num::NonZeroU32,
    of: std::num::NonZeroU32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShardPartWire {
    index: u32,
    of: u32,
}

impl Serialize for ShardPart {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ShardPartWire {
            index: self.index(),
            of: self.of(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ShardPart {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ShardPartWire::deserialize(deserializer)?;
        Self::checked(wire.index, wire.of).map_err(serde::de::Error::custom)
    }
}

impl ShardPart {
    /// Validates one shard identity.
    ///
    /// # Errors
    /// Returns the exact zero or range invariant violated by `index` and `of`.
    pub fn checked(index: u32, of: u32) -> Result<Self, PartLedgerError> {
        let index =
            std::num::NonZeroU32::new(index).ok_or(PartLedgerError::InvalidShard { index, of })?;
        let of = std::num::NonZeroU32::new(of).ok_or_else(|| PartLedgerError::InvalidShard {
            index: index.get(),
            of,
        })?;
        if index > of {
            return Err(PartLedgerError::InvalidShard {
                index: index.get(),
                of: of.get(),
            });
        }
        Ok(Self { index, of })
    }

    /// The one-based part index.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index.get()
    }

    /// The total number of parts.
    #[must_use]
    pub const fn of(self) -> u32 {
        self.of.get()
    }

    /// Whether the canonical catalog position belongs to this part.
    #[must_use]
    pub const fn holds(self, index: CatalogIndex) -> bool {
        match index.get().checked_rem(self.of.get()) {
            Some(part) => part == self.index.get() - 1,
            None => false,
        }
    }
}

impl CatalogPart {
    pub(crate) fn parse(shard: Option<&str>) -> Result<Self, PartLedgerError> {
        let Some(shard) = shard else {
            return Ok(Self::Whole);
        };
        let parsed = rust_mutants::run::Shard::parse(shard).map_err(|_invalid| {
            PartLedgerError::MalformedShard {
                shard: shard.to_owned(),
            }
        })?;
        Ok(Self::Shard(ShardPart::checked(parsed.index, parsed.of)?))
    }

    pub(crate) fn shard(self) -> Option<String> {
        match self {
            Self::Whole => None,
            Self::Shard(shard) => Some(format!("{}/{}", shard.index(), shard.of())),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum CatalogPartWire {
    Whole,
    Shard { index: u32, of: u32 },
}

impl Serialize for CatalogPart {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Whole => CatalogPartWire::Whole,
            Self::Shard(shard) => CatalogPartWire::Shard {
                index: shard.index(),
                of: shard.of(),
            },
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CatalogPart {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match CatalogPartWire::deserialize(deserializer)? {
            CatalogPartWire::Whole => Ok(Self::Whole),
            CatalogPartWire::Shard { index, of } => ShardPart::checked(index, of)
                .map(Self::Shard)
                .map_err(serde::de::Error::custom),
        }
    }
}

/// Every fact established by one source run for one configured build and one whole catalog or shard.
///
/// Containment retains the namespace that owns every artifact and prevents shard merge from relabelling foreign evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildPartEvidence {
    /// The canonical run namespace that owns these facts.
    run_id: rust_mutants::id::RunId,
    /// The exact part this namespace measured.
    part: CatalogPart,
    /// What compiled and ran this source.
    toolchain: Toolchain,
    /// When this source ran.
    timing: Timing,
    /// Every count derived from this source's records.
    accounting: Accounting,
    /// Every integration resource this source started.
    resources: Vec<ResourceRecord>,
    /// Every repair candidate this source established.
    candidates: Vec<CandidateRecord>,
    /// Every seam question this source put.
    seams: Vec<SeamRecord>,
    /// Every site of this source's part a fault was asked at.
    faults: Vec<faults::FaultRecord>,
    /// Every survivor of this source's part a target told apart only under a fault.
    beside: Vec<faults::BesideRecord>,
    /// The baseline target facts, in canonical target order.
    targets: Vec<TargetRecord>,
    /// The SHA-256 of each file this source's mutants were read from, as it read them.
    #[serde(serialize_with = "sources_wire::serialize")]
    sources: BTreeMap<String, rust_mutants::id::HexDigest>,
    /// Every mutation this source judged.
    mutants: Vec<MutantRecord>,
    /// Every actionable fact this source raised.
    findings: Vec<Finding>,
    /// Everything this source does not claim.
    limitations: Vec<Limitation>,
    /// Whether each target this source's baseline measured held its reach on a control.
    drift: Vec<drift::Drift>,
}

impl BuildPartEvidence {
    /// Closes one mutable build draft into source-owned evidence and proves every row/count/part relation before it can enter a ledger.
    pub(crate) fn from_report(
        report: &BuildReport,
        run_id: rust_mutants::id::RunId,
        part: CatalogPart,
        findings: Vec<Finding>,
    ) -> Result<Self, PartLedgerError> {
        let evidence = Self {
            run_id,
            part,
            toolchain: report.toolchain.clone(),
            timing: report.timing.clone(),
            accounting: report.accounting,
            resources: report.resources.clone(),
            candidates: report.candidates.clone(),
            seams: report.seams.clone(),
            faults: report.faults.clone(),
            beside: report.beside.clone(),
            targets: report.targets.clone(),
            sources: report.sources.clone(),
            mutants: report.mutants.clone(),
            findings,
            limitations: report.limitations.clone(),
            drift: report.drift.clone(),
        };
        validate_part_evidence(&evidence)?;
        Ok(evidence)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildPartEvidenceWire {
    run_id: rust_mutants::id::RunId,
    part: CatalogPart,
    toolchain: Toolchain,
    timing: Timing,
    accounting: Accounting,
    resources: Vec<ResourceRecord>,
    candidates: Vec<CandidateRecord>,
    seams: Vec<SeamRecord>,
    faults: Vec<faults::FaultRecord>,
    beside: Vec<faults::BesideRecord>,
    targets: Vec<TargetRecord>,
    #[serde(deserialize_with = "sources_wire::deserialize")]
    sources: BTreeMap<String, rust_mutants::id::HexDigest>,
    mutants: Vec<MutantRecord>,
    findings: Vec<Finding>,
    limitations: Vec<Limitation>,
    drift: Vec<drift::Drift>,
}

impl<'de> Deserialize<'de> for BuildPartEvidence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = BuildPartEvidenceWire::deserialize(deserializer)?;
        let held = Self {
            run_id: wire.run_id,
            part: wire.part,
            toolchain: wire.toolchain,
            timing: wire.timing,
            accounting: wire.accounting,
            resources: wire.resources,
            candidates: wire.candidates,
            seams: wire.seams,
            faults: wire.faults,
            beside: wire.beside,
            targets: wire.targets,
            sources: wire.sources,
            mutants: wire.mutants,
            findings: wire.findings,
            limitations: wire.limitations,
            drift: wire.drift,
        };
        validate_part_evidence(&held).map_err(serde::de::Error::custom)?;
        Ok(held)
    }
}

/// The wire spelling of a part's source digests: one closed object per file in path order, which a reader holds to one entry for each path.
mod sources_wire {
    use std::collections::BTreeMap;

    use rust_mutants::id::HexDigest;
    use serde::{Deserialize, Serialize};

    /// One file, as the document writes it.
    #[derive(Serialize)]
    struct Written<'a> {
        path: &'a str,
        digest: &'a HexDigest,
    }

    /// One file, as the document is read.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Read {
        path: String,
        digest: HexDigest,
    }

    /// Writes each file's digest as one object, in path order.
    pub(super) fn serialize<S: serde::Serializer>(
        sources: &BTreeMap<String, HexDigest>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        sources
            .iter()
            .map(|(path, digest)| Written { path, digest })
            .collect::<Vec<Written<'_>>>()
            .serialize(serializer)
    }

    /// Reads each file's digest, refusing a path out of order or written twice, so the one spelling a document has is the map's.
    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<String, HexDigest>, D::Error> {
        let mut sources: BTreeMap<String, HexDigest> = BTreeMap::new();
        for Read { path, digest } in Vec::<Read>::deserialize(deserializer)? {
            if sources
                .last_key_value()
                .is_some_and(|(before, _digest)| *before >= path)
            {
                return Err(serde::de::Error::custom(format!(
                    "the source {path:?} is repeated or out of path order"
                )));
            }
            sources.insert(path, digest);
        }
        Ok(sources)
    }
}

/// A complete part set: exactly one whole-catalog source, or every shard of one denominator in index order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartLedger {
    first: BuildPartEvidence,
    rest: Vec<BuildPartEvidence>,
    division: PartDivision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartDivision {
    Whole,
    Sharded(std::num::NonZeroU32),
}

/// A structural refusal while constructing a complete part set.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PartLedgerError {
    /// No evidence source was retained.
    #[error("a configured build must retain at least one evidence source")]
    Empty,
    /// Whole and sharded evidence were mixed, or more than one whole source was offered.
    #[error("whole-catalog and sharded evidence cannot be mixed or repeated")]
    Mixed,
    /// A shard identity was malformed.
    #[error("{shard:?} is not a canonical shard identity")]
    MalformedShard {
        /// The malformed spelling.
        shard: String,
    },
    /// A shard identity was outside its valid range.
    #[error("shard {index}/{of} is outside 1..={of}")]
    InvalidShard {
        /// The invalid one-based index.
        index: u32,
        /// The invalid denominator.
        of: u32,
    },
    /// Shards named different denominators.
    #[error("shard {index}/{actual} disagrees with denominator {expected}")]
    Denominator {
        /// The shard whose denominator differed.
        index: u32,
        /// The denominator established by the first shard.
        expected: u32,
        /// The denominator this shard claimed.
        actual: u32,
    },
    /// One shard occurred more than once.
    #[error("shard {index}/{of} occurs more than once")]
    DuplicateShard {
        /// The repeated one-based shard.
        index: u32,
        /// The common denominator.
        of: u32,
    },
    /// One shard was absent from an otherwise coherent division.
    #[error("shard {index}/{of} is missing")]
    MissingShard {
        /// The absent one-based shard.
        index: u32,
        /// The common denominator.
        of: u32,
    },
    /// Two evidence sources claimed one run namespace.
    #[error("run namespace {run_id} occurs more than once in one configured build")]
    DuplicateRun {
        /// The repeated canonical namespace.
        run_id: rust_mutants::id::RunId,
    },
    /// Rows inside one source were not in strictly increasing catalog order.
    #[error(
        "source {run_id} has catalog index {actual} after {previous}; rows must retain exact catalog order"
    )]
    CatalogOutOfOrder {
        /// The owning source.
        run_id: rust_mutants::id::RunId,
        /// The preceding index.
        previous: u32,
        /// The later index.
        actual: u32,
    },
    /// A row was placed in a shard that does not own its canonical index.
    #[error("source {run_id} puts catalog index {index} in shard {shard}/{of}")]
    CatalogOutsideShard {
        /// The owning source.
        run_id: rust_mutants::id::RunId,
        /// The misplaced index.
        index: u32,
        /// The claimed shard.
        shard: u32,
        /// Its denominator.
        of: u32,
    },
    /// Two rows in a completed part set claimed one canonical index.
    #[error("catalog index {index} occurs more than once in the completed part set")]
    DuplicateCatalogIndex {
        /// The repeated dense position.
        index: u32,
    },
    /// Two rows in a completed part set claimed one identity.
    #[error("mutation identity {mutant} occurs more than once in the completed part set")]
    DuplicateMutant {
        /// The repeated full identity.
        mutant: String,
    },
    /// The union of a complete part set was not the dense canonical catalog.
    #[error("the completed catalog expected index {expected}, found {actual:?}")]
    CatalogNotDense {
        /// The next dense position.
        expected: u32,
        /// The position found, or no row where one was required.
        actual: Option<u32>,
    },
    /// A row-derived counter could not fit the report's fixed-width wire.
    #[error("source {run_id} has more than u32::MAX {about}")]
    CountOverflow {
        /// The owning source.
        run_id: rust_mutants::id::RunId,
        /// What was being counted.
        about: &'static str,
    },
    /// Evidence under a fault names a survivor or a fault the part does not hold.
    #[error(
        "source {run_id} holds evidence under a fault about {mutant} beside {fault}, which are not a survivor and a fault of it"
    )]
    BesideUnheld {
        /// The source.
        run_id: rust_mutants::id::RunId,
        /// The survivor it names.
        mutant: String,
        /// The fault it names.
        fault: String,
    },
    /// Stored accounting disagreed with the retained rows.
    #[error("source {run_id} has {about} accounting that disagrees with its rows")]
    AccountingMismatch {
        /// The owning source.
        run_id: rust_mutants::id::RunId,
        /// Which accounting section differed.
        about: &'static str,
    },
    /// A full mutation identity was not canonical.
    #[error("source {run_id} carries noncanonical mutation identity {mutant:?}")]
    InvalidMutantIdentity {
        /// The owning source.
        run_id: rust_mutants::id::RunId,
        /// The invalid identity.
        mutant: String,
    },
    /// A display identity was invalid or did not derive from its full identity.
    #[error(
        "source {run_id} pairs mutation {mutant} with noncanonical display identity {display:?}"
    )]
    InvalidDisplayIdentity {
        /// The owning source.
        run_id: rust_mutants::id::RunId,
        /// The full identity.
        mutant: String,
        /// The invalid short identity.
        display: String,
    },
    /// Target rows were duplicated or not in their canonical order.
    #[error("source {run_id} has target rows that are duplicate or out of canonical order")]
    TargetOrder {
        /// The owning source.
        run_id: rust_mutants::id::RunId,
    },
    /// Baseline evidence differed across shards.
    #[error("shard {shard} disagrees with shard 1 about {about}")]
    BaselineMismatch {
        /// The disagreeing one-based shard.
        shard: u32,
        /// Which baseline fact differed.
        about: &'static str,
    },
    /// A source timestamp was absent, malformed, or not in the producer's canonical RFC 3339 spelling.
    #[error("source {run_id} has noncanonical {field} timestamp {value:?}")]
    InvalidTimestamp {
        /// The owning source.
        run_id: rust_mutants::id::RunId,
        /// `started` or `finished`.
        field: &'static str,
        /// The rejected wire spelling.
        value: String,
    },
    /// A source claimed to finish before it started.
    #[error("source {run_id} finished before it started")]
    ReversedTiming {
        /// The owning source.
        run_id: rust_mutants::id::RunId,
    },
}

impl PartLedger {
    /// Validates and retains a whole source or one complete shard set.
    ///
    /// # Errors
    /// Returns the first emptiness, shard topology, namespace, catalog, or accounting invariant violated by `parts`.
    pub fn checked(parts: Vec<BuildPartEvidence>) -> Result<Self, PartLedgerError> {
        let Some(first) = parts.first() else {
            return Err(PartLedgerError::Empty);
        };
        match first.part {
            CatalogPart::Whole if parts.len() == 1 => {
                validate_complete_catalog(&parts)?;
                let mut parts = parts;
                let Some(first) = parts.pop() else {
                    return Err(PartLedgerError::Empty);
                };
                Ok(Self {
                    first,
                    rest: Vec::new(),
                    division: PartDivision::Whole,
                })
            }
            CatalogPart::Whole => Err(PartLedgerError::Mixed),
            CatalogPart::Shard(first_shard) => {
                let of = first_shard.of();
                let mut runs = BTreeSet::new();
                let mut by_index = BTreeMap::new();
                for part in parts {
                    let CatalogPart::Shard(shard) = part.part else {
                        return Err(PartLedgerError::Mixed);
                    };
                    if shard.of() != of {
                        return Err(PartLedgerError::Denominator {
                            index: shard.index(),
                            expected: of,
                            actual: shard.of(),
                        });
                    }
                    if !runs.insert(part.run_id.clone()) {
                        return Err(PartLedgerError::DuplicateRun {
                            run_id: part.run_id,
                        });
                    }
                    validate_part_catalog(&part)?;
                    if by_index.insert(shard.index(), part).is_some() {
                        return Err(PartLedgerError::DuplicateShard {
                            index: shard.index(),
                            of,
                        });
                    }
                }
                for index in 1..=of {
                    if !by_index.contains_key(&index) {
                        return Err(PartLedgerError::MissingShard { index, of });
                    }
                }
                let parts: Vec<BuildPartEvidence> = by_index.into_values().collect();
                validate_complete_catalog(&parts)?;
                let mut parts = parts.into_iter();
                let Some(first) = parts.next() else {
                    return Err(PartLedgerError::MissingShard { index: 1, of });
                };
                let rest: Vec<_> = parts.collect();
                for part in &rest {
                    let CatalogPart::Shard(shard) = part.part else {
                        return Err(PartLedgerError::Mixed);
                    };
                    baseline_matches(&first, part, shard.index())?;
                }
                Ok(Self {
                    first,
                    rest,
                    division: PartDivision::Sharded(first_shard.of),
                })
            }
        }
    }

    /// Every retained evidence source in whole-or-shard order.
    pub fn iter(&self) -> impl Iterator<Item = &BuildPartEvidence> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }

    /// The source whose baseline facts stand for the constructor-proved equal baseline shared by every shard.
    #[must_use]
    pub const fn baseline(&self) -> &BuildPartEvidence {
        &self.first
    }
}

fn validate_part_catalog(part: &BuildPartEvidence) -> Result<(), PartLedgerError> {
    validate_catalog_positions(
        part,
        &part
            .mutants
            .iter()
            .map(|mutant| mutant.catalog_index)
            .collect::<Vec<_>>(),
    )?;
    validate_catalog_positions(
        part,
        &part
            .faults
            .iter()
            .map(|fault| fault.catalog_index)
            .collect::<Vec<_>>(),
    )
}

/// Holds one catalog's positions to strictly ascending order and to the part's shard, so no position is in two parts.
fn validate_catalog_positions(
    part: &BuildPartEvidence,
    positions: &[CatalogIndex],
) -> Result<(), PartLedgerError> {
    for pair in positions.windows(2) {
        let [previous, actual] = pair else {
            continue;
        };
        if previous >= actual {
            return Err(PartLedgerError::CatalogOutOfOrder {
                run_id: part.run_id.clone(),
                previous: previous.get(),
                actual: actual.get(),
            });
        }
    }
    if let CatalogPart::Shard(shard) = part.part {
        for index in positions {
            if !shard.holds(*index) {
                return Err(PartLedgerError::CatalogOutsideShard {
                    run_id: part.run_id.clone(),
                    index: index.get(),
                    shard: shard.index(),
                    of: shard.of(),
                });
            }
        }
    }
    Ok(())
}

/// Holds every record of evidence under a fault to a survivor the part holds, and, where the part is the whole catalog, to a fault it holds: a shard owns its survivors by their index, and the fault beside one may be another shard's.
fn validate_beside(part: &BuildPartEvidence) -> Result<(), PartLedgerError> {
    for beside in &part.beside {
        let survivor = part.mutants.iter().any(|mutant| {
            mutant.display_id == beside.mutant && mutant.outcome.outcome() == Outcome::Survived
        });
        let fault = match part.part {
            CatalogPart::Whole => part.faults.iter().any(|one| one.display_id == beside.fault),
            CatalogPart::Shard(_) => true,
        };
        if !survivor || !fault {
            return Err(PartLedgerError::BesideUnheld {
                run_id: part.run_id.clone(),
                mutant: beside.mutant.clone(),
                fault: beside.fault.clone(),
            });
        }
    }
    Ok(())
}

fn validate_part_evidence(part: &BuildPartEvidence) -> Result<(), PartLedgerError> {
    validate_part_catalog(part)?;
    let started = canonical_timestamp(part, "started", &part.timing.started)?;
    let finished = canonical_timestamp(part, "finished", &part.timing.finished)?;
    if finished < started {
        return Err(PartLedgerError::ReversedTiming {
            run_id: part.run_id.clone(),
        });
    }
    let targets = exact_target_accounting(part)?;
    if targets != part.accounting.targets {
        return Err(PartLedgerError::AccountingMismatch {
            run_id: part.run_id.clone(),
            about: "target",
        });
    }
    let mutants = exact_mutant_accounting(part)?;
    if mutants != part.accounting.mutants {
        return Err(PartLedgerError::AccountingMismatch {
            run_id: part.run_id.clone(),
            about: "mutation",
        });
    }
    validate_beside(part)?;
    let faults = faults::FaultAccounting::of(&part.faults).map_err(|_too_wide| {
        PartLedgerError::CountOverflow {
            run_id: part.run_id.clone(),
            about: "fault sites",
        }
    })?;
    if faults != part.accounting.faults || !faults.adds_up() {
        return Err(PartLedgerError::AccountingMismatch {
            run_id: part.run_id.clone(),
            about: "fault",
        });
    }
    let mut target_ids = BTreeSet::new();
    for target in &part.targets {
        if !target_ids.insert(target.id.as_str()) {
            return Err(PartLedgerError::TargetOrder {
                run_id: part.run_id.clone(),
            });
        }
    }
    for pair in part.targets.windows(2) {
        let [before, after] = pair else {
            continue;
        };
        if before.duration_ms < after.duration_ms
            || (before.duration_ms == after.duration_ms && before.id > after.id)
        {
            return Err(PartLedgerError::TargetOrder {
                run_id: part.run_id.clone(),
            });
        }
    }
    for mutant in &part.mutants {
        let full =
            rust_mutants::id::MutantId::try_from(mutant.id.as_str()).map_err(|_invalid| {
                PartLedgerError::InvalidMutantIdentity {
                    run_id: part.run_id.clone(),
                    mutant: mutant.id.clone(),
                }
            })?;
        let display = rust_mutants::id::DisplayId::try_from(mutant.display_id.as_str()).map_err(
            |_invalid| PartLedgerError::InvalidDisplayIdentity {
                run_id: part.run_id.clone(),
                mutant: mutant.id.clone(),
                display: mutant.display_id.clone(),
            },
        )?;
        if !display.belongs_to(&full) {
            return Err(PartLedgerError::InvalidDisplayIdentity {
                run_id: part.run_id.clone(),
                mutant: mutant.id.clone(),
                display: mutant.display_id.clone(),
            });
        }
    }
    Ok(())
}

fn canonical_timestamp(
    part: &BuildPartEvidence,
    field: &'static str,
    value: &str,
) -> Result<jiff::Timestamp, PartLedgerError> {
    let parsed =
        value
            .parse::<jiff::Timestamp>()
            .map_err(|_invalid| PartLedgerError::InvalidTimestamp {
                run_id: part.run_id.clone(),
                field,
                value: value.to_owned(),
            })?;
    if parsed.to_string() != value {
        return Err(PartLedgerError::InvalidTimestamp {
            run_id: part.run_id.clone(),
            field,
            value: value.to_owned(),
        });
    }
    Ok(parsed)
}

fn exact_target_accounting(part: &BuildPartEvidence) -> Result<TargetAccounting, PartLedgerError> {
    Ok(TargetAccounting {
        selected: exact_count(part, "selected targets", part.targets.len())?,
        passed: exact_count(
            part,
            "passed targets",
            part.targets
                .iter()
                .filter(|target| target.status == TargetStatus::Passed)
                .count(),
        )?,
        failed: exact_count(
            part,
            "failed targets",
            part.targets
                .iter()
                .filter(|target| target.status == TargetStatus::Failed)
                .count(),
        )?,
        skipped: exact_count(
            part,
            "skipped targets",
            part.targets
                .iter()
                .filter(|target| target.status == TargetStatus::Skipped)
                .count(),
        )?,
        missing: exact_count(
            part,
            "missing targets",
            part.targets
                .iter()
                .filter(|target| target.status == TargetStatus::Missing)
                .count(),
        )?,
    })
}

fn exact_mutant_accounting(part: &BuildPartEvidence) -> Result<MutantAccounting, PartLedgerError> {
    Ok(MutantAccounting {
        cataloged: exact_count(part, "catalogued mutations", part.mutants.len())?,
        rejected: count_outcome(part, Outcome::CompileRejected, "compile-rejected mutations")?,
        executed: exact_count(part, "executed mutations", executed_mutants(part))?,
        killed: count_outcome(part, Outcome::Killed, "killed mutations")?,
        survived: count_outcome(part, Outcome::Survived, "surviving mutations")?,
        step_limit_reached: count_outcome(
            part,
            Outcome::StepLimitReached,
            "step-limited mutations",
        )?,
        waited: count_outcome(part, Outcome::Waited, "waited mutations")?,
        unreached: count_outcome(part, Outcome::Unreached, "unreached mutations")?,
        model_noticed: count_outcome(part, Outcome::ModelNoticed, "model-noticed mutations")?,
        model_proved: count_outcome(part, Outcome::ModelProved, "model-proved mutations")?,
        equivalent: count_outcome(part, Outcome::Equivalent, "equivalent mutations")?,
        accepted: exact_count(
            part,
            "accepted mutations",
            part.mutants.iter().filter(|mutant| mutant.accepted).count(),
        )?,
        reused_killed: exact_count(
            part,
            "reused kills",
            part.mutants
                .iter()
                .filter(|mutant| {
                    mutant.outcome.outcome() == Outcome::Killed
                        && mutant.reuse.0.read_back().is_some()
                })
                .count(),
        )?,
        reused_survived: exact_count(
            part,
            "reused survivors",
            part.mutants
                .iter()
                .filter(|mutant| {
                    mutant.outcome.outcome() == Outcome::Survived
                        && mutant.reuse.0.read_back().is_some()
                })
                .count(),
        )?,
        observers: ObserverAccounting {
            types: count_decision(part, Decision::Types, "type decisions")?,
            tests: count_decision(part, Decision::Tests, "test decisions")?,
            model_noticed: count_decision(part, Decision::ModelNoticed, "model-noticed decisions")?,
            model_proved: count_decision(part, Decision::ModelProved, "model-proved decisions")?,
            step_limit_reached: count_decision(
                part,
                Decision::StepLimitReached,
                "step-limit decisions",
            )?,
            proved: count_decision(part, Decision::Proved, "proof decisions")?,
            unnoticed: count_decision(part, Decision::Unnoticed, "unnoticed decisions")?,
            unreached: count_decision(part, Decision::Unreached, "unreached decisions")?,
            waited: count_decision(part, Decision::Waited, "waited decisions")?,
            errored: count_decision(part, Decision::Errored, "errored decisions")?,
        },
    })
}

fn count_outcome(
    part: &BuildPartEvidence,
    outcome: Outcome,
    about: &'static str,
) -> Result<u32, PartLedgerError> {
    exact_count(
        part,
        about,
        part.mutants
            .iter()
            .filter(|mutant| mutant.outcome.outcome() == outcome)
            .count(),
    )
}

fn count_decision(
    part: &BuildPartEvidence,
    decision: Decision,
    about: &'static str,
) -> Result<u32, PartLedgerError> {
    exact_count(
        part,
        about,
        part.mutants
            .iter()
            .filter(|mutant| mutant.outcome.decision() == decision)
            .count(),
    )
}

fn executed_mutants(part: &BuildPartEvidence) -> usize {
    part.mutants
        .iter()
        .filter(|mutant| {
            matches!(
                mutant.outcome.outcome(),
                Outcome::Killed
                    | Outcome::Survived
                    | Outcome::StepLimitReached
                    | Outcome::Waited
                    | Outcome::ModelNoticed
                    | Outcome::ModelProved
                    | Outcome::Unconfirmed
                    | Outcome::Errored
            )
        })
        .count()
}

fn exact_count(
    part: &BuildPartEvidence,
    about: &'static str,
    count: usize,
) -> Result<u32, PartLedgerError> {
    u32::try_from(count).map_err(|_too_wide| PartLedgerError::CountOverflow {
        run_id: part.run_id.clone(),
        about,
    })
}

fn validate_complete_catalog(parts: &[BuildPartEvidence]) -> Result<(), PartLedgerError> {
    let mut by_index = BTreeMap::new();
    let mut identities = BTreeSet::new();
    for part in parts {
        validate_part_evidence(part)?;
        for mutant in &part.mutants {
            if by_index
                .insert(mutant.catalog_index, mutant.id.as_str())
                .is_some()
            {
                return Err(PartLedgerError::DuplicateCatalogIndex {
                    index: mutant.catalog_index.get(),
                });
            }
            if !identities.insert(mutant.id.as_str()) {
                return Err(PartLedgerError::DuplicateMutant {
                    mutant: mutant.id.clone(),
                });
            }
        }
    }
    for (position, index) in by_index.keys().copied().enumerate() {
        let expected =
            u32::try_from(position).map_err(|_too_wide| PartLedgerError::CatalogNotDense {
                expected: u32::MAX,
                actual: Some(index.get()),
            })?;
        if index.get() != expected {
            return Err(PartLedgerError::CatalogNotDense {
                expected,
                actual: Some(index.get()),
            });
        }
    }
    Ok(())
}

fn baseline_matches(
    first: &BuildPartEvidence,
    part: &BuildPartEvidence,
    shard: u32,
) -> Result<(), PartLedgerError> {
    fn verdict(rows: &[TargetRecord]) -> Vec<(&str, &str, &str, &str)> {
        let mut spelled: Vec<(&str, &str, &str, &str)> = rows
            .iter()
            .map(|row| {
                (
                    row.id.as_str(),
                    row.package.as_str(),
                    row.name.as_str(),
                    match row.status {
                        TargetStatus::Passed => "passed",
                        TargetStatus::Failed => "failed",
                        TargetStatus::Skipped => "skipped",
                        TargetStatus::Missing => "missing",
                    },
                )
            })
            .collect();
        spelled.sort_unstable();
        spelled
    }
    for (about, agrees) in [
        ("the toolchain", first.toolchain == part.toolchain),
        (
            "target rows",
            verdict(&first.targets) == verdict(&part.targets),
        ),
        (
            "target accounting",
            first.accounting.targets == part.accounting.targets,
        ),
        (
            "soundness accounting",
            first.accounting.soundness == part.accounting.soundness,
        ),
        ("resources", first.resources == part.resources),
        ("seam evidence", first.seams == part.seams),
    ] {
        if !agrees {
            return Err(PartLedgerError::BaselineMismatch { shard, about });
        }
    }
    Ok(())
}

impl Serialize for PartLedger {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self.iter())
    }
}

impl<'de> Deserialize<'de> for PartLedger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let parts = Vec::<BuildPartEvidence>::deserialize(deserializer)?;
        Self::checked(parts).map_err(serde::de::Error::custom)
    }
}

/// Every source of truth for one configured build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildEvidence {
    /// The configuration name (`default` for the primary build).
    name: BuildName,
    /// The exact Cargo inputs for this build.
    configuration: BuildSelection,
    /// The complete evidence-source ledger.
    parts: PartLedger,
}

impl BuildEvidence {
    /// What this build's controls established about each target's baseline reach, over every part, one record each.
    #[must_use]
    pub fn drift(&self) -> Vec<drift::Drift> {
        drift::combined(self.parts.iter().flat_map(|part| part.drift.iter()))
    }

    /// Joins already proved typed components without exposing mutable fields.
    pub(crate) const fn from_parts(
        name: BuildName,
        configuration: BuildSelection,
        parts: PartLedger,
    ) -> Self {
        Self {
            name,
            configuration,
            parts,
        }
    }

    /// The configured build name.
    #[must_use]
    pub const fn name(&self) -> &BuildName {
        &self.name
    }

    /// The constructor-proved common baseline source.
    #[must_use]
    pub const fn baseline(&self) -> &BuildPartEvidence {
        self.parts.baseline()
    }
}

/// One question a seam's recording licensed, and what the run made of it.
///
/// A `wire-unnoticed` finding names a question by its identity, and a reader who cannot look that identity up has been handed a name and no way to know what it stands for.
/// This is what they look it up in: ADR 0002 keeps a finding off the recording, so what the finding rests on has to be in the report.
///
/// The decision is a nested closed object, so direct deserialization and the published schema both reject extra or ambiguous fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeamRecord {
    /// The question's identity, which is what a finding names.
    pub id: String,
    /// The capability the seam serves, as `[resources.<name>]` names it.
    pub capability: String,
    /// Which exchange on that seam, from zero.
    pub seq: u64,
    /// What the caller asked, where the wire says how to read one.
    /// Empty where it does not.
    pub asked: String,
    /// What the upstream answered, where the wire says how to read one.
    /// `None` where it does not.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub answered: Option<u16>,
    /// What the question asks of the exchange.
    pub rule: crate::wire::rule::Rule,
    /// Who decided it, and — where somebody did — who that was.
    #[serde(rename = "answer")]
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
    /// How many the model checker distinguished with a counterexample.
    pub model_noticed: u32,
    /// How many the model checker proved equal over the closed domain.
    pub model_proved: u32,
    /// How many crossed the configured step boundary without a mutation/control comparison establishing a verdict.
    pub step_limit_reached: u32,
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
    ///
    /// # Errors
    /// Returns [`CountError`] if the selected closed decision column is full.
    pub fn counted(&mut self, decision: Decision) -> Result<(), CountError> {
        let (field, column) = match decision {
            Decision::Types => ("observer types", &mut self.types),
            Decision::Tests => ("observer tests", &mut self.tests),
            Decision::ModelNoticed => ("observer model-noticed", &mut self.model_noticed),
            Decision::ModelProved => ("observer model-proved", &mut self.model_proved),
            Decision::StepLimitReached => {
                ("observer step-limit-reached", &mut self.step_limit_reached)
            }
            Decision::Proved => ("observer proved", &mut self.proved),
            Decision::Unnoticed => ("observer unnoticed", &mut self.unnoticed),
            Decision::Unreached => ("observer unreached", &mut self.unreached),
            Decision::Waited => ("observer waited", &mut self.waited),
            Decision::Errored => ("observer errored", &mut self.errored),
        };
        increment(field, column)
    }

    /// How many mutations these columns account for.
    ///
    /// # Errors
    /// Returns [`CountError`] when mutually exclusive columns cannot be represented by the report's total.
    pub fn total(&self) -> Result<u32, CountError> {
        let mut total = 0;
        for decision in Decision::ALL {
            total = add("observer total", total, self.of(decision))?;
        }
        Ok(total)
    }

    /// The column one decision is counted in.
    ///
    /// Read through here rather than by adding the fields up by hand.
    /// A chain over the struct is told nothing when the set of decisions grows: the eighth column was added and the sum kept six, and only a law caught it — a runtime test standing in for a compile error.
    /// Matched against the closed set, a ninth variant breaks this one function and everything that adds them up follows for free (ADR 0023).
    #[must_use]
    pub const fn of(&self, decision: Decision) -> u32 {
        match decision {
            Decision::Types => self.types,
            Decision::Tests => self.tests,
            Decision::ModelNoticed => self.model_noticed,
            Decision::ModelProved => self.model_proved,
            Decision::StepLimitReached => self.step_limit_reached,
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
    /// The sites a fault was asked at.
    pub faults: faults::FaultAccounting,
}

/// What became of one target.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
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
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub message: Option<String>,
}

/// A canonical lowercase SHA-256 value used by model evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ModelDigest(String);

/// A closed construction failure for the compiler-checked model evidence graph.
/// These are programmer/protocol invariants, never user prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ModelInvariant {
    #[error("the pristine source digest is not canonical")]
    SourceDigest,
    #[error("the rendered source digest is not canonical")]
    RenderedDigest,
    #[error("the isolated proof crate digest is not canonical")]
    CrateDigest,
    #[error("the isolated proof crate identity differs from the fixed verified-v1 inputs")]
    CrateInput,
    #[error("the mutation digest is not canonical")]
    MutantDigest,
    #[error("the model path is not canonical workspace-relative UTF-8")]
    Path,
    #[error("the mutation rule version is zero")]
    RuleVersion,
    #[error("the original mutation bytes are not canonical lowercase hexadecimal")]
    OriginalHex,
    #[error("the replacement mutation bytes are not canonical lowercase hexadecimal")]
    ReplacementHex,
    #[error("the mutation span is invalid or disagrees with its original bytes")]
    Span,
    #[error("the model identity fields contradict one another")]
    Identity,
    #[error("the mutation digest does not bind the recorded identity fields")]
    MutantBinding,
    #[error("the artifact path is not canonical run-relative UTF-8")]
    ArtifactPath,
    #[error("a complete artifact has zero bytes")]
    ArtifactBytes,
    #[error("the artifact digest is not canonical")]
    ArtifactDigest,
    #[error("the artifact paths do not bind the mutation identity and artifact kind")]
    ArtifactBinding,
    #[error("the verifier identity differs from the pinned backend")]
    Verifier,
    #[error("the retained generated source does not match the rendered digest")]
    SourceBinding,
    #[error("the raw attempt digest is not canonical")]
    RawDigest,
    #[error("the complete raw artifact and attempt digest disagree")]
    ArtifactRawBinding,
    #[error("an attempt without a raw artifact does not carry the empty digest")]
    AbsentRawBinding,
    #[error("a verifier identity exists without its raw artifact")]
    VerifierArtifact,
    #[error("an other-failure reason has an empty category")]
    OtherFailure,
    #[error("the uncertainty reason and process fact disagree")]
    ReasonProcess,
    #[error("the model record mutation identity is not canonical")]
    RecordMutant,
    #[error("the model record and nested evidence name different mutations")]
    RecordBinding,
}

impl ModelDigest {
    /// Parses the canonical lowercase SHA-256 representation.
    pub(crate) fn parse(value: String) -> Option<Self> {
        (value.len() == 64
            && value
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
        .then_some(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ModelDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).ok_or_else(|| {
            serde::de::Error::custom("a model digest must be 64 lowercase hexadecimal digits")
        })
    }
}

/// A nonempty relative path made only of ordinary components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
struct ModelRelativePath(String);

impl ModelRelativePath {
    fn parse(value: &str) -> Option<Self> {
        match rust_mutants::id::normalize_path(value) {
            Ok(normalized) if normalized == value => Some(Self(normalized)),
            Ok(_unnormalized) => None,
            Err(_unnormalizable) => None,
        }
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// The only retained model-artifact namespace: one file named by the full mutation identity under the run's fresh `model` directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
struct ModelArtifactPath(ModelRelativePath);

impl ModelArtifactPath {
    fn parse(value: &str) -> Option<Self> {
        let relative = ModelRelativePath::parse(value)?;
        let name = relative.as_str().strip_prefix("model/")?;
        if name.contains('/') {
            return None;
        }
        let (stem, extension) = name.rsplit_once('.')?;
        (matches!(extension, "json" | "rs") && ModelDigest::parse(stem.to_owned()).is_some())
            .then_some(Self(relative))
    }

    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl<'de> Deserialize<'de> for ModelArtifactPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| {
            serde::de::Error::custom(
                "a model artifact path must be model/<64-lowercase-hex>.json or .rs",
            )
        })
    }
}

impl<'de> Deserialize<'de> for ModelRelativePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).ok_or_else(|| {
            serde::de::Error::custom(
                "a model path must be a nonempty relative path of ordinary components",
            )
        })
    }
}

/// Canonical lowercase hexadecimal bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
struct ModelHex(String);

impl ModelHex {
    fn parse(value: String) -> Option<Self> {
        (value.len().is_multiple_of(2)
            && value
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
        .then_some(Self(value))
    }

    const fn bytes(&self) -> usize {
        self.0.len() / 2
    }
}

impl<'de> Deserialize<'de> for ModelHex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).ok_or_else(|| {
            serde::de::Error::custom("model bytes must be even-length lowercase hexadecimal")
        })
    }
}

/// The complete identity of the dependency-free crate handed to Kani.
/// Fields are private and deserialization is checked so a report cannot claim a different package, edition, source path, network policy, or lock policy while retaining an affirmative model answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ModelCrateIdentity {
    package: String,
    edition: String,
    source: ModelRelativePath,
    offline: bool,
    dependency_resolution: String,
    environment: String,
    sha256: ModelDigest,
}

impl ModelCrateIdentity {
    fn checked(sha256: String) -> Result<Self, ModelInvariant> {
        Ok(Self {
            package: crate::assure::model::MODEL_PACKAGE.to_owned(),
            edition: crate::assure::model::MODEL_EDITION.to_owned(),
            source: ModelRelativePath::parse(crate::assure::model::MODEL_SOURCE_PATH)
                .ok_or(ModelInvariant::CrateInput)?,
            offline: true,
            dependency_resolution: crate::assure::model::MODEL_DEPENDENCY_POLICY.to_owned(),
            environment: crate::assure::model::MODEL_ENVIRONMENT_POLICY.to_owned(),
            sha256: ModelDigest::parse(sha256).ok_or(ModelInvariant::CrateDigest)?,
        })
    }

    const fn digest(&self) -> &ModelDigest {
        &self.sha256
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelCrateIdentityWire {
    package: String,
    edition: String,
    source: ModelRelativePath,
    offline: bool,
    dependency_resolution: String,
    environment: String,
    sha256: String,
}

impl<'de> Deserialize<'de> for ModelCrateIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ModelCrateIdentityWire::deserialize(deserializer)?;
        if wire.package != crate::assure::model::MODEL_PACKAGE
            || wire.edition != crate::assure::model::MODEL_EDITION
            || wire.source.as_str() != crate::assure::model::MODEL_SOURCE_PATH
            || !wire.offline
            || wire.dependency_resolution != crate::assure::model::MODEL_DEPENDENCY_POLICY
            || wire.environment != crate::assure::model::MODEL_ENVIRONMENT_POLICY
        {
            return Err(serde::de::Error::custom(ModelInvariant::CrateInput));
        }
        Self::checked(wire.sha256).map_err(serde::de::Error::custom)
    }
}

/// The compiler-checked identity shared by every answer from the model checker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelIdentity {
    /// The exact unqualified generated harness identity.
    harness: String,
    /// The unique assertion description whose property decides the differential question.
    assertion: String,
    /// The nonzero unwind compiled directly onto the proof harness.
    unwind: std::num::NonZeroU32,
    /// The externally supervised process ceiling.
    timeout_ms: std::num::NonZeroU64,
    /// The pristine source bytes admitted by the eligibility gate.
    source_sha256: ModelDigest,
    /// The exact generated Rust source Kani compiled.
    rendered_sha256: ModelDigest,
    /// The fixed manifest, lockfile, source path, and execution policy.
    crate_input: ModelCrateIdentity,
    /// The full mutation identity embedded in the harness and assertion names.
    mutant: ModelDigest,
    /// The workspace-relative path which participates in that mutation identity.
    path: ModelRelativePath,
    /// The canonical mutation operator name.
    rule: String,
    /// The mutation operator version.
    rule_version: std::num::NonZeroU32,
    /// The first byte of the replaced pristine-source span.
    start_byte: u32,
    /// One past the last byte of the replaced pristine-source span.
    end_byte: u32,
    /// Hexadecimal pristine bytes covered by the span.
    original_hex: ModelHex,
    /// Hexadecimal bytes used for the mutant rendering.
    replacement_hex: ModelHex,
}

/// Producer-side ingredients which become evidence only after all relational identity invariants have been checked.
pub(crate) struct ModelIdentityInput {
    pub harness: String,
    pub assertion: String,
    pub unwind: std::num::NonZeroU32,
    pub timeout_ms: std::num::NonZeroU64,
    pub source_sha256: String,
    pub rendered_sha256: String,
    pub crate_sha256: String,
    pub mutant: String,
    pub path: String,
    pub rule: String,
    pub rule_version: u32,
    pub start_byte: u32,
    pub end_byte: u32,
    pub original_hex: String,
    pub replacement_hex: String,
}

impl ModelIdentity {
    pub(crate) fn checked(input: ModelIdentityInput) -> Result<Self, ModelInvariant> {
        let source_sha256 =
            ModelDigest::parse(input.source_sha256).ok_or(ModelInvariant::SourceDigest)?;
        let rendered_sha256 =
            ModelDigest::parse(input.rendered_sha256).ok_or(ModelInvariant::RenderedDigest)?;
        let crate_input = ModelCrateIdentity::checked(input.crate_sha256)?;
        if crate_input.digest().as_str()
            != crate::assure::model::model_crate_digest_from_rendered_digest(
                rendered_sha256.as_str(),
            )
        {
            return Err(ModelInvariant::CrateInput);
        }
        let mutant = ModelDigest::parse(input.mutant).ok_or(ModelInvariant::MutantDigest)?;
        let path = ModelRelativePath::parse(&input.path).ok_or(ModelInvariant::Path)?;
        let rule_version =
            std::num::NonZeroU32::new(input.rule_version).ok_or(ModelInvariant::RuleVersion)?;
        let original_hex =
            ModelHex::parse(input.original_hex).ok_or(ModelInvariant::OriginalHex)?;
        let replacement_hex =
            ModelHex::parse(input.replacement_hex).ok_or(ModelInvariant::ReplacementHex)?;
        let span = input
            .end_byte
            .checked_sub(input.start_byte)
            .ok_or(ModelInvariant::Span)?;
        let span = usize::try_from(span).map_err(|_error| ModelInvariant::Span)?;
        let original =
            hex::decode(&original_hex.0).map_err(|_error| ModelInvariant::OriginalHex)?;
        let replacement =
            hex::decode(&replacement_hex.0).map_err(|_error| ModelInvariant::ReplacementHex)?;
        if input.harness != format!("__njutest_model_{}", mutant.as_str())
            || input.assertion != format!("njutest-model-v1:{}", mutant.as_str())
            || input.rule.is_empty()
            || input.rule.contains(['@', ' ', '\t', '\r', '\n'])
            || original_hex.bytes() != span
            || original == replacement
        {
            return Err(ModelInvariant::Identity);
        }
        let stable = rust_mutants::id::Identity {
            path: path.as_str().to_owned(),
            rule_name: input.rule.clone(),
            rule_version: rule_version.get(),
            span: rust_mutants::span::Span::new(input.start_byte, input.end_byte)
                .map_err(|_error| ModelInvariant::Span)?,
            source_digest: source_sha256.as_str().to_owned(),
            original_digest: rust_mutants::id::digest(&original),
            replacement_digest: rust_mutants::id::digest(&replacement),
        };
        if stable
            .id()
            .map_err(|_error| ModelInvariant::Identity)?
            .as_str()
            != mutant.as_str()
        {
            return Err(ModelInvariant::MutantBinding);
        }
        Ok(Self {
            harness: input.harness,
            assertion: input.assertion,
            unwind: input.unwind,
            timeout_ms: input.timeout_ms,
            source_sha256,
            rendered_sha256,
            crate_input,
            mutant,
            path,
            rule: input.rule,
            rule_version,
            start_byte: input.start_byte,
            end_byte: input.end_byte,
            original_hex,
            replacement_hex,
        })
    }

    const fn mutant(&self) -> &ModelDigest {
        &self.mutant
    }

    const fn rendered_digest(&self) -> &ModelDigest {
        &self.rendered_sha256
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelIdentityWire {
    harness: String,
    assertion: String,
    unwind: std::num::NonZeroU32,
    timeout_ms: std::num::NonZeroU64,
    source_sha256: String,
    rendered_sha256: String,
    crate_input: ModelCrateIdentity,
    mutant: String,
    path: String,
    rule: String,
    rule_version: u32,
    start_byte: u32,
    end_byte: u32,
    original_hex: String,
    replacement_hex: String,
}

impl<'de> Deserialize<'de> for ModelIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ModelIdentityWire::deserialize(deserializer)?;
        Self::checked(ModelIdentityInput {
            harness: wire.harness,
            assertion: wire.assertion,
            unwind: wire.unwind,
            timeout_ms: wire.timeout_ms,
            source_sha256: wire.source_sha256,
            rendered_sha256: wire.rendered_sha256,
            crate_sha256: wire.crate_input.digest().as_str().to_owned(),
            mutant: wire.mutant,
            path: wire.path,
            rule: wire.rule,
            rule_version: wire.rule_version,
            start_byte: wire.start_byte,
            end_byte: wire.end_byte,
            original_hex: wire.original_hex,
            replacement_hex: wire.replacement_hex,
        })
        .map_err(serde::de::Error::custom)
    }
}

/// One complete raw Kani export retained beside the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelArtifact {
    /// The run-relative path a second implementation reads.
    path: ModelArtifactPath,
    /// The exact byte length.
    bytes: std::num::NonZeroU64,
    /// SHA-256 of those exact bytes.
    sha256: ModelDigest,
}

impl ModelArtifact {
    pub(crate) fn checked(path: &str, bytes: u64, sha256: String) -> Result<Self, ModelInvariant> {
        Ok(Self {
            path: ModelArtifactPath::parse(path).ok_or(ModelInvariant::ArtifactPath)?,
            bytes: std::num::NonZeroU64::new(bytes).ok_or(ModelInvariant::ArtifactBytes)?,
            sha256: ModelDigest::parse(sha256).ok_or(ModelInvariant::ArtifactDigest)?,
        })
    }

    pub(crate) fn path(&self) -> &str {
        self.path.as_str()
    }

    pub(crate) const fn bytes(&self) -> u64 {
        self.bytes.get()
    }

    const fn digest(&self) -> &ModelDigest {
        &self.sha256
    }

    pub(crate) fn matches(&self, bytes: &[u8]) -> bool {
        let Ok(length) = u64::try_from(bytes.len()) else {
            return false;
        };
        length == self.bytes.get() && rust_mutants::id::digest(bytes) == self.sha256.as_str()
    }

    fn belongs_to(&self, mutant: &ModelDigest, extension: &str) -> bool {
        self.path.as_str() == format!("model/{}.{}", mutant.as_str(), extension)
    }
}

/// The exact formal-verification backend admitted by the v1 parser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelBackend {
    /// Kani's exported result schema version.
    export_version: String,
    /// Kani's code-generation mode.
    build_mode: String,
    /// The target explicitly shared with the measured Cargo build.
    target: String,
    /// The compiler embedded in Kani; this is intentionally not the measured rustc.
    rustc: String,
    /// The bounded model checker banner.
    cbmc: String,
    /// The goto compiler banner.
    goto_cc: String,
    /// The goto instrumentation banner.
    goto_instrument: String,
    /// The explicitly selected solver.
    solver: String,
}

/// The model checker and backend identity, present or absent as one fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelVerifier {
    /// The exact Kani version named by the exported result.
    tool: String,
    /// The exact compiler, target, model checker, goto tools, and solver.
    backend: ModelBackend,
}

/// Producer-side verifier fields accepted only when they name the exact pin.
pub(crate) struct ModelVerifierInput {
    pub tool: String,
    pub export_version: String,
    pub build_mode: String,
    pub target: String,
    pub rustc: String,
    pub cbmc: String,
    pub goto_cc: String,
    pub goto_instrument: String,
    pub solver: String,
}

impl ModelVerifier {
    pub(crate) fn checked(input: ModelVerifierInput) -> Result<Self, ModelInvariant> {
        if input.tool != crate::assure::model::KANI_VERSION
            || input.export_version != crate::assure::model::KANI_EXPORT_VERSION
            || input.build_mode != crate::assure::model::KANI_BUILD_MODE
            || input.rustc != crate::assure::model::KANI_RUSTC_VERSION
            || input.cbmc != crate::assure::model::KANI_CBMC_VERSION
            || input.goto_cc != crate::assure::model::KANI_GOTO_CC_VERSION
            || input.goto_instrument != crate::assure::model::KANI_GOTO_INSTRUMENT_VERSION
            || input.solver != crate::assure::model::KANI_SOLVER
            || input.target.is_empty()
            || !input
                .target
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(ModelInvariant::Verifier);
        }
        Ok(Self {
            tool: input.tool,
            backend: ModelBackend {
                export_version: input.export_version,
                build_mode: input.build_mode,
                target: input.target,
                rustc: input.rustc,
                cbmc: input.cbmc,
                goto_cc: input.goto_cc,
                goto_instrument: input.goto_instrument,
                solver: input.solver,
            },
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelBackendWire {
    export_version: String,
    build_mode: String,
    target: String,
    rustc: String,
    cbmc: String,
    goto_cc: String,
    goto_instrument: String,
    solver: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelVerifierWire {
    tool: String,
    backend: ModelBackendWire,
}

impl<'de> Deserialize<'de> for ModelVerifier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ModelVerifierWire::deserialize(deserializer)?;
        Self::checked(ModelVerifierInput {
            tool: wire.tool,
            export_version: wire.backend.export_version,
            build_mode: wire.backend.build_mode,
            target: wire.backend.target,
            rustc: wire.backend.rustc,
            cbmc: wire.backend.cbmc,
            goto_cc: wire.backend.goto_cc,
            goto_instrument: wire.backend.goto_instrument,
            solver: wire.backend.solver,
        })
        .map_err(serde::de::Error::custom)
    }
}

/// The supervised process fact retained independently of its property meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    tag = "kind",
    content = "code",
    rename_all = "kebab-case"
)]
pub enum ModelProcess {
    /// No verifier proof process started.
    NotRun,
    /// The verifier exited normally with this code.
    Exited(i32),
    /// The external wall-clock ceiling stopped the process tree.
    Cutoff,
    /// The caller cancelled the process tree.
    Cancelled,
    /// No trustworthy exit code was available.
    Failed,
}

/// An affirmative property answer whose required exit code can be re-derived.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum ModelAffirmative {
    /// Equality requires exit zero.
    Proved,
    /// A tagged counterexample requires exit one.
    Noticed,
}

/// The only process fact a proved answer can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelProvedExit;

/// The only process fact a noticed answer can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelNoticedExit;

impl Serialize for ModelProvedExit {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ModelProcess::Exited(0).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ModelProvedExit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match ModelProcess::deserialize(deserializer)? {
            ModelProcess::Exited(0) => Ok(Self),
            ModelProcess::NotRun
            | ModelProcess::Exited(_)
            | ModelProcess::Cutoff
            | ModelProcess::Cancelled
            | ModelProcess::Failed => Err(serde::de::Error::custom(
                "proved model evidence requires process exit 0",
            )),
        }
    }
}

impl Serialize for ModelNoticedExit {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ModelProcess::Exited(1).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ModelNoticedExit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match ModelProcess::deserialize(deserializer)? {
            ModelProcess::Exited(1) => Ok(Self),
            ModelProcess::NotRun
            | ModelProcess::Exited(_)
            | ModelProcess::Cutoff
            | ModelProcess::Cancelled
            | ModelProcess::Failed => Err(serde::de::Error::custom(
                "noticed model evidence requires process exit 1",
            )),
        }
    }
}

/// Evidence carried only by one typed affirmative Kani decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelEvidence<P> {
    /// The inseparable model checker and backend identity.
    verifier: ModelVerifier,
    /// The question compiled into the harness.
    identity: ModelIdentity,
    /// The complete raw export a separate audit re-parses.
    artifact: ModelArtifact,
    /// The generated Rust bytes independently retained for re-hashing.
    source: ModelArtifact,
    /// The decision-specific normal exit, unrepresentable with another code.
    process: P,
}

/// Producer-side affirmative facts checked before they can inhabit a report.
pub(crate) struct ModelEvidenceInput<P> {
    pub verifier: ModelVerifier,
    pub identity: ModelIdentity,
    pub artifact: ModelArtifact,
    pub source: ModelArtifact,
    pub process: P,
}

impl<P> ModelEvidence<P> {
    pub(crate) fn checked(input: ModelEvidenceInput<P>) -> Result<Self, ModelInvariant> {
        if input.source.digest() != input.identity.rendered_digest() {
            return Err(ModelInvariant::SourceBinding);
        }
        if !input.artifact.belongs_to(input.identity.mutant(), "json")
            || !input.source.belongs_to(input.identity.mutant(), "rs")
        {
            return Err(ModelInvariant::ArtifactBinding);
        }
        Ok(Self {
            verifier: input.verifier,
            identity: input.identity,
            artifact: input.artifact,
            source: input.source,
            process: input.process,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "P: Deserialize<'de>"))]
struct ModelEvidenceWire<P> {
    verifier: ModelVerifier,
    identity: ModelIdentity,
    artifact: ModelArtifact,
    source: ModelArtifact,
    process: P,
}

impl<'de, P> Deserialize<'de> for ModelEvidence<P>
where
    P: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ModelEvidenceWire::<P>::deserialize(deserializer)?;
        Self::checked(ModelEvidenceInput {
            verifier: wire.verifier,
            identity: wire.identity,
            artifact: wire.artifact,
            source: wire.source,
            process: wire.process,
        })
        .map_err(serde::de::Error::custom)
    }
}

/// Evidence from a Kani attempt that established no affirmative answer.
///
/// Its fields are private because the raw artifact, its digest, and the optional verifier are one fact: a complete artifact must carry its own digest, no artifact must carry the digest of the empty sequence, and a verifier identity cannot exist without the export that established it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelAttemptEvidence {
    /// The checker/backend pair, absent unless one complete document established both.
    verifier: Option<ModelVerifier>,
    /// The question the runner attempted.
    identity: ModelIdentity,
    /// A complete export when Kani produced one; never a partial file.
    artifact: Option<ModelArtifact>,
    /// The generated Rust bytes handed to the private workspace.
    source: ModelArtifact,
    /// How the proof process ended, independently of the uncertainty reason.
    process: ModelProcess,
    /// SHA-256 of whatever raw bytes were interpreted, including the empty sequence when none existed.
    raw_sha256: ModelDigest,
}

/// Producer-side attempt facts which become evidence only after their cross-field invariants have been checked.
pub(crate) struct ModelAttemptEvidenceInput {
    pub verifier: Option<ModelVerifier>,
    pub identity: ModelIdentity,
    pub artifact: Option<ModelArtifact>,
    pub source: ModelArtifact,
    pub process: ModelProcess,
    pub raw_sha256: String,
}

impl ModelAttemptEvidence {
    const EMPTY_SHA256: &'static str =
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    pub(crate) fn checked(input: ModelAttemptEvidenceInput) -> Result<Self, ModelInvariant> {
        let raw_sha256 = ModelDigest::parse(input.raw_sha256).ok_or(ModelInvariant::RawDigest)?;
        if input.source.digest() != input.identity.rendered_digest() {
            return Err(ModelInvariant::SourceBinding);
        }
        if !input.source.belongs_to(input.identity.mutant(), "rs")
            || input
                .artifact
                .as_ref()
                .is_some_and(|artifact| !artifact.belongs_to(input.identity.mutant(), "json"))
        {
            return Err(ModelInvariant::ArtifactBinding);
        }
        match input.artifact.as_ref() {
            Some(artifact) if artifact.digest() != &raw_sha256 => {
                return Err(ModelInvariant::ArtifactRawBinding);
            }
            None if raw_sha256.as_str() != Self::EMPTY_SHA256 => {
                return Err(ModelInvariant::AbsentRawBinding);
            }
            Some(_) | None => {}
        }
        if input.verifier.is_some() && input.artifact.is_none() {
            return Err(ModelInvariant::VerifierArtifact);
        }
        Ok(Self {
            verifier: input.verifier,
            identity: input.identity,
            artifact: input.artifact,
            source: input.source,
            process: input.process,
            raw_sha256,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelAttemptEvidenceWire {
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    verifier: Option<ModelVerifier>,
    identity: ModelIdentity,
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    artifact: Option<ModelArtifact>,
    source: ModelArtifact,
    process: ModelProcess,
    raw_sha256: String,
}

impl<'de> Deserialize<'de> for ModelAttemptEvidence {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ModelAttemptEvidenceWire::deserialize(deserializer)?;
        Self::checked(ModelAttemptEvidenceInput {
            verifier: wire.verifier,
            identity: wire.identity,
            artifact: wire.artifact,
            source: wire.source,
            process: wire.process,
            raw_sha256: wire.raw_sha256,
        })
        .map_err(serde::de::Error::custom)
    }
}

/// A signature, source, or expression feature outside the closed proof fragment.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum ModelIneligibility {
    /// The source bytes are not UTF-8 Rust text.
    SourceEncoding,
    /// The source digest differs from the catalogued candidate.
    SourceDigest,
    /// The candidate's own identity fields contradict one another.
    Candidate,
    /// Independent mutation identity derivation disagreed.
    Identity,
    /// The pristine source did not parse.
    SourceSyntax,
    /// The edit is not wholly inside one free-function body.
    EnclosingFunction,
    /// Function modifiers are outside the closed fragment.
    FunctionShape,
    /// The function has no symbolic by-value input.
    NoSymbolicInput,
    /// An argument is not a plain by-value local binding.
    ArgumentPattern,
    /// An input type is outside the closed symbolic domain.
    InputType,
    /// The output type is outside the closed equality domain.
    OutputType,
    /// The original or mutant body has an open effect.
    Effect,
    /// Applying the edit did not produce a parseable function.
    MutantSyntax,
    /// A generated private identifier already occurs in the source.
    NameCollision,
    /// Parser locations could not be mapped back to source bytes.
    SourceSpan,
}

/// A build selection the closed Kani command refuses to reinterpret.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum ModelConfiguration {
    /// The owning Cargo package name is absent.
    Package,
    /// An explicit Cargo target is outside this contract.
    Target,
    /// A custom profile is outside this contract.
    Profile,
    /// Caller or Cargo configuration supplied compiler flags outside the retained proof identity.
    CompilerFlags,
    /// Caller configuration selected or interposed on the compiler named by the pinned Kani evidence.
    CompilerEnvironment,
    /// An evidence path is not absolute at the process boundary.
    RelativePath,
    /// A required private directory is absent.
    Directory,
    /// A measured test changed the subject tree before model verification.
    TreeWritten,
    /// A fresh proof copy differed from, or wrote beyond, its prepared source manifest.
    WorkspaceDrift,
}

/// A failure to establish the pinned Kani executable.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum ModelToolFailure {
    /// The pinned executable could not be started.
    Unavailable,
    /// The version command did not exit successfully.
    VersionCommand,
    /// The exact two-line Kani/CBMC banner differed.
    VersionBanner,
    /// The harness-list command did not exit successfully.
    HarnessListCommand,
    /// The harness-list artifact was absent, stale, or unreadable.
    HarnessListArtifact,
    /// The harness-list artifact did not match the pinned schema.
    HarnessListSchema,
    /// The generated suffix did not identify exactly one full harness name.
    HarnessListMatch,
}

/// A verifier process termination that has no property meaning.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum ModelProcessFailure {
    /// The verifier could not be started.
    NotStarted,
    /// An unowned monitor stopped the verifier.
    Stopped,
    /// The execution monitor could not be inspected safely.
    Monitor,
    /// Waiting for the verifier failed.
    Wait,
    /// A signal ended the verifier.
    Signal,
    /// The operating system exposed no code or signal.
    UnknownExit,
    /// The exit code lies outside Kani's pinned zero/one protocol.
    UnexpectedExit,
}

/// A raw-export failure that prevents independent audit.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum ModelArtifactFailure {
    /// A stale path existed before the invocation.
    AlreadyExists,
    /// Kani exited without writing the required export.
    Missing,
    /// The export path is not a regular file.
    NotFile,
    /// The export exceeded the fixed memory ceiling.
    TooLarge,
    /// The complete export could not be read.
    Unreadable,
    /// The generated Rust changed across the supervised verifier process.
    SourceChanged,
}

/// A broken invariant of the Kani 0.68 export protocol.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum ModelProtocol {
    /// The bytes do not match the closed JSON schema.
    Schema,
    /// Metadata and tools do not both name the pinned Kani version.
    ToolVersion,
    /// The export schema version differs from the pinned one.
    ExportVersion,
    /// The target, compiler, backend, build mode, or solver differs.
    Backend,
    /// Summary totals do not describe one completed harness.
    Summary,
    /// The result does not name exactly the generated harness.
    Harness,
    /// The tagged differential assertion is absent or duplicated.
    Assertion,
    /// Summary, harness, property, and process facts disagree.
    Contradiction,
}

/// Every non-affirmative Kani 0.68 property status.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum ModelPropertyStatus {
    /// A counterexample exists.
    Failure,
    /// A cover property was reached.
    Covered,
    /// A contract property was satisfied.
    Satisfied,
    /// A property was proved.
    Success,
    /// The solver did not determine the property.
    Undetermined,
    /// Kani could not classify the property.
    Unknown,
    /// The property was unreachable.
    Unreachable,
    /// A cover property was not reached.
    Uncovered,
    /// A contract property was unsatisfiable.
    Unsatisfiable,
    /// The checker errored on the property.
    Error,
}

/// Why one attempted model question established neither equality nor difference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    deny_unknown_fields,
    tag = "kind",
    content = "detail",
    rename_all = "kebab-case"
)]
pub enum ModelUncertainty {
    /// A failed unwind property exhausted the configured bound.
    BoundExhausted,
    /// The external wall-clock ceiling expired.
    Cutoff,
    /// The caller cancelled the proof.
    Cancelled,
    /// The measured Cargo selection cannot be represented faithfully.
    Configuration(ModelConfiguration),
    /// The pinned tool boundary could not be established.
    Tool(ModelToolFailure),
    /// The proof process supplied no supported normal exit.
    Process(ModelProcessFailure),
    /// Raw or generated evidence could not be trusted.
    Artifact(ModelArtifactFailure),
    /// The property document and process exit disagree.
    ExitMismatch {
        /// The affirmative property answer encoded by the document.
        expected: ModelAffirmative,
        /// The verifier's actual normal exit code.
        actual: i32,
    },
    /// The export contradicted its pinned protocol.
    Protocol(ModelProtocol),
    /// A non-affirmative property status was returned.
    Property(ModelPropertyStatus),
    /// A property other than the tagged differential assertion failed.
    OtherFailure(String),
}

/// One non-affirmative attempt whose reason and process fact are coupled.
///
/// This is deliberately nested beneath the `undecided` decision.
/// Keeping the reason beside an independently constructible evidence product would admit nonsense such as `cutoff` with a normal exit or `tool/unavailable` after a verifier identity had been established.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelAttempt {
    /// Why the attempt establishes neither equality nor difference.
    reason: ModelUncertainty,
    /// The exact attempt facts supporting that refusal.
    evidence: ModelAttemptEvidence,
}

impl ModelAttempt {
    pub(crate) fn checked(
        reason: ModelUncertainty,
        evidence: ModelAttemptEvidence,
    ) -> Result<Self, ModelInvariant> {
        if matches!(&reason, ModelUncertainty::OtherFailure(category) if category.is_empty()) {
            return Err(ModelInvariant::OtherFailure);
        }
        let process = evidence.process;
        let supported = match &reason {
            ModelUncertainty::BoundExhausted
            | ModelUncertainty::Protocol(_)
            | ModelUncertainty::Property(_)
            | ModelUncertainty::OtherFailure(_) => {
                matches!(process, ModelProcess::Exited(0 | 1))
            }
            ModelUncertainty::Cutoff => process == ModelProcess::Cutoff,
            ModelUncertainty::Cancelled => process == ModelProcess::Cancelled,
            ModelUncertainty::Configuration(ModelConfiguration::WorkspaceDrift)
            | ModelUncertainty::Artifact(ModelArtifactFailure::SourceChanged) => true,
            ModelUncertainty::Configuration(
                ModelConfiguration::Package
                | ModelConfiguration::Target
                | ModelConfiguration::Profile
                | ModelConfiguration::CompilerFlags
                | ModelConfiguration::CompilerEnvironment
                | ModelConfiguration::RelativePath
                | ModelConfiguration::Directory
                | ModelConfiguration::TreeWritten,
            )
            | ModelUncertainty::Tool(_) => process == ModelProcess::NotRun,
            ModelUncertainty::Process(_) => process == ModelProcess::Failed,
            ModelUncertainty::Artifact(ModelArtifactFailure::AlreadyExists) => {
                process == ModelProcess::NotRun
            }
            ModelUncertainty::Artifact(
                ModelArtifactFailure::Missing
                | ModelArtifactFailure::NotFile
                | ModelArtifactFailure::TooLarge
                | ModelArtifactFailure::Unreadable,
            ) => matches!(process, ModelProcess::Exited(_)),
            ModelUncertainty::ExitMismatch { actual, .. } => {
                process == ModelProcess::Exited(*actual)
            }
        };
        if !supported {
            return Err(ModelInvariant::ReasonProcess);
        }
        Ok(Self { reason, evidence })
    }

    const fn mutant(&self) -> &ModelDigest {
        self.evidence.identity.mutant()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelAttemptWire {
    reason: ModelUncertainty,
    evidence: ModelAttemptEvidence,
}

impl<'de> Deserialize<'de> for ModelAttempt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ModelAttemptWire::deserialize(deserializer)?;
        Self::checked(wire.reason, wire.evidence).map_err(serde::de::Error::custom)
    }
}

/// What the optional model-checking phase established for one test survivor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ModelDecision {
    /// The mutation is outside the deliberately closed fragment; no process ran.
    Ineligible {
        /// The closed-set refusal class.
        reason: ModelIneligibility,
    },
    /// Kani produced one counterexample to equality.
    Noticed {
        /// The independently auditable counterexample evidence.
        evidence: ModelEvidence<ModelNoticedExit>,
    },
    /// Kani proved equality with every safety and unwind property succeeding.
    Proved {
        /// The independently auditable proof evidence.
        evidence: ModelEvidence<ModelProvedExit>,
    },
    /// Kani was asked but its evidence supports neither affirmative answer.
    Undecided {
        /// The closed pairing of refusal reason and retained attempt facts.
        attempt: ModelAttempt,
    },
}

impl ModelDecision {
    const fn mutant(&self) -> Option<&ModelDigest> {
        match self {
            Self::Ineligible { .. } => None,
            Self::Noticed { evidence } => Some(evidence.identity.mutant()),
            Self::Proved { evidence } => Some(evidence.identity.mutant()),
            Self::Undecided { attempt } => Some(attempt.mutant()),
        }
    }
}

/// One mutation asked of `verified-v1`, independently of its final outcome row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelRecord {
    /// The full mutation identity.
    mutant: ModelDigest,
    /// What the closed eligibility gate or pinned verifier established.
    answer: ModelDecision,
}

/// The dense zero-based position of one mutation in the canonical catalog.
///
/// This is not a presentation counter.
/// Shard membership is a function of this value, so retaining it is what lets a durable reader prove that a row was not moved between parts after execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CatalogIndex(u32);

impl CatalogIndex {
    /// Constructs an index minted by the catalog builder.
    #[must_use]
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// The catalog's zero-based integer position.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl ModelRecord {
    pub(crate) fn checked(mutant: String, answer: ModelDecision) -> Result<Self, ModelInvariant> {
        let mutant = ModelDigest::parse(mutant).ok_or(ModelInvariant::RecordMutant)?;
        if answer.mutant().is_some_and(|identity| identity != &mutant) {
            return Err(ModelInvariant::RecordBinding);
        }
        Ok(Self { mutant, answer })
    }

    /// The canonical full mutation identity.
    #[must_use]
    pub fn mutant(&self) -> &str {
        self.mutant.as_str()
    }

    /// What the closed model phase established.
    #[must_use]
    pub const fn answer(&self) -> &ModelDecision {
        &self.answer
    }

    #[cfg(feature = "testkit")]
    pub(crate) fn specimen_ineligible(reason: ModelIneligibility) -> Self {
        Self {
            mutant: ModelDigest("a".repeat(64)),
            answer: ModelDecision::Ineligible { reason },
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelRecordWire {
    mutant: String,
    answer: ModelDecision,
}

impl<'de> Deserialize<'de> for ModelRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ModelRecordWire::deserialize(deserializer)?;
        Self::checked(wire.mutant, wire.answer).map_err(serde::de::Error::custom)
    }
}

/// The exact ordered survivor set a completed lattice offers to the model phase.
/// The owner is the final report namespace, not any per-build source namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCandidates {
    owner: rust_mutants::id::RunId,
    builds: Vec<BuildName>,
    mutants: Vec<rust_mutants::id::MutantId>,
}

impl ModelCandidates {
    /// Every configured build, in the exact order proved by the completed build ledger.
    pub(crate) fn builds(&self) -> impl ExactSizeIterator<Item = &BuildName> {
        self.builds.iter()
    }

    /// Every survivor, in canonical catalog order.
    #[must_use]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &rust_mutants::id::MutantId> {
        self.mutants.iter()
    }
}

/// One final-run-owned, catalog-ordered model answer batch.
///
/// Construction compares the complete record sequence with a [`ModelCandidates`] value minted by the same [`LatticedReport`].
/// A caller therefore cannot silently omit, duplicate, reorder, or add a model answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelBatch {
    owner: rust_mutants::id::RunId,
    records: Vec<ModelRecord>,
}

/// Why model records do not answer one exact post-lattice survivor set.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelBatchError {
    /// The record sequence differs from the canonical survivor sequence.
    #[error("model records {actual:?} differ from the exact survivor set {expected:?}")]
    Records {
        /// The required canonical identities.
        expected: Vec<rust_mutants::id::MutantId>,
        /// The identities actually retained.
        actual: Vec<rust_mutants::id::MutantId>,
    },
    /// A stand-alone batch repeated one mutation identity.
    #[error("model record for mutation {mutant} occurs more than once")]
    Duplicate {
        /// The repeated full identity.
        mutant: rust_mutants::id::MutantId,
    },
    /// A model record carried a noncanonical mutation identity.
    #[error("model record carries noncanonical mutation identity {mutant:?}")]
    InvalidMutant {
        /// The rejected wire spelling.
        mutant: String,
    },
}

impl ModelBatch {
    /// Closes one exact ordered survivor answer set under its final owner.
    ///
    /// # Errors
    /// Refuses an omitted, duplicated, extra, or reordered record.
    pub fn checked(
        candidates: &ModelCandidates,
        records: Vec<ModelRecord>,
    ) -> Result<Self, ModelBatchError> {
        let actual = model_record_ids(&records)?;
        if actual != candidates.mutants {
            return Err(ModelBatchError::Records {
                expected: candidates.mutants.clone(),
                actual,
            });
        }
        Ok(Self {
            owner: candidates.owner.clone(),
            records,
        })
    }

    fn structurally_checked(
        owner: rust_mutants::id::RunId,
        records: Vec<ModelRecord>,
    ) -> Result<Self, ModelBatchError> {
        let ids = model_record_ids(&records)?;
        let mut seen = BTreeSet::new();
        for mutant in ids {
            if !seen.insert(mutant.clone()) {
                return Err(ModelBatchError::Duplicate { mutant });
            }
        }
        Ok(Self { owner, records })
    }

    pub(crate) fn artifacts(&self) -> Vec<&ModelArtifact> {
        let mut artifacts = Vec::new();
        for record in &self.records {
            match &record.answer {
                ModelDecision::Ineligible { .. } => {}
                ModelDecision::Noticed { evidence } => {
                    artifacts.push(&evidence.source);
                    artifacts.push(&evidence.artifact);
                }
                ModelDecision::Proved { evidence } => {
                    artifacts.push(&evidence.source);
                    artifacts.push(&evidence.artifact);
                }
                ModelDecision::Undecided { attempt } => {
                    artifacts.push(&attempt.evidence.source);
                    if let Some(artifact) = &attempt.evidence.artifact {
                        artifacts.push(artifact);
                    }
                }
            }
        }
        artifacts
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelBatchWire {
    owner: rust_mutants::id::RunId,
    records: Vec<ModelRecord>,
}

impl<'de> Deserialize<'de> for ModelBatch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ModelBatchWire::deserialize(deserializer)?;
        Self::structurally_checked(wire.owner, wire.records).map_err(serde::de::Error::custom)
    }
}

fn model_record_ids(
    records: &[ModelRecord],
) -> Result<Vec<rust_mutants::id::MutantId>, ModelBatchError> {
    records
        .iter()
        .map(|record| {
            rust_mutants::id::MutantId::try_from(record.mutant()).map_err(|_invalid| {
                ModelBatchError::InvalidMutant {
                    mutant: record.mutant().to_owned(),
                }
            })
        })
        .collect()
}

/// The model-phase state of a durable complete report.
///
/// The sum type distinguishes a contract that has no model phase from a verified contract whose exact survivor set has been answered.
/// There is no `None`/pending state in a serializable [`Report`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "batch",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum ModelCompletion {
    /// `standard-v1` or `deep-v1`; no model answer is meaningful.
    NotRequired,
    /// `verified-v1`; every post-lattice survivor has one retained answer.
    Verified(ModelBatch),
}

/// What became of one mutant.
///
/// The decision and reuse provenance are nested closed objects, so direct deserialization and the published schema both reject extra or ambiguous fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutantRecord {
    /// The dense canonical catalog position used to prove ordering and shard membership.
    pub catalog_index: CatalogIndex,
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
    pub item: String,
    /// The bytes the edit replaces, which narrow a locator to one of several on a line.
    pub original: String,
    /// The bytes it puts there instead, which is what a reader has to see to know what was asked of their tests.
    pub replacement: String,
    /// What the run established, and the target it was established against where there was one.
    #[serde(rename = "decision")]
    pub outcome: Decided,
    /// Whether a reviewer explicitly accepted this remaining gap.
    ///
    /// Kept on the row so durable accounting can be re-derived rather than trusted as an independent counter.
    pub accepted: bool,
    /// The builds it is a hole in, each with what that build established.
    /// Empty when the run measured one build or every build answered for it.
    pub blind_in: Vec<BlindIn>,
    /// Which targets could have noticed it, and what removed the rest.
    /// `null` where the run never asked.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub routing: Option<Routing>,
    /// Whether this run established the disposition, or read it back from another.
    pub reuse: Reuse,
}

/// Whether any target's reach moved between its baseline and a control.
fn moved(drift: &[drift::Drift]) -> bool {
    drift
        .iter()
        .any(|one| matches!(one, drift::Drift::Moved { .. }))
}

/// Rebuilds every mutation counter whose source is the durable mutation rows.
///
/// Producers, shard merging, configured-build reconciliation, and the persistence audit all call this function.
/// A new outcome therefore cannot acquire four subtly different accounting rules.
pub(crate) fn count_mutants(mutants: &[MutantRecord]) -> Result<MutantAccounting, CountError> {
    let mut counts = MutantAccounting {
        cataloged: count_of("mutation rows", mutants.len())?,
        ..MutantAccounting::default()
    };
    for mutant in mutants {
        let outcome = mutant.outcome.outcome();
        counts.observers.counted(outcome.decision())?;
        if mutant.accepted {
            increment("accepted mutations", &mut counts.accepted)?;
        }
        match outcome {
            Outcome::CompileRejected => increment("rejected mutations", &mut counts.rejected)?,
            Outcome::Killed => {
                increment("executed mutations", &mut counts.executed)?;
                increment("killed mutations", &mut counts.killed)?;
                if mutant.reuse.0.read_back().is_some() {
                    increment("reused killed mutations", &mut counts.reused_killed)?;
                }
            }
            Outcome::Survived => {
                increment("executed mutations", &mut counts.executed)?;
                increment("surviving mutations", &mut counts.survived)?;
                if mutant.reuse.0.read_back().is_some() {
                    increment("reused surviving mutations", &mut counts.reused_survived)?;
                }
            }
            Outcome::StepLimitReached => {
                increment("executed mutations", &mut counts.executed)?;
                increment(
                    "step-limit-reached mutations",
                    &mut counts.step_limit_reached,
                )?;
            }
            Outcome::Waited => {
                increment("executed mutations", &mut counts.executed)?;
                increment("waited mutations", &mut counts.waited)?;
            }
            Outcome::Unreached => increment("unreached mutations", &mut counts.unreached)?,
            Outcome::Equivalent => increment("equivalent mutations", &mut counts.equivalent)?,
            Outcome::ModelNoticed => {
                increment("executed mutations", &mut counts.executed)?;
                increment("model-noticed mutations", &mut counts.model_noticed)?;
            }
            Outcome::ModelProved => {
                increment("executed mutations", &mut counts.executed)?;
                increment("model-proved mutations", &mut counts.model_proved)?;
            }
            Outcome::Unconfirmed | Outcome::Errored => {
                increment("executed mutations", &mut counts.executed)?;
            }
        }
    }
    Ok(counts)
}

/// What kind of thing a run found.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, njutest_macros::AllVariants,
)]
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
    /// A mutation crossed its verified step allowance without a matched control establishing divergence.
    StepLimitReachedMutant,
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
    /// A target reached something on an original-code control that it did not reach on its baseline, so every proof read off its baseline is unfounded.
    UnstableBaseline,
    /// A call a `?` asks about failed and every test that reached it passed.
    UnnoticedFault,
    /// A test wrote into the tree under measurement while a fault failed a call, which it did not do while none did.
    BrokenUnderFault,
}

/// Which configured-build evidence raised a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "kebab-case", deny_unknown_fields)]
pub enum FindingOrigin {
    /// A configuration/catalog fact not produced by executing one build.
    Global,
    /// Evidence produced by one retained source namespace for one configured build and catalog part.
    Source {
        /// The exact name in [`Report::builds`].
        build: BuildName,
        /// The namespace that owns the evidence and any related artifacts.
        run_id: rust_mutants::id::RunId,
        /// The catalog part measured by that source.
        part: CatalogPart,
    },
}

impl FindingKind {
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
            Self::StepLimitReachedMutant => "step-limit-reached-mutant",
            Self::NotMeasured => "not-measured",
            Self::UnmatchedAcceptance => "unmatched-acceptance",
            Self::UndefinedBehaviour => "undefined-behaviour",
            Self::HollowTarget => "hollow-target",
            Self::WireUnnoticed => "wire-unnoticed",
            Self::UnstableBaseline => "unstable-baseline",
            Self::UnnoticedFault => "unnoticed-fault",
            Self::BrokenUnderFault => "broken-under-fault",
        }
    }

    /// Whether this is a fault in the code under test rather than a gap in what was established.
    #[must_use]
    pub const fn is_defect(self) -> bool {
        match self {
            Self::BuildFailure
            | Self::FailingTest
            | Self::UndefinedBehaviour
            | Self::BrokenUnderFault => true,
            Self::TargetMissing
            | Self::SurvivingMutant
            | Self::Timeout
            | Self::WaitedMutant
            | Self::StepLimitReachedMutant
            | Self::NotMeasured
            | Self::UnmatchedAcceptance
            | Self::HollowTarget
            | Self::WireUnnoticed
            | Self::UnstableBaseline
            | Self::UnnoticedFault => false,
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
    /// Whether this is global or came from one configured build.
    pub origin: FindingOrigin,
    /// The file it is in, when the run knows, relative to the workspace root.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub path: Option<String>,
    /// Where it is, when the run knows.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub position: Option<Position>,
}

impl Finding {
    /// The wire name of this finding's kind.
    #[must_use]
    pub fn kind_name(&self) -> String {
        self.kind.name().to_owned()
    }

    /// A finding of `kind` about `subject`.
    #[must_use]
    pub fn new(kind: FindingKind, subject: &str, detail: &str) -> Self {
        Self {
            kind,
            subject: subject.to_owned(),
            detail: detail.to_owned(),
            origin: FindingOrigin::Global,
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
    /// The variable names its provider set for every test process, in name order.
    /// Never a value: a report is read by people who may not hold the secret in it.
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
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    pub preimage: Option<String>,
    /// How many times the patched tree passed with nothing active.
    pub stability_runs: u32,
    /// How many times the patched tree noticed the mutant.
    pub kill_runs: u32,
    /// Whether it may be applied.
    pub accepted: bool,
    /// Why it may not, when it may not.
    #[serde(deserialize_with = "crate::strictjson::required_option")]
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

/// The earliest start and latest finish across all retained executions.
///
/// Both spellings have already passed the canonical timestamp parser at the evidence constructor boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WallSpan {
    started: String,
    finished: String,
}

impl WallSpan {
    /// The earliest retained start.
    #[must_use]
    pub fn started(&self) -> &str {
        &self.started
    }

    /// The latest retained finish.
    #[must_use]
    pub fn finished(&self) -> &str {
        &self.finished
    }
}

/// Timing derived from a complete non-empty execution ledger.
///
/// Wall-clock span and summed compute time are different quantities and are deliberately named separately rather than storing one ambiguous `duration_ms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConclusionTiming {
    wall: WallSpan,
    compute_total_ms: u64,
}

/// One configured build's independently retained soundness inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildSoundness {
    build: BuildName,
    accounting: SoundnessAccounting,
}

impl BuildSoundness {
    /// The configured build this inventory was measured under.
    #[must_use]
    pub const fn build(&self) -> &BuildName {
        &self.build
    }

    /// The exact per-build inventory; it is never collapsed with `max`.
    #[must_use]
    pub const fn accounting(&self) -> SoundnessAccounting {
        self.accounting
    }
}

/// Counts derived from the complete build ledger.
///
/// Target and mutation projections span the configured request.
/// Soundness facts remain build-qualified because taking a maximum would erase which program was actually inspected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConclusionAccounting {
    /// Target rows across every configured build.
    pub targets: TargetAccounting,
    /// Mutation decisions after the cross-build and model lattices.
    pub mutants: MutantAccounting,
    /// One exact soundness inventory per configured build, in request order.
    pub soundness_by_build: Vec<BuildSoundness>,
    /// The fault sites of every build and part, counted by what became of each.
    pub faults: faults::FaultAccounting,
}

impl ConclusionTiming {
    /// The earliest-start/latest-finish wall span.
    #[must_use]
    pub const fn wall(&self) -> &WallSpan {
        &self.wall
    }

    /// The checked sum of each retained source's measured duration.
    #[must_use]
    pub const fn compute_total_ms(&self) -> u64 {
        self.compute_total_ms
    }
}

/// Mutable evidence while one configured build is being measured.
///
/// This type is deliberately not serializable.
/// Completion consumes one or more build reports into [`Report`], whose non-empty build ledger is the only durable source of truth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildReport {
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
    pub resources: Vec<ResourceRecord>,
    /// Every repair a provider offered, and what putting it to the tests established.
    pub candidates: Vec<CandidateRecord>,
    /// Every question a watched seam licensed, and what became of it.
    pub seams: Vec<SeamRecord>,
    /// Every site a fault was asked at, and what became of it.
    pub faults: Vec<faults::FaultRecord>,
    /// Every survivor a target told apart only once the call at its site failed.
    pub beside: Vec<faults::BesideRecord>,
    /// Every target it selected, slowest first.
    pub targets: Vec<TargetRecord>,
    /// The SHA-256 of each file its mutants were read from, as it read them, by workspace-relative path.
    pub sources: BTreeMap<String, rust_mutants::id::HexDigest>,
    /// Every mutant it has something to say about.
    pub mutants: Vec<MutantRecord>,
    /// Every actionable problem it found in the project or its verification configuration.
    pub findings: Vec<Finding>,
    /// Everything it is not claiming.
    pub limitations: Vec<Limitation>,
    /// Whether each target its baseline measured reached, on an original-code control, what it reached on that baseline.
    pub drift: Vec<drift::Drift>,
}

impl BuildReport {
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
            faults: Vec::new(),
            beside: Vec::new(),
            targets: Vec::new(),
            sources: BTreeMap::new(),
            mutants: Vec::new(),
            findings: Vec::new(),
            limitations: Vec::new(),
            drift: Vec::new(),
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
    ///
    /// # Errors
    /// Returns [`CountError`] when the durable counters cannot represent all retained rows.
    pub fn count_targets(&mut self) -> Result<(), CountError> {
        self.accounting.targets = count_targets(&self.targets)?;
        Ok(())
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
        if !observed || !asked || moved(&self.drift) {
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

/// A non-empty, ordered configured-build ledger.
///
/// The default build is stored separately from the remaining builds, so an empty completed report cannot be represented.
/// Construction also fixes the catalog relation once; callers can inspect but never mutate the entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildLedger {
    first: BuildEvidence,
    rest: Vec<BuildEvidence>,
    count: std::num::NonZeroUsize,
    catalog_count: u32,
    timing: ConclusionTiming,
}

/// A structural refusal while completing the ordered configured-build ledger.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildLedgerError {
    /// No configured build was retained.
    #[error("a completed report must retain at least its default build")]
    Empty,
    /// The first entry was not the default configuration.
    #[error("the first configured build must be {expected:?}, not {actual:?}")]
    DefaultFirst {
        /// The required name.
        expected: &'static str,
        /// The name found.
        actual: BuildName,
    },
    /// A build name occurred twice.
    #[error("configured build name {name:?} occurs more than once")]
    DuplicateName {
        /// The repeated configured name.
        name: BuildName,
    },
    /// An evidence namespace occurred in more than one build/part.
    #[error("evidence namespace {run_id:?} occurs more than once")]
    DuplicateRun {
        /// The repeated canonical namespace.
        run_id: rust_mutants::id::RunId,
    },
    /// Two builds do not carry the same catalog.
    #[error("configured build {build:?} does not carry the default build's exact mutation catalog")]
    CatalogMismatch {
        /// The configured build whose partition differed.
        build: BuildName,
    },
    /// Two builds were measured with different whole/shard divisions.
    #[error("configured build {build:?} does not carry the default build's exact part set")]
    PartSetMismatch {
        /// The configured build whose part division differed.
        build: BuildName,
    },
    /// The complete catalog cannot be represented by the report counters.
    #[error("the complete mutation catalog contains {count} rows, beyond the u32 report boundary")]
    CatalogTooLarge {
        /// The unrepresentable row count.
        count: usize,
    },
    /// Adding shard catalog sizes overflowed the platform counter.
    #[error("the complete mutation catalog size overflowed usize")]
    CatalogCountOverflow,
    /// Summing retained source compute durations overflowed the fixed wire.
    #[error("the retained source compute-duration total overflowed u64 milliseconds")]
    DurationOverflow,
    /// Constructor-proved source timing could not be re-read while deriving the complete ledger span.
    #[error("retained source {run_id} lost its canonical {field} timestamp invariant")]
    RetainedTiming {
        /// The owning source namespace.
        run_id: rust_mutants::id::RunId,
        /// `started` or `finished`.
        field: &'static str,
    },
}

impl BuildLedger {
    /// Builds a ledger after proving its structural cross-build invariants.
    ///
    /// # Errors
    /// Empty input, a non-default first build, duplicate names or namespaces,
    /// or build catalogs with different identities or order.
    pub(crate) fn try_from_vec(builds: Vec<BuildEvidence>) -> Result<Self, BuildLedgerError> {
        let count = std::num::NonZeroUsize::new(builds.len()).ok_or(BuildLedgerError::Empty)?;
        let mut builds = builds.into_iter();
        let Some(first) = builds.next() else {
            return Err(BuildLedgerError::Empty);
        };
        let rest: Vec<_> = builds.collect();
        if first.name.as_str() != crate::config::DEFAULT_CONFIGURATION {
            return Err(BuildLedgerError::DefaultFirst {
                expected: crate::config::DEFAULT_CONFIGURATION,
                actual: first.name,
            });
        }
        let mut names = BTreeSet::new();
        let mut runs = BTreeSet::new();
        let expected_catalog = catalog_partition(&first);
        let catalog_len = expected_catalog
            .iter()
            .map(|(_, rows)| rows.len())
            .try_fold(0usize, usize::checked_add)
            .ok_or(BuildLedgerError::CatalogCountOverflow)?;
        let catalog_count = u32::try_from(catalog_len)
            .map_err(|_overflow| BuildLedgerError::CatalogTooLarge { count: catalog_len })?;
        let expected_parts: Vec<CatalogPart> = first.parts.iter().map(|part| part.part).collect();
        for build in std::iter::once(&first).chain(rest.iter()) {
            if !names.insert(build.name.as_str()) {
                return Err(BuildLedgerError::DuplicateName {
                    name: build.name.clone(),
                });
            }
            for part in build.parts.iter() {
                if !runs.insert(part.run_id.clone()) {
                    return Err(BuildLedgerError::DuplicateRun {
                        run_id: part.run_id.clone(),
                    });
                }
            }
            if build.parts.iter().map(|part| part.part).collect::<Vec<_>>() != expected_parts {
                return Err(BuildLedgerError::PartSetMismatch {
                    build: build.name.clone(),
                });
            }
            if catalog_partition(build) != expected_catalog {
                return Err(BuildLedgerError::CatalogMismatch {
                    build: build.name.clone(),
                });
            }
        }
        let timing = conclusion_timing(std::iter::once(&first).chain(rest.iter()))?;
        Ok(Self {
            first,
            rest,
            count,
            catalog_count,
            timing,
        })
    }

    /// The default build.
    #[must_use]
    pub const fn first(&self) -> &BuildEvidence {
        &self.first
    }

    /// Every build in configuration order.
    pub fn iter(&self) -> impl Iterator<Item = &BuildEvidence> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }

    /// How many builds are retained.
    #[must_use]
    pub(crate) const fn len(&self) -> usize {
        self.count.get()
    }

    /// The complete catalog size proved representable by durable counters.
    #[must_use]
    pub const fn catalog_count(&self) -> u32 {
        self.catalog_count
    }
}

fn conclusion_timing<'a>(
    builds: impl Iterator<Item = &'a BuildEvidence>,
) -> Result<ConclusionTiming, BuildLedgerError> {
    let mut sources = builds.flat_map(|build| build.parts.iter());
    let Some(first) = sources.next() else {
        return Err(BuildLedgerError::Empty);
    };
    let mut earliest_value = first.timing.started.clone();
    let mut latest_value = first.timing.finished.clone();
    let mut earliest = first
        .timing
        .started
        .parse::<jiff::Timestamp>()
        .map_err(|_invalid| BuildLedgerError::RetainedTiming {
            run_id: first.run_id.clone(),
            field: "started",
        })?;
    let mut latest = first
        .timing
        .finished
        .parse::<jiff::Timestamp>()
        .map_err(|_invalid| BuildLedgerError::RetainedTiming {
            run_id: first.run_id.clone(),
            field: "finished",
        })?;
    let mut compute_total_ms = first.timing.duration_ms;
    for source in sources {
        let started = source
            .timing
            .started
            .parse::<jiff::Timestamp>()
            .map_err(|_invalid| BuildLedgerError::RetainedTiming {
                run_id: source.run_id.clone(),
                field: "started",
            })?;
        let finished = source
            .timing
            .finished
            .parse::<jiff::Timestamp>()
            .map_err(|_invalid| BuildLedgerError::RetainedTiming {
                run_id: source.run_id.clone(),
                field: "finished",
            })?;
        if started < earliest {
            earliest = started;
            earliest_value.clone_from(&source.timing.started);
        }
        if finished > latest {
            latest = finished;
            latest_value.clone_from(&source.timing.finished);
        }
        compute_total_ms = compute_total_ms
            .checked_add(source.timing.duration_ms)
            .ok_or(BuildLedgerError::DurationOverflow)?;
    }
    Ok(ConclusionTiming {
        wall: WallSpan {
            started: earliest_value,
            finished: latest_value,
        },
        compute_total_ms,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CatalogEntry {
    index: CatalogIndex,
    id: String,
    display_id: String,
    path: String,
    position: Position,
    rule: String,
    item: String,
    original: String,
    replacement: String,
}

fn catalog_partition(build: &BuildEvidence) -> Vec<(CatalogPart, Vec<CatalogEntry>)> {
    build
        .parts
        .iter()
        .map(|part| {
            (
                part.part,
                part.mutants
                    .iter()
                    .map(|mutant| CatalogEntry {
                        index: mutant.catalog_index,
                        id: mutant.id.clone(),
                        display_id: mutant.display_id.clone(),
                        path: mutant.path.clone(),
                        position: mutant.position,
                        rule: mutant.rule.clone(),
                        item: mutant.item.clone(),
                        original: mutant.original.clone(),
                        replacement: mutant.replacement.clone(),
                    })
                    .collect(),
            )
        })
        .collect()
}

impl Serialize for BuildLedger {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self.iter())
    }
}

impl<'de> Deserialize<'de> for BuildLedger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let builds = Vec::<BuildEvidence>::deserialize(deserializer)?;
        Self::try_from_vec(builds).map_err(serde::de::Error::custom)
    }
}

/// One configured build's evidence for a single catalog shard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShardBuildEvidence {
    /// The configuration name (`default` for the primary build).
    name: BuildName,
    /// The exact Cargo selection for this build.
    configuration: BuildSelection,
    /// The one source namespace that measured this shard.
    source: BuildPartEvidence,
}

impl ShardBuildEvidence {
    /// Joins already proved typed components without exposing mutable fields.
    pub(crate) const fn from_parts(
        name: BuildName,
        configuration: BuildSelection,
        source: BuildPartEvidence,
    ) -> Self {
        Self {
            name,
            configuration,
            source,
        }
    }
}

/// A non-empty configured-build ledger for one and the same shard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShardBuildLedger {
    first: ShardBuildEvidence,
    rest: Vec<ShardBuildEvidence>,
    shard: ShardPart,
}

/// A structural refusal while retaining a single-shard build ledger.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShardBuildLedgerError {
    /// No configured build was retained.
    #[error("a shard report must retain at least its default build")]
    Empty,
    /// The first configured build was not `default`.
    #[error("the first configured build must be {expected:?}, not {actual:?}")]
    DefaultFirst {
        /// The required name.
        expected: &'static str,
        /// The name found.
        actual: BuildName,
    },
    /// A configured build name occurred twice.
    #[error("configured build name {name:?} occurs more than once")]
    DuplicateName {
        /// The repeated name.
        name: BuildName,
    },
    /// A source did not describe a shard.
    #[error("configured build {build:?} carries whole-catalog evidence in a shard report")]
    WholeSource {
        /// The offending build.
        build: BuildName,
    },
    /// A source described a different shard.
    #[error(
        "configured build {build:?} carries shard {actual_index}/{actual_of}, expected {expected_index}/{expected_of}"
    )]
    ShardMismatch {
        /// The offending build.
        build: BuildName,
        /// The required shard index.
        expected_index: u32,
        /// The required denominator.
        expected_of: u32,
        /// The source's shard index.
        actual_index: u32,
        /// The source's denominator.
        actual_of: u32,
    },
    /// One source namespace occurred in more than one configured build.
    #[error("evidence namespace {run_id} occurs more than once")]
    DuplicateRun {
        /// The repeated canonical namespace.
        run_id: rust_mutants::id::RunId,
    },
    /// Configured builds did not retain the identical ordered rows for the shard.
    #[error(
        "configured build {build:?} does not carry the default build's exact ordered shard catalog"
    )]
    CatalogMismatch {
        /// The offending build.
        build: BuildName,
    },
    /// A source's own row order or shard membership was invalid.
    #[error(transparent)]
    Part(#[from] PartLedgerError),
}

impl ShardBuildLedger {
    /// Proves a non-empty, ordered ledger whose sources all measured the same shard.
    pub(crate) fn try_from_vec(
        builds: Vec<ShardBuildEvidence>,
    ) -> Result<Self, ShardBuildLedgerError> {
        let mut builds = builds.into_iter();
        let Some(first) = builds.next() else {
            return Err(ShardBuildLedgerError::Empty);
        };
        let rest: Vec<_> = builds.collect();
        if first.name.as_str() != crate::config::DEFAULT_CONFIGURATION {
            return Err(ShardBuildLedgerError::DefaultFirst {
                expected: crate::config::DEFAULT_CONFIGURATION,
                actual: first.name,
            });
        }
        let CatalogPart::Shard(shard) = first.source.part else {
            return Err(ShardBuildLedgerError::WholeSource { build: first.name });
        };
        validate_part_evidence(&first.source)?;
        let expected_catalog = catalog_rows(&first.source);
        let mut names = BTreeSet::new();
        let mut runs = BTreeSet::new();
        for build in std::iter::once(&first).chain(rest.iter()) {
            if !names.insert(build.name.as_str()) {
                return Err(ShardBuildLedgerError::DuplicateName {
                    name: build.name.clone(),
                });
            }
            if !runs.insert(build.source.run_id.clone()) {
                return Err(ShardBuildLedgerError::DuplicateRun {
                    run_id: build.source.run_id.clone(),
                });
            }
            let CatalogPart::Shard(actual) = build.source.part else {
                return Err(ShardBuildLedgerError::WholeSource {
                    build: build.name.clone(),
                });
            };
            if actual != shard {
                return Err(ShardBuildLedgerError::ShardMismatch {
                    build: build.name.clone(),
                    expected_index: shard.index(),
                    expected_of: shard.of(),
                    actual_index: actual.index(),
                    actual_of: actual.of(),
                });
            }
            validate_part_evidence(&build.source)?;
            if catalog_rows(&build.source) != expected_catalog {
                return Err(ShardBuildLedgerError::CatalogMismatch {
                    build: build.name.clone(),
                });
            }
        }
        Ok(Self { first, rest, shard })
    }

    /// Every configured build in request order.
    pub fn iter(&self) -> impl Iterator<Item = &ShardBuildEvidence> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }

    /// The shard every source measured.
    #[must_use]
    pub const fn shard(&self) -> ShardPart {
        self.shard
    }
}

fn catalog_rows(part: &BuildPartEvidence) -> Vec<CatalogEntry> {
    part.mutants
        .iter()
        .map(|mutant| CatalogEntry {
            index: mutant.catalog_index,
            id: mutant.id.clone(),
            display_id: mutant.display_id.clone(),
            path: mutant.path.clone(),
            position: mutant.position,
            rule: mutant.rule.clone(),
            item: mutant.item.clone(),
            original: mutant.original.clone(),
            replacement: mutant.replacement.clone(),
        })
        .collect()
}

impl Serialize for ShardBuildLedger {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self.iter())
    }
}

impl<'de> Deserialize<'de> for ShardBuildLedger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let builds = Vec::<ShardBuildEvidence>::deserialize(deserializer)?;
        Self::try_from_vec(builds).map_err(serde::de::Error::custom)
    }
}

/// One configured build's exact retained answer for a projected mutation.
///
/// Unlike the projection, this remains bound to the source namespace and catalog part that established it.
/// Acceptance, routing, and reuse never leak into a synthetic representative row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildMutationDecision {
    build: BuildName,
    run_id: rust_mutants::id::RunId,
    part: CatalogPart,
    decision: Decided,
    accepted: bool,
    routing: Option<Routing>,
    reuse: Reuse,
}

impl BuildMutationDecision {
    /// The configured build that established this answer.
    #[must_use]
    pub const fn build(&self) -> &BuildName {
        &self.build
    }

    /// The source row's closed outcome and optional deciding target.
    #[must_use]
    pub const fn outcome(&self) -> &Decided {
        &self.decision
    }

    /// The source row's exact reuse provenance.
    #[must_use]
    pub const fn reuse(&self) -> &Reuse {
        &self.reuse
    }

    /// Whether a reviewer accepted the source row.
    #[must_use]
    pub const fn accepted(&self) -> bool {
        self.accepted
    }

    /// Which targets could have noticed the source row's mutation, and what removed the rest, where the run asked.
    #[must_use]
    pub const fn routing(&self) -> Option<&Routing> {
        self.routing.as_ref()
    }
}

/// A presentation-only cross-build mutation projection.
///
/// This deliberately has no synthetic `accepted`, `routing`, `reuse`, or selected source outcome.
/// Those facts remain on [`BuildMutationDecision`] entries, while `decision` is only the lattice minimum across them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedMutant {
    catalog_index: CatalogIndex,
    id: String,
    display_id: String,
    path: String,
    position: Position,
    rule: String,
    item: String,
    original: String,
    replacement: String,
    decision: Decision,
    by_build: Vec<BuildMutationDecision>,
    blind_in: Vec<BlindIn>,
}

impl ProjectedMutant {
    /// Full mutation identity.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Human-facing mutation identity.
    #[must_use]
    pub fn display_id(&self) -> &str {
        &self.display_id
    }

    /// Workspace-relative source path.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Exact source position.
    #[must_use]
    pub const fn position(&self) -> Position {
        self.position
    }

    /// Mutation rule name.
    #[must_use]
    pub fn rule(&self) -> &str {
        &self.rule
    }

    /// Enclosing item name, if known.
    #[must_use]
    pub fn item(&self) -> &str {
        &self.item
    }

    /// Original source spelling.
    #[must_use]
    pub fn original(&self) -> &str {
        &self.original
    }

    /// Replacement source spelling.
    #[must_use]
    pub fn replacement(&self) -> &str {
        &self.replacement
    }

    /// Weakest lattice decision across all configured builds.
    #[must_use]
    pub const fn decision(&self) -> Decision {
        self.decision
    }

    /// Every configured build's source-bound answer in request order.
    #[must_use]
    pub fn by_build(&self) -> &[BuildMutationDecision] {
        &self.by_build
    }

    /// Every configured build in which this remains a gap.
    #[must_use]
    pub fn blind_in(&self) -> &[BlindIn] {
        &self.blind_in
    }
}

/// The derived, non-serialized view used by presentation and decisions.
/// Every call reconstructs it from [`Report::builds`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conclusion {
    /// The verdict all build rows support together.
    pub verdict: Verdict,
    /// The span and sum of all configured build executions.
    pub timing: ConclusionTiming,
    /// Counts derived from the projected rows.
    pub accounting: ConclusionAccounting,
    /// Every build's resources, in build then start order.
    pub resources: Vec<ResourceRecord>,
    /// Every build's repair candidates, without cross-build deduplication.
    pub candidates: Vec<CandidateRecord>,
    /// Every build's seam facts.
    pub seams: Vec<SeamRecord>,
    /// Every build's fault sites, part by part.
    pub faults: Vec<faults::FaultRecord>,
    /// Every build's survivors told apart only under a fault, part by part.
    pub beside: Vec<faults::BesideRecord>,
    /// Every build's target facts.
    pub targets: Vec<TargetRecord>,
    /// The mutation lattice projection, for presentation only.
    pub mutants: Vec<ProjectedMutant>,
    /// Every build-qualified finding; global findings occur once.
    pub findings: Vec<Finding>,
    /// Every build's limitations.
    pub limitations: Vec<Limitation>,
    /// The SHA-256 of each file the run's mutants were read from, which every part and build agreed on.
    pub sources: BTreeMap<String, rust_mutants::id::HexDigest>,
}

/// One partial report envelope consumed by a complete shard merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeSource {
    run_id: rust_mutants::id::RunId,
    shard: ShardPart,
}

impl MergeSource {
    /// Binds one typed partial envelope namespace to its typed shard.
    pub(crate) const fn from_parts(run_id: rust_mutants::id::RunId, shard: ShardPart) -> Self {
        Self { run_id, shard }
    }
}

/// The complete, canonical source-report set behind one merged answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeSources {
    first: MergeSource,
    rest: Vec<MergeSource>,
}

/// Why partial report envelopes do not form one complete merge source set.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MergeSourcesError {
    /// No partial report was retained.
    #[error("a merged report must retain at least one source report")]
    Empty,
    /// Two partial reports reused one namespace.
    #[error("partial report namespace {run_id} occurs more than once")]
    DuplicateRun {
        /// The repeated namespace.
        run_id: rust_mutants::id::RunId,
    },
    /// The same shard occurred twice.
    #[error("partial report shard {index}/{of} occurs more than once")]
    DuplicateShard {
        /// The repeated one-based index.
        index: u32,
        /// The shared denominator.
        of: u32,
    },
    /// One report used another denominator.
    #[error("partial report shard {index}/{actual} disagrees with denominator {expected}")]
    Denominator {
        /// The report's one-based index.
        index: u32,
        /// The required denominator.
        expected: u32,
        /// The report's denominator.
        actual: u32,
    },
    /// One required shard was absent.
    #[error("partial report shard {index}/{of} is missing")]
    Missing {
        /// The missing one-based index.
        index: u32,
        /// The shared denominator.
        of: u32,
    },
}

impl MergeSources {
    /// Canonicalizes unordered inputs by shard index and proves exact denominator coverage plus namespace uniqueness.
    pub(crate) fn checked(sources: Vec<MergeSource>) -> Result<Self, MergeSourcesError> {
        if sources.is_empty() {
            return Err(MergeSourcesError::Empty);
        }
        let Some(seed) = sources.first() else {
            return Err(MergeSourcesError::Empty);
        };
        let of = seed.shard.of();
        let mut by_index = BTreeMap::new();
        let mut runs = BTreeSet::new();
        for source in sources {
            if source.shard.of() != of {
                return Err(MergeSourcesError::Denominator {
                    index: source.shard.index(),
                    expected: of,
                    actual: source.shard.of(),
                });
            }
            if !runs.insert(source.run_id.clone()) {
                return Err(MergeSourcesError::DuplicateRun {
                    run_id: source.run_id,
                });
            }
            let index = source.shard.index();
            if by_index.insert(index, source).is_some() {
                return Err(MergeSourcesError::DuplicateShard { index, of });
            }
        }
        for index in 1..=of {
            if !by_index.contains_key(&index) {
                return Err(MergeSourcesError::Missing { index, of });
            }
        }
        let Some(first) = by_index.remove(&1) else {
            return Err(MergeSourcesError::Missing { index: 1, of });
        };
        Ok(Self {
            first,
            rest: by_index.into_values().collect(),
        })
    }

    /// Every partial source in canonical shard-index order.
    pub fn iter(&self) -> impl Iterator<Item = &MergeSource> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }
}

impl Serialize for MergeSources {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self.iter())
    }
}

impl<'de> Deserialize<'de> for MergeSources {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::checked(Vec::<MergeSource>::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

/// How this complete response was assembled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Composition {
    /// One whole-catalog configured run completed directly.
    Direct,
    /// A complete set of partial report envelopes was merged.
    Merged {
        /// Every partial input, in canonical shard order.
        sources: MergeSources,
    },
}

/// A complete test/build lattice before the contract-specific model phase.
///
/// This value is deliberately not serializable.
/// It can become a durable [`Report`] only by taking one of the contract-checked consuming transitions:
/// [`LatticedReport::complete_without_models`] or [`LatticedReport::attach_models`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatticedReport {
    schema: String,
    schema_version: u32,
    run_id: rust_mutants::id::RunId,
    run_kind: RunKind,
    contract: crate::config::Contract,
    tool: Tool,
    repository: Repository,
    provenance: Provenance,
    scope: Scope,
    composition: Composition,
    builds: BuildLedger,
    global_findings: Vec<Finding>,
}

/// One completed verification.
///
/// The non-empty build ledger is the sole source of execution evidence;
/// derived verdicts, counters, and row projections are never stored or serialized.
/// Unlike [`LatticedReport`], this type always carries the exact terminal model state required by its contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    /// [`SCHEMA`].
    schema: String,
    /// [`SCHEMA_VERSION`].
    schema_version: u32,
    /// The final run identity.
    run_id: rust_mutants::id::RunId,
    /// The scope kind.
    run_kind: RunKind,
    /// The assurance contract.
    contract: crate::config::Contract,
    /// Producer versions shared by every build.
    tool: Tool,
    /// Repository evidence shared by every build.
    repository: Repository,
    /// Run-wide provenance.
    provenance: Provenance,
    /// The exact requested scope and configured build order.
    scope: Scope,
    /// Whether this answer completed directly or from typed partial reports.
    composition: Composition,
    /// The sole source of execution evidence.
    builds: BuildLedger,
    /// Run-wide findings derived without executing any one build, retained exactly once rather than copied into each source.
    global_findings: Vec<Finding>,
    /// The terminal contract-specific model state produced after the cross-build and cross-shard lattice.
    model_completion: ModelCompletion,
}

/// Why mutable build evidence could not become an immutable completed report.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompletionError {
    /// A derived evidence ledger exceeded the report's exact counter range.
    #[error(transparent)]
    Count(#[from] CountError),
    /// The final answer was not given a canonical writable run namespace.
    #[error(transparent)]
    RunId(#[from] rust_mutants::id::RunIdError),
    /// The non-empty configured-build structure was invalid.
    #[error(transparent)]
    Ledger(#[from] BuildLedgerError),
    /// Cross-field evidence contradicted itself.
    #[error(
        "completed report evidence is contradictory: {}",
        .violations.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")
    )]
    Contradictory {
        /// Every independently re-derived contradiction.
        violations: Vec<audit::Violation>,
    },
    /// A source namespace was reused as the completed answer's namespace.
    #[error("final run namespace {run_id} is also an evidence-source namespace")]
    EvidenceUsesFinalRun {
        /// The aliased final/source namespace.
        run_id: rust_mutants::id::RunId,
    },
    /// A direct completion retained divided-catalog evidence.
    #[error("a direct complete report must retain one whole-catalog source per build")]
    DirectHasShards,
    /// A merged completion's source envelopes and retained evidence parts differ.
    #[error("merged source shards {sources:?} differ from retained evidence parts {evidence:?}")]
    MergedParts {
        /// The source-report shard set.
        sources: Vec<CatalogPart>,
        /// The retained default-build part set.
        evidence: Vec<CatalogPart>,
    },
    /// A merge envelope namespace aliases an internal build evidence namespace.
    #[error("merge source namespace {run_id} is also an internal build evidence namespace")]
    MergeSourceUsesEvidence {
        /// The aliased namespace.
        run_id: rust_mutants::id::RunId,
    },
    /// A pre-lattice build already contains a model-derived outcome.
    #[error("mutation {mutant} contains model outcome {decision} before the final lattice")]
    PrematureModelOutcome {
        /// The affected full mutation identity.
        mutant: String,
        /// The impossible pre-lattice decision.
        decision: &'static str,
    },
    /// A constructor-proved catalog row could not be recovered as its typed mutation identity while minting the model candidate set.
    #[error("projected mutation identity {mutant:?} is not canonical")]
    InvalidProjectedMutant {
        /// The invalid full identity.
        mutant: String,
    },
    /// The selected contract requires a model batch before persistence.
    #[error("verified-v1 requires an exact post-lattice model batch")]
    ModelRequired,
    /// A non-model contract was given a model batch.
    #[error("contract {contract:?} does not admit model evidence")]
    ModelForbidden {
        /// The contract that has no model phase.
        contract: crate::config::Contract,
    },
    /// A retained model batch belongs to another final namespace.
    #[error("model batch owner {actual} differs from final run namespace {expected}")]
    ModelOwner {
        /// The final report namespace.
        expected: rust_mutants::id::RunId,
        /// The batch owner.
        actual: rust_mutants::id::RunId,
    },
    /// The retained model records do not answer the exact survivor set.
    #[error(transparent)]
    ModelBatch(#[from] ModelBatchError),
    /// Model artifacts cannot be relabelled into another run namespace.
    #[error("verified model artifacts owned by {owner} cannot be reissued as {requested}")]
    ModelReadBack {
        /// The namespace that actually owns the model artifacts.
        owner: rust_mutants::id::RunId,
        /// The requested read-back namespace.
        requested: rust_mutants::id::RunId,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportWire {
    schema: String,
    schema_version: u32,
    run_id: rust_mutants::id::RunId,
    run_kind: RunKind,
    contract: crate::config::Contract,
    tool: Tool,
    repository: Repository,
    provenance: Provenance,
    scope: Scope,
    composition: Composition,
    builds: BuildLedger,
    global_findings: Vec<Finding>,
    model_completion: ModelCompletion,
}

impl<'de> Deserialize<'de> for Report {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ReportWire::deserialize(deserializer)?;
        Self::checked(Self {
            schema: wire.schema,
            schema_version: wire.schema_version,
            run_id: wire.run_id,
            run_kind: wire.run_kind,
            contract: wire.contract,
            tool: wire.tool,
            repository: wire.repository,
            provenance: wire.provenance,
            scope: wire.scope,
            composition: wire.composition,
            builds: wire.builds,
            global_findings: wire.global_findings,
            model_completion: wire.model_completion,
        })
        .map_err(serde::de::Error::custom)
    }
}

impl LatticedReport {
    pub(crate) fn from_parts(
        template: &BuildReport,
        final_run_id: &rust_mutants::id::RunId,
        builds: BuildLedger,
        global_findings: Vec<Finding>,
    ) -> Result<Self, CompletionError> {
        Self::checked(Self {
            schema: template.schema.clone(),
            schema_version: template.schema_version,
            run_id: final_run_id.clone(),
            run_kind: template.run_kind,
            contract: template.contract,
            tool: template.tool.clone(),
            repository: template.repository.clone(),
            provenance: template.provenance.clone(),
            scope: template.scope.clone(),
            composition: Composition::Direct,
            builds,
            global_findings,
        })
    }

    pub(crate) fn from_merged_parts(
        template: &BuildReport,
        final_run_id: &rust_mutants::id::RunId,
        builds: BuildLedger,
        merged: MergedCompletion,
    ) -> Result<Self, CompletionError> {
        let MergedCompletion {
            global_findings,
            sources,
        } = merged;
        Self::checked(Self {
            schema: template.schema.clone(),
            schema_version: template.schema_version,
            run_id: final_run_id.clone(),
            run_kind: template.run_kind,
            contract: template.contract,
            tool: template.tool.clone(),
            repository: template.repository.clone(),
            provenance: template.provenance.clone(),
            scope: template.scope.clone(),
            composition: Composition::Merged { sources },
            builds,
            global_findings,
        })
    }

    fn checked(report: Self) -> Result<Self, CompletionError> {
        validate_completion_shape(&report.run_id, &report.composition, &report.builds)?;
        reject_premature_model_outcomes(&report.builds)?;
        let projected = projected_mutants_with_models(&report.builds, &[]);
        let accounting = projected_accounting(&report.builds, &projected)?;
        let violations = audit::validate_lattice(&report, &accounting, projected.len());
        if violations.is_empty() {
            Ok(report)
        } else {
            Err(CompletionError::Contradictory { violations })
        }
    }

    /// The assurance contract that determines the only legal completion transition.
    #[must_use]
    pub const fn contract(&self) -> crate::config::Contract {
        self.contract
    }

    /// The exact ordered survivor set to pass to the one post-lattice model phase.
    ///
    /// # Errors
    /// Refuses contracts that have no model phase.
    pub fn model_candidates(&self) -> Result<ModelCandidates, CompletionError> {
        if self.contract != crate::config::Contract::VerifiedV1 {
            return Err(CompletionError::ModelForbidden {
                contract: self.contract,
            });
        }
        Ok(ModelCandidates {
            owner: self.run_id.clone(),
            builds: self
                .builds
                .iter()
                .map(|build| build.name().clone())
                .collect(),
            mutants: expected_model_ids(&self.builds)?,
        })
    }

    /// Completes a contract that has no model phase.
    ///
    /// # Errors
    /// `verified-v1` cannot take this transition; it must call [`Self::attach_models`].
    pub fn complete_without_models(self) -> Result<Report, CompletionError> {
        if self.contract == crate::config::Contract::VerifiedV1 {
            return Err(CompletionError::ModelRequired);
        }
        self.into_report(ModelCompletion::NotRequired)
    }

    /// Attaches the exact final-run-owned model batch and completes `verified-v1`.
    ///
    /// # Errors
    /// Refuses another contract, another owner, or any record sequence other than the exact canonical survivor sequence.
    pub fn attach_models(self, batch: ModelBatch) -> Result<Report, CompletionError> {
        if self.contract != crate::config::Contract::VerifiedV1 {
            return Err(CompletionError::ModelForbidden {
                contract: self.contract,
            });
        }
        validate_model_batch(&self.run_id, &self.builds, &batch)?;
        self.into_report(ModelCompletion::Verified(batch))
    }

    fn into_report(self, model_completion: ModelCompletion) -> Result<Report, CompletionError> {
        Report::checked(Report {
            schema: self.schema,
            schema_version: self.schema_version,
            run_id: self.run_id,
            run_kind: self.run_kind,
            contract: self.contract,
            tool: self.tool,
            repository: self.repository,
            provenance: self.provenance,
            scope: self.scope,
            composition: self.composition,
            builds: self.builds,
            global_findings: self.global_findings,
            model_completion,
        })
    }
}

pub(crate) struct MergedCompletion {
    pub(crate) global_findings: Vec<Finding>,
    pub(crate) sources: MergeSources,
}

fn validate_completion_shape(
    run_id: &rust_mutants::id::RunId,
    composition: &Composition,
    builds: &BuildLedger,
) -> Result<(), CompletionError> {
    let evidence_runs: BTreeSet<_> = builds
        .iter()
        .flat_map(|build| build.parts.iter())
        .map(|part| part.run_id.clone())
        .collect();
    if evidence_runs.contains(run_id) {
        return Err(CompletionError::EvidenceUsesFinalRun {
            run_id: run_id.clone(),
        });
    }
    let evidence_parts: Vec<_> = builds.first().parts.iter().map(|part| part.part).collect();
    match composition {
        Composition::Direct => {
            if evidence_parts != [CatalogPart::Whole] {
                return Err(CompletionError::DirectHasShards);
            }
        }
        Composition::Merged { sources } => {
            let source_parts: Vec<_> = sources
                .iter()
                .map(|source| CatalogPart::Shard(source.shard))
                .collect();
            if source_parts != evidence_parts {
                return Err(CompletionError::MergedParts {
                    sources: source_parts,
                    evidence: evidence_parts,
                });
            }
            for source in sources.iter() {
                if &source.run_id == run_id {
                    return Err(CompletionError::EvidenceUsesFinalRun {
                        run_id: source.run_id.clone(),
                    });
                }
                if evidence_runs.contains(&source.run_id) {
                    return Err(CompletionError::MergeSourceUsesEvidence {
                        run_id: source.run_id.clone(),
                    });
                }
            }
        }
    }
    Ok(())
}

fn reject_premature_model_outcomes(builds: &BuildLedger) -> Result<(), CompletionError> {
    for row in builds
        .iter()
        .flat_map(|build| build.parts.iter())
        .flat_map(|part| part.mutants.iter())
    {
        let decision = row.outcome.decision();
        if matches!(decision, Decision::ModelNoticed | Decision::ModelProved) {
            return Err(CompletionError::PrematureModelOutcome {
                mutant: row.id.clone(),
                decision: decision.name(),
            });
        }
    }
    Ok(())
}

fn expected_model_ids(
    builds: &BuildLedger,
) -> Result<Vec<rust_mutants::id::MutantId>, CompletionError> {
    projected_mutants(builds)
        .into_iter()
        .filter(|mutant| mutant.decision == Decision::Unnoticed)
        .map(|mutant| {
            rust_mutants::id::MutantId::try_from(mutant.id.as_str())
                .map_err(|_invalid| CompletionError::InvalidProjectedMutant { mutant: mutant.id })
        })
        .collect()
}

fn validate_model_batch(
    run_id: &rust_mutants::id::RunId,
    builds: &BuildLedger,
    batch: &ModelBatch,
) -> Result<(), CompletionError> {
    if batch.owner != *run_id {
        return Err(CompletionError::ModelOwner {
            expected: run_id.clone(),
            actual: batch.owner.clone(),
        });
    }
    let expected = expected_model_ids(builds)?;
    let actual = model_record_ids(&batch.records)?;
    if actual != expected {
        return Err(CompletionError::ModelBatch(ModelBatchError::Records {
            expected,
            actual,
        }));
    }
    Ok(())
}

impl Report {
    fn checked(report: Self) -> Result<Self, CompletionError> {
        validate_completion_shape(&report.run_id, &report.composition, &report.builds)?;
        reject_premature_model_outcomes(&report.builds)?;
        match (&report.contract, &report.model_completion) {
            (crate::config::Contract::VerifiedV1, ModelCompletion::Verified(batch)) => {
                validate_model_batch(&report.run_id, &report.builds, batch)?;
            }
            (crate::config::Contract::VerifiedV1, ModelCompletion::NotRequired) => {
                return Err(CompletionError::ModelRequired);
            }
            (
                crate::config::Contract::StandardV1 | crate::config::Contract::DeepV1,
                ModelCompletion::NotRequired,
            ) => {}
            (
                crate::config::Contract::StandardV1 | crate::config::Contract::DeepV1,
                ModelCompletion::Verified(_),
            ) => {
                return Err(CompletionError::ModelForbidden {
                    contract: report.contract,
                });
            }
        }
        let conclusion = report.conclusion()?;
        let violations = audit::validate_for_persistence_with_conclusion(&report, &conclusion);
        if violations.is_empty() {
            Ok(report)
        } else {
            Err(CompletionError::Contradictory { violations })
        }
    }

    /// The completed response namespace.
    #[must_use]
    pub fn run_id(&self) -> &str {
        self.run_id.as_str()
    }

    /// How much of the workspace was requested.
    #[must_use]
    pub const fn run_kind(&self) -> RunKind {
        self.run_kind
    }

    /// Run-wide fact provenance.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Reissues an immutable completed answer under a new response run while retaining every evidence namespace and measurement time that actually established it.
    ///
    /// # Errors
    /// Refuses model-complete evidence because its retained artifacts are owned by the original final-run namespace.
    pub fn read_back_as(
        mut self,
        run_id: &rust_mutants::id::RunId,
    ) -> Result<Self, CompletionError> {
        if let ModelCompletion::Verified(batch) = &self.model_completion {
            return Err(CompletionError::ModelReadBack {
                owner: batch.owner.clone(),
                requested: run_id.clone(),
            });
        }
        let source = std::mem::replace(&mut self.run_id, run_id.clone());
        self.provenance.facts = Established::ReadBackFrom(source.to_string());
        Self::checked(self)
    }

    /// Every configured build in the exact requested order.
    pub fn builds(&self) -> impl Iterator<Item = &BuildEvidence> {
        self.builds.iter()
    }

    /// Post-lattice model evidence.
    #[must_use]
    pub fn models(&self) -> &[ModelRecord] {
        match &self.model_completion {
            ModelCompletion::NotRequired => &[],
            ModelCompletion::Verified(batch) => &batch.records,
        }
    }

    /// Re-derives the complete presentation view from the build ledger.
    /// # Errors
    /// Returns [`CountError`] if the exact projection does not fit the v1 accounting counters.
    pub fn conclusion(&self) -> Result<Conclusion, CountError> {
        let mutants = projected_mutants_with_models(&self.builds, self.models());
        let findings = projected_findings(&self.builds, &self.global_findings, &mutants);
        Ok(Conclusion {
            verdict: concluded_from_projection(ConclusionProjection {
                run_kind: self.run_kind,
                shard: self.scope.shard.as_deref(),
                builds: &self.builds,
                mutants: &mutants,
                findings: &findings,
            }),
            timing: spanning_timing(&self.builds),
            accounting: projected_accounting(&self.builds, &mutants)?,
            resources: self
                .builds
                .iter()
                .flat_map(|build| build.baseline().resources.iter().cloned())
                .collect(),
            candidates: self
                .builds
                .iter()
                .flat_map(|build| {
                    build
                        .parts
                        .iter()
                        .flat_map(|part| part.candidates.iter().cloned())
                })
                .collect(),
            seams: self
                .builds
                .iter()
                .flat_map(|build| build.baseline().seams.iter().cloned())
                .collect(),
            faults: self
                .builds
                .iter()
                .flat_map(|build| build.parts.iter())
                .flat_map(|part| part.faults.iter().cloned())
                .collect(),
            beside: self
                .builds
                .iter()
                .flat_map(|build| build.parts.iter())
                .flat_map(|part| part.beside.iter().cloned())
                .collect(),
            targets: self
                .builds
                .iter()
                .flat_map(|build| build.baseline().targets.iter().cloned())
                .collect(),
            mutants,
            findings,
            limitations: self
                .builds
                .iter()
                .flat_map(|build| {
                    build
                        .parts
                        .iter()
                        .flat_map(|part| part.limitations.iter().cloned())
                        .chain(merged_drift_limitation(build))
                })
                .collect(),
            sources: self
                .builds
                .iter()
                .flat_map(|build| build.parts.iter())
                .flat_map(|part| {
                    part.sources
                        .iter()
                        .map(|(path, digest)| (path.clone(), digest.clone()))
                })
                .collect(),
        })
    }

    /// The verdict derived from every build row.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        let mutants = projected_mutants_with_models(&self.builds, self.models());
        let findings = projected_findings(&self.builds, &self.global_findings, &mutants);
        concluded_from_projection(ConclusionProjection {
            run_kind: self.run_kind,
            shard: self.scope.shard.as_deref(),
            builds: &self.builds,
            mutants: &mutants,
            findings: &findings,
        })
    }
}

/// One deliberately partial `K/N` document.
/// It cannot be passed where a completed [`Report`] is required; only shard merge consumes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShardReport {
    schema: String,
    schema_version: u32,
    run_id: rust_mutants::id::RunId,
    run_kind: RunKind,
    contract: crate::config::Contract,
    tool: Tool,
    repository: Repository,
    provenance: Provenance,
    scope: Scope,
    shard: ShardPart,
    builds: ShardBuildLedger,
    global_findings: Vec<Finding>,
}

/// Why one mutable shard measurement could not become an immutable shard document.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShardCompletionError {
    /// The final answer was not given a canonical writable run namespace.
    #[error(transparent)]
    RunId(#[from] rust_mutants::id::RunIdError),
    /// The configured-build shard ledger was incoherent.
    #[error(transparent)]
    Ledger(#[from] ShardBuildLedgerError),
    /// The mutable source did not name the shard retained by the typed ledger.
    #[error("source scope names {actual:?}, but the shard ledger retains {expected}")]
    ScopeShard {
        /// The source's text, if any.
        actual: Option<String>,
        /// The typed canonical shard.
        expected: String,
    },
    /// The immutable shard scope still carried the old optional string copy.
    #[error("a shard document carries its part as a typed field, not scope.shard")]
    DuplicateShardTruth,
    /// The requested ordered configured builds and evidence ledger differed.
    #[error("ordered build names {actual:?} differ from requested builds {expected:?}")]
    ConfiguredBuilds {
        /// The evidence order.
        actual: Vec<String>,
        /// The request order.
        expected: Vec<String>,
    },
    /// A source namespace was reused as the shard document's namespace.
    #[error("final run namespace {run_id} is also an evidence-source namespace")]
    EvidenceUsesFinalRun {
        /// The aliased final/source namespace.
        run_id: rust_mutants::id::RunId,
    },
    /// The wire names the wrong schema identity.
    #[error("shard schema identity must be {SHARD_SCHEMA} version {SCHEMA_VERSION}")]
    Schema,
    /// A finding was stored in a scope that did not own it.
    #[error("finding about {subject:?} has an origin that does not match its retained source")]
    FindingOrigin {
        /// The finding subject.
        subject: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShardReportWire {
    schema: String,
    schema_version: u32,
    run_id: rust_mutants::id::RunId,
    run_kind: RunKind,
    contract: crate::config::Contract,
    tool: Tool,
    repository: Repository,
    provenance: Provenance,
    scope: Scope,
    shard: ShardPart,
    builds: ShardBuildLedger,
    global_findings: Vec<Finding>,
}

impl<'de> Deserialize<'de> for ShardReport {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ShardReportWire::deserialize(deserializer)?;
        Self::checked(Self {
            schema: wire.schema,
            schema_version: wire.schema_version,
            run_id: wire.run_id,
            run_kind: wire.run_kind,
            contract: wire.contract,
            tool: wire.tool,
            repository: wire.repository,
            provenance: wire.provenance,
            scope: wire.scope,
            shard: wire.shard,
            builds: wire.builds,
            global_findings: wire.global_findings,
        })
        .map_err(serde::de::Error::custom)
    }
}

impl ShardReport {
    pub(crate) fn from_parts(
        template: &BuildReport,
        final_run_id: &rust_mutants::id::RunId,
        builds: ShardBuildLedger,
        global_findings: Vec<Finding>,
    ) -> Result<Self, ShardCompletionError> {
        let expected = CatalogPart::parse(template.scope.shard.as_deref())
            .map_err(ShardBuildLedgerError::from)?;
        let CatalogPart::Shard(expected) = expected else {
            return Err(ShardCompletionError::ScopeShard {
                actual: template.scope.shard.clone(),
                expected: format!("{}/{}", builds.shard().index(), builds.shard().of()),
            });
        };
        if expected != builds.shard() {
            return Err(ShardCompletionError::ScopeShard {
                actual: template.scope.shard.clone(),
                expected: format!("{}/{}", builds.shard().index(), builds.shard().of()),
            });
        }
        let mut scope = template.scope.clone();
        scope.shard = None;
        Self::checked(Self {
            schema: SHARD_SCHEMA.to_owned(),
            schema_version: SCHEMA_VERSION,
            run_id: final_run_id.clone(),
            run_kind: template.run_kind,
            contract: template.contract,
            tool: template.tool.clone(),
            repository: template.repository.clone(),
            provenance: template.provenance.clone(),
            scope,
            shard: expected,
            builds,
            global_findings,
        })
    }

    fn checked(report: Self) -> Result<Self, ShardCompletionError> {
        if report.schema != SHARD_SCHEMA || report.schema_version != SCHEMA_VERSION {
            return Err(ShardCompletionError::Schema);
        }
        if report.scope.shard.is_some() {
            return Err(ShardCompletionError::DuplicateShardTruth);
        }
        let names: Vec<String> = report
            .builds
            .iter()
            .map(|build| build.name.as_str().to_owned())
            .collect();
        if names != report.scope.configured_builds {
            return Err(ShardCompletionError::ConfiguredBuilds {
                actual: names,
                expected: report.scope.configured_builds,
            });
        }
        if report
            .builds
            .iter()
            .any(|build| build.source.run_id == report.run_id)
        {
            return Err(ShardCompletionError::EvidenceUsesFinalRun {
                run_id: report.run_id,
            });
        }
        if let Some(finding) = report
            .global_findings
            .iter()
            .find(|finding| !matches!(finding.origin, FindingOrigin::Global))
        {
            return Err(ShardCompletionError::FindingOrigin {
                subject: finding.subject.clone(),
            });
        }
        for build in report.builds.iter() {
            for finding in &build.source.findings {
                let valid = matches!(
                    &finding.origin,
                    FindingOrigin::Source {
                        build: origin_build_name,
                        run_id,
                        part,
                    } if origin_build_name == &build.name
                        && run_id == &build.source.run_id
                        && part == &build.source.part
                );
                if !valid {
                    return Err(ShardCompletionError::FindingOrigin {
                        subject: finding.subject.clone(),
                    });
                }
            }
        }
        Ok(report)
    }

    /// What this partial evidence can say without pretending to be complete.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        shard_verdict(&self.builds, &self.global_findings)
    }
}

/// Reconciliation before the contract-specific terminal phase.
///
/// Whole-catalog evidence remains non-durable until it takes the required model transition.
/// A shard is already terminal partial evidence and has no model API at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LatticedDocument {
    /// Whole-catalog evidence awaiting contract completion.
    Complete(LatticedReport),
    /// One intentionally partial shard, suitable only for merge.
    Shard(ShardReport),
}

/// The strict durable assurance-document boundary.
/// Complete and partial evidence are distinct variants and cannot be confused by a caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "document_type",
    content = "report",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum ReportDocument {
    /// A whole-catalog answer.
    Complete(Report),
    /// One intentionally partial shard, suitable only for merge.
    Shard(ShardReport),
}

impl ReportDocument {
    /// The final envelope namespace, distinct from all retained evidence source namespaces.
    #[must_use]
    pub fn run_id(&self) -> &str {
        match self {
            Self::Complete(report) => report.run_id.as_str(),
            Self::Shard(report) => report.run_id.as_str(),
        }
    }

    /// What this document can claim at its own completeness level.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        match self {
            Self::Complete(report) => report.verdict(),
            Self::Shard(report) => report.verdict(),
        }
    }

    pub(crate) fn model_artifacts(&self) -> Vec<&ModelArtifact> {
        match self {
            Self::Complete(Report {
                model_completion: ModelCompletion::Verified(batch),
                ..
            }) => batch.artifacts(),
            Self::Complete(Report {
                model_completion: ModelCompletion::NotRequired,
                ..
            })
            | Self::Shard(_) => Vec::new(),
        }
    }
}

fn shard_verdict(builds: &ShardBuildLedger, global_findings: &[Finding]) -> Verdict {
    if global_findings
        .iter()
        .any(|finding| finding.kind.is_defect())
        || builds
            .iter()
            .flat_map(|build| build.source.findings.iter())
            .any(|finding| finding.kind.is_defect())
    {
        return Verdict::Defect;
    }
    let answered = builds.iter().all(|build| {
        build
            .source
            .mutants
            .iter()
            .all(|row| row.outcome.outcome().answered(row.accepted))
    });
    let observed = builds.iter().all(|build| {
        build.source.accounting.targets.passed > 0 && build.source.accounting.mutants.executed > 0
    });
    let no_findings =
        global_findings.is_empty() && builds.iter().all(|build| build.source.findings.is_empty());
    let steady = builds.iter().all(|build| !moved(&build.source.drift));
    if answered && observed && no_findings && steady {
        Verdict::Partial
    } else {
        Verdict::Insufficient
    }
}

pub(crate) fn count_targets(targets: &[TargetRecord]) -> Result<TargetAccounting, CountError> {
    let mut counts = TargetAccounting {
        selected: count_of("target rows", targets.len())?,
        ..TargetAccounting::default()
    };
    for target in targets {
        let (field, count) = match target.status {
            TargetStatus::Passed => ("passed targets", &mut counts.passed),
            TargetStatus::Failed => ("failed targets", &mut counts.failed),
            TargetStatus::Skipped => ("skipped targets", &mut counts.skipped),
            TargetStatus::Missing => ("missing targets", &mut counts.missing),
        };
        increment(field, count)?;
    }
    Ok(counts)
}

#[derive(Clone, Copy)]
struct ConclusionProjection<'a> {
    run_kind: RunKind,
    shard: Option<&'a str>,
    builds: &'a BuildLedger,
    mutants: &'a [ProjectedMutant],
    findings: &'a [Finding],
}

fn concluded_from_projection(projection: ConclusionProjection<'_>) -> Verdict {
    let ConclusionProjection {
        run_kind,
        shard,
        builds,
        mutants,
        findings,
    } = projection;
    if findings.iter().any(|finding| finding.kind.is_defect()) {
        return Verdict::Defect;
    }
    let all_answered = mutants.iter().all(|mutant| {
        if matches!(
            mutant.decision,
            Decision::ModelNoticed | Decision::ModelProved
        ) {
            return true;
        }
        mutant
            .by_build
            .iter()
            .all(|fact| fact.decision.outcome().answered(fact.accepted))
    });
    let all_observed = builds.iter().all(|build| {
        build.baseline().accounting.targets.passed > 0
            && build
                .parts
                .iter()
                .any(|part| part.accounting.mutants.executed > 0)
    });
    let no_findings = findings.is_empty();
    if !all_answered || !all_observed || !no_findings {
        return Verdict::Insufficient;
    }
    if shard.is_some() {
        return Verdict::Partial;
    }
    match run_kind {
        RunKind::Full => Verdict::Assured,
        RunKind::Changed => Verdict::ChangeAssured,
        RunKind::Scoped => Verdict::ScopeAssured,
    }
}

fn projected_mutants(builds: &BuildLedger) -> Vec<ProjectedMutant> {
    let mut projected: Vec<_> = builds
        .first()
        .parts
        .iter()
        .flat_map(|part| {
            part.mutants.iter().map(|row| ProjectedMutant {
                catalog_index: row.catalog_index,
                id: row.id.clone(),
                display_id: row.display_id.clone(),
                path: row.path.clone(),
                position: row.position,
                rule: row.rule.clone(),
                item: row.item.clone(),
                original: row.original.clone(),
                replacement: row.replacement.clone(),
                decision: row.outcome.decision(),
                by_build: vec![BuildMutationDecision {
                    build: builds.first.name.clone(),
                    run_id: part.run_id.clone(),
                    part: part.part,
                    decision: row.outcome.clone(),
                    accepted: row.accepted,
                    routing: row.routing.clone(),
                    reuse: row.reuse.clone(),
                }],
                blind_in: Vec::new(),
            })
        })
        .collect();

    for build in &builds.rest {
        let rows = build.parts.iter().flat_map(|part| {
            part.mutants
                .iter()
                .map(move |row| (part.run_id.clone(), part.part, row))
        });
        for (projection, (run_id, part, row)) in projected.iter_mut().zip(rows) {
            let candidate = row.outcome.decision();
            if candidate.standing() < projection.decision.standing() {
                projection.decision = candidate;
            }
            projection.by_build.push(BuildMutationDecision {
                build: build.name.clone(),
                run_id,
                part,
                decision: row.outcome.clone(),
                accepted: row.accepted,
                routing: row.routing.clone(),
                reuse: row.reuse.clone(),
            });
        }
    }

    if builds.len() > 1 {
        for projection in &mut projected {
            let mut facts = projection.by_build.iter();
            let Some(first) = facts.next() else {
                continue;
            };
            let rest: Vec<_> = facts
                .map(|fact| (&fact.build, fact.decision.outcome().decision()))
                .collect();
            let resolved =
                across::across((&first.build, first.decision.outcome().decision()), &rest);
            projection.decision = resolved.decision();
            projection.blind_in = resolved
                .blind_in()
                .iter()
                .filter(|blind| {
                    projection.by_build.iter().any(|fact| {
                        fact.build == blind.build
                            && !fact.decision.outcome().answered(fact.accepted)
                    })
                })
                .cloned()
                .collect();
        }
    }
    projected
}

fn projected_mutants_with_models(
    builds: &BuildLedger,
    models: &[ModelRecord],
) -> Vec<ProjectedMutant> {
    let mut projected = projected_mutants(builds);
    let answers: BTreeMap<_, _> = models
        .iter()
        .map(|model| (model.mutant(), model.answer()))
        .collect();
    for mutant in &mut projected {
        let Some(answer) = answers.get(mutant.id.as_str()) else {
            continue;
        };
        match answer {
            ModelDecision::Noticed { .. } => {
                mutant.decision = Decision::ModelNoticed;
                mutant.blind_in.clear();
            }
            ModelDecision::Proved { .. } => {
                mutant.decision = Decision::ModelProved;
                mutant.blind_in.clear();
            }
            ModelDecision::Ineligible { .. } | ModelDecision::Undecided { .. } => {}
        }
    }
    projected
}

fn projected_findings(
    builds: &BuildLedger,
    global_findings: &[Finding],
    mutants: &[ProjectedMutant],
) -> Vec<Finding> {
    let affirmative: BTreeSet<(&str, &str)> = mutants
        .iter()
        .filter(|mutant| {
            matches!(
                mutant.decision,
                Decision::ModelNoticed | Decision::ModelProved
            )
        })
        .map(|mutant| (mutant.id.as_str(), mutant.display_id.as_str()))
        .collect();
    let mut projected = global_findings.to_vec();
    for build in builds.iter() {
        projected.extend(merged_drift_findings(build));
        for part in build.parts.iter() {
            for finding in &part.findings {
                let answered_by_model = finding.kind == FindingKind::SurvivingMutant
                    && affirmative.iter().any(|(full, display)| {
                        finding.subject == *full || finding.subject == *display
                    });
                if answered_by_model {
                    continue;
                }
                projected.push(finding.clone());
            }
        }
    }
    projected
}

/// Whether a build was measured in parts, which is when its drift finding and limitation are raised over the combined records rather than by a part.
fn sharded(build: &BuildEvidence) -> bool {
    build
        .parts
        .iter()
        .any(|part| matches!(part.part, CatalogPart::Shard(_)))
}

/// The `unstable-baseline` findings of a build measured in parts, over every part's records and rows, each attributed to the part that saw its target move.
fn merged_drift_findings(build: &BuildEvidence) -> Vec<Finding> {
    if !sharded(build) {
        return Vec::new();
    }
    let records = drift::combined(build.parts.iter().flat_map(|part| part.drift.iter()));
    let rows: Vec<MutantRecord> = build
        .parts
        .iter()
        .flat_map(|part| part.mutants.iter().cloned())
        .collect();
    drift::found(&records, &rows)
        .into_iter()
        .map(|mut finding| {
            let saw = build.parts.iter().find(|part| {
                part.drift.iter().any(|one| {
                    matches!(one, drift::Drift::Moved { .. }) && one.target() == finding.subject
                })
            });
            if let Some(part) = saw {
                finding.origin = FindingOrigin::Source {
                    build: build.name.clone(),
                    run_id: part.run_id.clone(),
                    part: part.part,
                };
            }
            finding
        })
        .collect()
}

/// The `drift-not-measured` limitation of a build measured in parts, over every part's records.
fn merged_drift_limitation(build: &BuildEvidence) -> Option<Limitation> {
    if !sharded(build) {
        return None;
    }
    drift::unmeasured(&drift::combined(
        build.parts.iter().flat_map(|part| part.drift.iter()),
    ))
}

fn spanning_timing(builds: &BuildLedger) -> ConclusionTiming {
    builds.timing.clone()
}

fn projected_accounting(
    builds: &BuildLedger,
    mutants: &[ProjectedMutant],
) -> Result<ConclusionAccounting, CountError> {
    let targets: Vec<TargetRecord> = builds
        .iter()
        .flat_map(|build| build.baseline().targets.iter().cloned())
        .collect();
    let soundness_by_build = builds
        .iter()
        .map(|build| BuildSoundness {
            build: build.name.clone(),
            accounting: build.baseline().accounting.soundness,
        })
        .collect();
    let faulted: Vec<faults::FaultRecord> = builds
        .iter()
        .flat_map(|build| build.parts.iter())
        .flat_map(|part| part.faults.iter().cloned())
        .collect();
    Ok(ConclusionAccounting {
        targets: count_targets(&targets)?,
        mutants: count_projected_mutants(builds.catalog_count(), mutants)?,
        soundness_by_build,
        faults: faults::FaultAccounting::of(&faulted)?,
    })
}

fn count_projected_mutants(
    cataloged: u32,
    mutants: &[ProjectedMutant],
) -> Result<MutantAccounting, CountError> {
    let mut counts = MutantAccounting {
        cataloged,
        ..MutantAccounting::default()
    };
    for mutant in mutants {
        counts.observers.counted(mutant.decision)?;
        let accepted_answer = mutant
            .by_build
            .iter()
            .all(|fact| fact.decision.outcome().answered(fact.accepted))
            && mutant.by_build.iter().any(|fact| fact.accepted);
        if accepted_answer {
            increment("accepted projected mutations", &mut counts.accepted)?;
        }
        let reused = mutant
            .by_build
            .iter()
            .any(|fact| fact.reuse().0.read_back().is_some());
        match mutant.decision {
            Decision::Types => increment("rejected projected mutations", &mut counts.rejected)?,
            Decision::Tests => {
                increment("executed projected mutations", &mut counts.executed)?;
                increment("killed projected mutations", &mut counts.killed)?;
                if reused {
                    increment("reused killed mutations", &mut counts.reused_killed)?;
                }
            }
            Decision::ModelNoticed => {
                increment("executed projected mutations", &mut counts.executed)?;
                increment(
                    "model-noticed projected mutations",
                    &mut counts.model_noticed,
                )?;
            }
            Decision::ModelProved => {
                increment("executed projected mutations", &mut counts.executed)?;
                increment("model-proved projected mutations", &mut counts.model_proved)?;
            }
            Decision::Proved => {
                increment("equivalent projected mutations", &mut counts.equivalent)?;
            }
            Decision::Unnoticed => {
                increment("executed projected mutations", &mut counts.executed)?;
                increment("surviving projected mutations", &mut counts.survived)?;
                if reused {
                    increment("reused surviving mutations", &mut counts.reused_survived)?;
                }
            }
            Decision::Unreached => {
                increment("unreached projected mutations", &mut counts.unreached)?;
            }
            Decision::StepLimitReached => {
                increment("executed projected mutations", &mut counts.executed)?;
                increment(
                    "step-limit-reached projected mutations",
                    &mut counts.step_limit_reached,
                )?;
            }
            Decision::Waited => {
                increment("executed projected mutations", &mut counts.executed)?;
                increment("waited projected mutations", &mut counts.waited)?;
            }
            Decision::Errored => {
                increment("executed projected mutations", &mut counts.executed)?;
            }
        }
    }
    Ok(counts)
}
