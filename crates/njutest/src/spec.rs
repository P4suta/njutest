// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run established each item of the source pins, and what it leaves free.

use std::collections::{BTreeMap, BTreeSet};

use crate::report::{
    Answered, BuildMutationDecision, Decided, Decision, Discharged, Established, ProjectedMutant,
    Report, Routing, RunKind,
};

/// Which items of a run a reader asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// Every item the run made a change in.
    Everything,
    /// The items of one file.
    File(String),
    /// The items of that name wherever they are, and the items inside them.
    Item(String),
    /// The items of that name in one file, and the items inside them.
    InFile {
        /// The file, from the project's root.
        path: String,
        /// The item, as the source names it.
        item: String,
    },
}

impl Subject {
    /// What `named` asks about, or every item when nothing was named.
    #[must_use]
    pub fn parse(named: Option<&str>) -> Self {
        let Some(named) = named.map(str::trim).filter(|named| !named.is_empty()) else {
            return Self::Everything;
        };
        if let Some((path, item)) = in_file(named) {
            return Self::InFile {
                path: path.to_owned(),
                item: item.to_owned(),
            };
        }
        if named.contains('/')
            || std::path::Path::new(named)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
        {
            return Self::File(named.to_owned());
        }
        Self::Item(named.to_owned())
    }

    /// Whether a change at `path`, inside `item`, is one this asks about.
    #[must_use]
    pub fn names(&self, path: &str, item: &str) -> bool {
        match self {
            Self::Everything => true,
            Self::File(file) => at(path, file),
            Self::Item(wanted) => within(item, wanted),
            Self::InFile {
                path: file,
                item: wanted,
            } => at(path, file) && within(item, wanted),
        }
    }

    /// The subject as the reader typed it, or nothing for every item.
    #[must_use]
    pub fn typed(&self) -> Option<String> {
        match self {
            Self::Everything => None,
            Self::File(named) | Self::Item(named) => Some(named.clone()),
            Self::InFile { path, item } => Some(format!("{path}:{item}")),
        }
    }
}

/// The file and the item of `named` where it is written `PATH:ITEM`, telling the one colon between them from the two inside a path.
fn in_file(named: &str) -> Option<(&str, &str)> {
    named.match_indices(':').find_map(|(at, _)| {
        let (path, rest) = named.split_at_checked(at)?;
        let item = rest.strip_prefix(':')?;
        (!path.is_empty() && !item.is_empty() && !path.ends_with(':') && !item.starts_with(':'))
            .then_some((path, item))
    })
}

/// Whether `path` is the file `wanted` names, whole or by its last components.
fn at(path: &str, wanted: &str) -> bool {
    path == wanted
        || path
            .strip_suffix(wanted)
            .is_some_and(|before| before.ends_with('/'))
}

/// Whether `item` is the item `wanted` names or one inside it, segment by segment, so `retry` is not `retry_all`.
fn within(item: &str, wanted: &str) -> bool {
    let segments: Vec<&str> = item.split("::").collect();
    let wanted: Vec<&str> = wanted.split("::").collect();
    segments
        .windows(wanted.len())
        .any(|window| window == wanted.as_slice())
}

/// What a run established about every change it made to the items a subject names.
///
/// It holds the report's own rows and nothing beside them: where a change stands and what each build established are read off those rows when they are asked for, so no second account of a row exists to disagree with the first, and a specification can only be made from a report.
#[derive(Debug, Clone)]
pub struct Specification {
    run: String,
    kind: RunKind,
    builds: Vec<String>,
    items: Vec<Item>,
}

impl Specification {
    /// The run it was read from.
    #[must_use]
    pub fn run(&self) -> &str {
        &self.run
    }

    /// How much of the workspace that run asked about.
    #[must_use]
    pub const fn kind(&self) -> RunKind {
        self.kind
    }

    /// The builds that run measured, in the order it measured them.
    #[must_use]
    pub fn builds(&self) -> &[String] {
        &self.builds
    }

    /// Every item the subject names that the run made a change in, by path and then by where it starts.
    #[must_use]
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// The lines of the file at `path` the run changed, each with every change that starts on it, in the order the file has them.
    ///
    /// The path is the whole path from the project's root, so a line belongs to one file.
    #[must_use]
    pub fn lines(&self, path: &str) -> Vec<Line<'_>> {
        let mut starting: BTreeMap<u32, Vec<&Change>> = BTreeMap::new();
        for item in self.items.iter().filter(|item| item.path == path) {
            for change in &item.changes {
                starting.entry(change.line()).or_default().push(change);
            }
        }
        starting
            .into_iter()
            .filter_map(|(number, changes)| {
                let mut changes = changes.into_iter();
                changes.next().map(|first| Line {
                    number,
                    first,
                    rest: changes.collect(),
                })
            })
            .collect()
    }
}

/// One line of the source and every change the run made that starts on it.
///
/// It holds its first change apart from the rest, so a line with nothing on it is not a value there is.
#[derive(Debug, Clone)]
pub struct Line<'a> {
    number: u32,
    first: &'a Change,
    rest: Vec<&'a Change>,
}

impl<'a> Line<'a> {
    /// Which line of the file it is.
    #[must_use]
    pub const fn number(&self) -> u32 {
        self.number
    }

    /// Every change that starts on it, in the order the source has them.
    pub fn changes(&self) -> impl Iterator<Item = &'a Change> + '_ {
        std::iter::once(self.first).chain(self.rest.iter().copied())
    }

    /// How its weakest change was decided, which is what the line as a whole stands on.
    #[must_use]
    pub fn decision(&self) -> Decision {
        self.rest
            .iter()
            .fold(self.first.decision(), |weakest, change| {
                weakest.weaker(change.decision())
            })
    }

    /// Where the line stands, which is where its weakest change stands.
    #[must_use]
    pub fn section(&self) -> Section {
        Section::of(self.decision())
    }
}

/// One item of the source and every change the run made inside it.
#[derive(Debug, Clone)]
pub struct Item {
    path: String,
    name: String,
    changes: Vec<Change>,
}

impl Item {
    /// The file, from the project's root.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The item, as the source names it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Every change, in the order the source has them.
    #[must_use]
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }

    /// The first line anything was changed on.
    fn starts(&self) -> u32 {
        self.changes
            .iter()
            .map(Change::line)
            .min()
            .unwrap_or(u32::MAX)
    }
}

/// One change the run made: the report's own projection of that mutation across every build, beside the targets each build found its baseline reach moved for.
#[derive(Debug, Clone)]
pub struct Change {
    mutant: ProjectedMutant,
    moved: BTreeMap<String, BTreeSet<String>>,
}

impl Change {
    /// What was done to the code.
    #[must_use]
    pub fn edit(&self) -> Edit<'_> {
        if self.mutant.replacement().trim().is_empty() {
            Edit::Deleted {
                was: self.mutant.original(),
            }
        } else {
            Edit::Replaced {
                was: self.mutant.original(),
                now: self.mutant.replacement(),
            }
        }
    }

    /// Where the change stands across every build, which is the section of the decision the report's own lattice took over them.
    #[must_use]
    pub const fn section(&self) -> Section {
        Section::of(self.mutant.decision())
    }

    /// How the builds together decided it, which is the weakest of what each established.
    #[must_use]
    pub const fn decision(&self) -> Decision {
        self.mutant.decision()
    }

    /// What each build established, in the order the builds were measured.
    pub fn answers(&self) -> impl Iterator<Item = Answer<'_>> {
        self.mutant.by_build().iter().map(|row| Answer {
            row,
            moved: self.moved.get(row.build().as_str()),
        })
    }

    /// How a reader names the change again after they have edited the file.
    #[must_use]
    pub fn locator(&self) -> String {
        crate::naming::locator(&self.mutant)
    }

    /// The line it starts on.
    #[must_use]
    pub const fn line(&self) -> u32 {
        self.mutant.position().line
    }
}

/// What a change did to the code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit<'a> {
    /// The code became something else.
    Replaced {
        /// What the code says.
        was: &'a str,
        /// What the run made it say.
        now: &'a str,
    },
    /// The code was taken out.
    Deleted {
        /// What the code says.
        was: &'a str,
    },
}

/// Where a change stands across every build a run measured, in the order a page lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Section {
    /// Every build noticed it or found it to be the same program, and at least one noticed it.
    Pinned,
    /// Some build noticed nothing of it, and every build established something.
    Free,
    /// Every build found it to be the same program.
    Same,
    /// Some build established nothing about it.
    Unsettled,
}

impl Section {
    /// Where a change the builds together decided as `decision` stands.
    #[must_use]
    pub const fn of(decision: Decision) -> Self {
        match decision {
            Decision::Types | Decision::Tests | Decision::ModelNoticed => Self::Pinned,
            Decision::Unnoticed | Decision::Unreached => Self::Free,
            Decision::Proved | Decision::ModelProved => Self::Same,
            Decision::StepLimitReached | Decision::Waited | Decision::Errored => Self::Unsettled,
        }
    }
}

/// What one build established about one change: that build's row of the report.
#[derive(Debug, Clone, Copy)]
pub struct Answer<'a> {
    row: &'a BuildMutationDecision,
    moved: Option<&'a BTreeSet<String>>,
}

impl<'a> Answer<'a> {
    /// The build.
    #[must_use]
    pub fn build(self) -> &'a str {
        self.row.build().as_str()
    }

    /// What the build established, with the names its route recorded.
    #[must_use]
    pub fn held(self) -> Held {
        Held::of(
            self.row.outcome(),
            self.row.accepted(),
            Recorded::of(self.row.routing(), self.established()),
        )
    }

    /// Whether this run established it, or read it back from another.
    #[must_use]
    pub const fn established(self) -> &'a Established {
        &self.row.reuse().0
    }

    /// The targets whose baseline reach moved on a control and on which what this build established rests: none for anything but a change left free, because a kill rests on no reach.
    #[must_use]
    pub fn unfounded(self) -> Vec<String> {
        let Some(moved) = self.moved else {
            return Vec::new();
        };
        match self.held() {
            Held::Free { .. } => moved
                .iter()
                .filter(|target| crate::report::drift::rests_on(self.row.routing(), target))
                .cloned()
                .collect(),
            Held::Pinned(_) | Held::Same(_) | Held::Unsettled(_) => Vec::new(),
        }
    }
}

/// What one build established about one change, with the names a reader goes and looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Held {
    /// Something noticed it.
    Pinned(Pin),
    /// It is a different program and nothing noticed it.
    Free {
        /// Why nothing did.
        free: Free,
        /// Whether a reviewer accepted it.
        accepted: bool,
    },
    /// It is not a different program.
    Same(Same),
    /// Nothing was established.
    Unsettled(Unsettled),
}

/// What noticed a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pin {
    /// A target's tests failed on it.
    Tests {
        /// The target.
        by: String,
        /// Who else was asked, and who reaches it and was not.
        asked: Asked,
    },
    /// The compiler refused it.
    Types,
    /// The model checker found an input that tells the two apart.
    Model,
}

/// Who else a run asked about a change a target noticed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// This run asked in order and recorded every answer.
    Here {
        /// The targets asked before the one that noticed, each with what it answered.
        before: Vec<Answered>,
        /// The targets that reach it and were never asked, because one asked before them noticed.
        unasked: Vec<String>,
    },
    /// The record does not say who else was asked, because the answer was inherited or read back.
    Unrecorded,
}

/// Why nothing noticed a change that makes a different program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Free {
    /// Every target asked ran it and none noticed; the others that reach it a proof removed.
    Ran {
        /// The targets that ran it.
        answered: Vec<String>,
        /// The targets that reach it and a proof removed.
        removed: Vec<Discharged>,
    },
    /// A proof removed every target that reaches it, so nothing ran it.
    Removed(Vec<Discharged>),
    /// Nothing the suite runs executes it.
    Never,
    /// Nothing noticed, and the record does not say who ran it, because the answer was inherited or read back.
    Unrecorded,
}

/// Why a change is not a different program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Same {
    /// The compiler rendered it identically.
    Compiled,
    /// The model checker proved the two equal throughout its closed domain.
    Model,
}

/// Why nothing was established about a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsettled {
    /// The target crossed its step allowance without a control verdict.
    StepLimit {
        /// The target.
        on: String,
        /// The count it was stopped at.
        observed: u64,
    },
    /// This machine stopped waiting for the target.
    Waited {
        /// The target.
        on: String,
    },
    /// The target did not answer the same way twice.
    Unconfirmed {
        /// The target.
        on: String,
    },
    /// Nothing could be measured on the target.
    Errored {
        /// The target.
        on: String,
    },
}

/// Why a run could not be specified.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SpecError {
    /// The run made no change the subject names.
    #[error(
        "{}: run {run}, {kind}, made no change {within}",
        crate::error::SUBJECT_NOT_CATALOGED.code
    )]
    NamesNothing {
        /// The run.
        run: String,
        /// How much of the workspace it asked about.
        kind: &'static str,
        /// Where it made none, as the subject was typed.
        within: String,
    },
    /// The report's projection does not fit its counters.
    #[error("{}: {source}", crate::error::REPORT_UNSOUND.code)]
    Unsound {
        /// What did not fit.
        #[from]
        source: crate::report::CountError,
    },
}

impl SpecError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> crate::error::ErrorCode {
        match self {
            Self::NamesNothing { .. } => crate::error::SUBJECT_NOT_CATALOGED,
            Self::Unsound { .. } => crate::error::REPORT_UNSOUND,
        }
    }
}

/// What `report` established about every change it made in the file at `typed`, which names one file by its whole path from the project's root however it is spelled.
///
/// # Errors
/// [`SpecError::NamesNothing`] when the run changed nothing in exactly that file, and [`SpecError::Unsound`] as [`specified`] refuses.
pub fn guarded(report: &Report, typed: &str) -> Result<(Specification, String), SpecError> {
    let nothing = || SpecError::NamesNothing {
        run: report.run_id().to_owned(),
        kind: kind(report.run_kind()),
        within: format!("in {typed}"),
    };
    let Ok(path) = rust_mutants::id::normalize_path(typed) else {
        return Err(nothing());
    };
    let specification = specified(report, &Subject::File(path.clone()))?;
    if specification.lines(&path).is_empty() {
        return Err(nothing());
    }
    Ok((specification, path))
}

/// What `report` established about every change it made to the items `subject` names.
///
/// # Errors
/// [`SpecError::NamesNothing`] when the run made no change the subject names, and [`SpecError::Unsound`] when the report's projection does not fit its counters.
pub fn specified(report: &Report, subject: &Subject) -> Result<Specification, SpecError> {
    let conclusion = report.conclusion()?;
    let moved: BTreeMap<String, BTreeSet<String>> = report
        .builds()
        .map(|build| {
            let targets = build
                .drift()
                .iter()
                .filter_map(|one| match one {
                    crate::report::drift::Drift::Moved { target, .. } => Some(target.clone()),
                    crate::report::drift::Drift::Held { .. }
                    | crate::report::drift::Drift::NotMeasured { .. } => None,
                })
                .collect();
            (build.name().as_str().to_owned(), targets)
        })
        .collect();
    let mut named: BTreeMap<(String, String), Vec<Change>> = BTreeMap::new();
    for mutant in conclusion.mutants {
        if subject.names(mutant.path(), mutant.item()) {
            named
                .entry((mutant.path().to_owned(), mutant.item().to_owned()))
                .or_default()
                .push(Change {
                    mutant,
                    moved: moved.clone(),
                });
        }
    }
    if named.is_empty() {
        return Err(SpecError::NamesNothing {
            run: report.run_id().to_owned(),
            kind: kind(report.run_kind()),
            within: subject.typed().map_or_else(
                || "anywhere".to_owned(),
                |typed| format!("in anything `{typed}` names"),
            ),
        });
    }
    let mut items: Vec<Item> = named
        .into_iter()
        .map(|((path, name), mut changes)| {
            changes.sort_by_cached_key(|change| (change.line(), change.locator()));
            Item {
                path,
                name,
                changes,
            }
        })
        .collect();
    items.sort_by(|one, other| {
        (&one.path, one.starts(), &one.name).cmp(&(&other.path, other.starts(), &other.name))
    });
    Ok(Specification {
        run: report.run_id().to_owned(),
        kind: report.run_kind(),
        builds: report
            .builds()
            .map(|build| build.name().as_str().to_owned())
            .collect(),
        items,
    })
}

/// How much of the workspace a run asked about, in the words a sentence about it uses, so a reader of part of a workspace is not told it is the whole.
#[must_use]
pub const fn kind(kind: RunKind) -> &'static str {
    match kind {
        RunKind::Full => "a full run",
        RunKind::Changed => "a changed run, which asked only about what changed",
        RunKind::Scoped => "a scoped run, which asked only about what it was scoped to",
    }
}

/// The route a row carries, where this run asked by it and kept every answer.
#[derive(Clone, Copy)]
enum Recorded<'a> {
    /// This run routed it and asked by that route.
    Asked(&'a Routing),
    /// The answer was inherited or read back, so the route, if any, is not the one it was asked by.
    Not,
}

impl<'a> Recorded<'a> {
    /// What a row's `routing` and provenance say about who was asked.
    const fn of(routing: Option<&'a Routing>, established: &Established) -> Self {
        match (routing, established) {
            (Some(routing), Established::Here) => Self::Asked(routing),
            (None, _) | (Some(_), Established::ReadBackFrom(_)) => Self::Not,
        }
    }
}

impl Held {
    /// What a build that decided `decided` established, with the names its route recorded.
    fn of(decided: &Decided, accepted: bool, recorded: Recorded<'_>) -> Self {
        match decided {
            Decided::CompileRejected => Self::Pinned(Pin::Types),
            Decided::Killed { by } => Self::Pinned(Pin::Tests {
                by: by.clone(),
                asked: Asked::of(by, recorded),
            }),
            Decided::ModelNoticed => Self::Pinned(Pin::Model),
            Decided::ModelProved => Self::Same(Same::Model),
            Decided::Equivalent => Self::Same(Same::Compiled),
            Decided::Survived => Self::Free {
                free: Free::of(recorded),
                accepted,
            },
            Decided::Unreached => Self::Free {
                free: Free::Never,
                accepted,
            },
            Decided::StepLimitReached { on, boundary } => Self::Unsettled(Unsettled::StepLimit {
                on: on.clone(),
                observed: boundary.observed(),
            }),
            Decided::Waited { on } => Self::Unsettled(Unsettled::Waited { on: on.clone() }),
            Decided::Unconfirmed { on } => {
                Self::Unsettled(Unsettled::Unconfirmed { on: on.clone() })
            }
            Decided::Errored { on } => Self::Unsettled(Unsettled::Errored { on: on.clone() }),
        }
    }
}

impl Asked {
    /// Who else the run asked about a mutation `by` noticed, and who reaches it and was not asked.
    fn of(by: &str, recorded: Recorded<'_>) -> Self {
        let Recorded::Asked(routing) = recorded else {
            return Self::Unrecorded;
        };
        let Some(noticed) = routing.answered.iter().position(|one| one.target == by) else {
            return Self::Unrecorded;
        };
        let before: Vec<Answered> = routing.answered.iter().take(noticed).cloned().collect();
        let unasked = routing
            .reaching
            .iter()
            .filter(|target| {
                target.as_str() != by && !routing.answered.iter().any(|one| &one.target == *target)
            })
            .cloned()
            .collect();
        Self::Here { before, unasked }
    }
}

impl Free {
    /// Who ran a mutation nothing noticed, and what removed the rest of those that reach it.
    fn of(recorded: Recorded<'_>) -> Self {
        let Recorded::Asked(routing) = recorded else {
            return Self::Unrecorded;
        };
        let answered: Vec<String> = routing
            .answered
            .iter()
            .map(|one| one.target.clone())
            .collect();
        let removed: Vec<Discharged> = routing.discharged.clone();
        match (answered.is_empty(), removed.is_empty()) {
            (false, _) => Self::Ran { answered, removed },
            (true, false) => Self::Removed(removed),
            (true, true) => Self::Unrecorded,
        }
    }
}
