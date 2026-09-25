// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What git says about the tree, and what it is never allowed to say by silence.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use crate::glob::{GlobError, Pattern};
use crate::runner::{Bound, Spec, Watch, run};

/// The revision a change set is computed against when the caller names none.
pub const DEFAULT_BASE: &str = "HEAD";

/// The pattern a run about an empty change set mutates within.
/// No file is called this.
pub const NOTHING_CHANGED: &str = ".rust-mutants-nothing-changed";

/// The most a short answer, a revision or a list of names, is read to.
const ANSWER_LIMIT: usize = 1 << 20;

/// The most a diff is read to; one longer than this is refused rather than read in part.
const DIFF_LIMIT: usize = 1 << 26;

/// Where to ask git, with what environment, and under whose watch.
#[derive(Debug)]
pub struct Asking<'a, W> {
    /// The directory the commands run in.
    pub root: &'a Path,
    /// The environment the commands run with.
    pub env: &'a [(OsString, OsString)],
    /// Directories whose contents are written by a run rather than verified by one, as workspace-relative prefixes.
    pub excluded: &'a [&'a str],
    /// What stops the commands, and who hears that they ran.
    pub watch: &'a W,
}

/// What the repository was when it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The commit `HEAD` resolves to.
    pub commit: String,
    /// The branch `HEAD` is on, or `HEAD` when it is detached.
    pub branch: String,
    /// Whether anything in the tree differs from that commit.
    pub dirty: bool,
}

/// What differs from a revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The revision the change set was computed against.
    pub base: String,
    /// The commit both branches share, when git could name one.
    pub merge_base: Option<String>,
    /// Every file that differs, as workspace-relative paths, sorted.
    /// What is not committed counts too.
    pub files: Vec<String>,
}

/// What the repository at `root` was, or nothing when git could not be asked.
#[must_use]
pub fn facts<W: Watch>(asking: &Asking<'_, W>) -> Option<Facts> {
    let commit = ask(
        asking,
        Empty::Refuse,
        Shape::Trimmed,
        &["rev-parse", "HEAD"],
    )?;
    let branch = ask(
        asking,
        Empty::Refuse,
        Shape::Trimmed,
        &["rev-parse", "--abbrev-ref", "HEAD"],
    )?;
    let status = ask(
        asking,
        Empty::Accept,
        Shape::Verbatim,
        &["status", "--porcelain"],
    )?;
    Some(Facts {
        commit,
        branch,
        dirty: !status.trim().is_empty(),
    })
}

/// Every file that differs from `base`, committed and not, leaving out anything under an excluded directory.
#[must_use]
pub fn changed<W: Watch>(asking: &Asking<'_, W>, base: &str) -> Option<Change> {
    let merge_base = ask(
        asking,
        Empty::Refuse,
        Shape::Trimmed,
        &["merge-base", base, "HEAD"],
    );
    let against = match merge_base.as_deref() {
        Some(merge_base) => merge_base,
        None => base,
    };
    let committed = ask(
        asking,
        Empty::Accept,
        Shape::Trimmed,
        &["diff", "--name-only", against],
    )?;
    let uncommitted = ask(
        asking,
        Empty::Accept,
        Shape::Verbatim,
        &["status", "--porcelain"],
    )?;
    let mut files: Vec<String> = committed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    let uncommitted_paths: Option<Vec<String>> = uncommitted.lines().map(porcelain_path).collect();
    files.extend(uncommitted_paths?);
    files.retain(|path| !is_under(path, asking.excluded));
    files.sort();
    files.dedup();
    Some(Change {
        base: base.to_owned(),
        merge_base,
        files,
    })
}

/// What a change set leaves to mutate within what a configuration includes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Within {
    /// The changed Rust files the configuration includes, as the patterns a run mutates within.
    Changed(Vec<Pattern>),
    /// No changed file is a Rust file the configuration includes; these are the files that did change.
    Nothing {
        /// Every changed path, none of them one the configuration measures.
        changed: Vec<String>,
    },
}

impl Within {
    /// The patterns a run mutates within, where a change that touched nothing measured is a pattern no file matches.
    ///
    /// # Errors
    /// The sentinel pattern failing to compile, which it does not.
    pub fn patterns(self) -> Result<Vec<Pattern>, GlobError> {
        match self {
            Self::Changed(patterns) => Ok(patterns),
            Self::Nothing { .. } => Pattern::compile(NOTHING_CHANGED).map(|pattern| vec![pattern]),
        }
    }
}

/// The lines a change set left in the files under the root, as the new side of each file counts them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lines {
    /// Every file with a changed line, relative to the root, and which of its lines changed.
    pub files: BTreeMap<String, Touched>,
}

/// Which lines of one file a change set left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Touched {
    /// Every line, because git does not track the file.
    Whole,
    /// These inclusive ranges of lines, in the order the diff gives them.
    Ranges(Vec<(u32, u32)>),
}

impl Lines {
    /// Whether the change left `line` of `path`, a path relative to the root.
    #[must_use]
    pub fn touches(&self, path: &str, line: u32) -> bool {
        match self.files.get(path) {
            None => false,
            Some(Touched::Whole) => true,
            Some(Touched::Ranges(ranges)) => ranges
                .iter()
                .any(|(first, last)| (*first..=*last).contains(&line)),
        }
    }
}

/// The lines that differ from `base`, committed and not, in the files under the root, or nothing when git could not say every one of them.
#[must_use]
pub fn lines<W: Watch>(asking: &Asking<'_, W>, base: &str) -> Option<Lines> {
    let merge_base = ask(
        asking,
        Empty::Refuse,
        Shape::Trimmed,
        &["merge-base", base, "HEAD"],
    );
    let against = merge_base.as_deref().unwrap_or(base);
    let diff = ask_up_to(
        asking,
        (Empty::Accept, Shape::Verbatim, DIFF_LIMIT),
        &[
            "-c",
            "core.quotepath=false",
            "diff",
            "--relative",
            "--unified=0",
            "--no-color",
            "--no-ext-diff",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            against,
        ],
    )?;
    let untracked = ask(
        asking,
        Empty::Accept,
        Shape::Verbatim,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    let mut files = hunks(&diff)?;
    for path in untracked.split('\0').filter(|path| !path.is_empty()) {
        files.insert(path.to_owned(), Touched::Whole);
    }
    files.retain(|path, _touched| !is_under(path, asking.excluded));
    Some(Lines { files })
}

/// Where a `--unified=0` diff is: in a file's header, or in its hunks, where a line that looks like a header is content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    Header,
    Hunks,
}

/// The new-side line ranges of a `--unified=0` diff, by file, or nothing when a header cannot be read exactly.
fn hunks(diff: &str) -> Option<BTreeMap<String, Touched>> {
    let mut files: BTreeMap<String, Touched> = BTreeMap::new();
    let mut reading = Reading::Header;
    let mut current: Option<String> = None;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            reading = Reading::Header;
            current = None;
            continue;
        }
        if reading == Reading::Header
            && let Some(target) = line.strip_prefix("+++ ")
        {
            current = if target == "/dev/null" {
                None
            } else {
                Some(target.strip_prefix("b/")?.to_owned())
            };
            continue;
        }
        let Some(header) = line.strip_prefix("@@ ") else {
            continue;
        };
        reading = Reading::Hunks;
        let Some(path) = &current else {
            continue;
        };
        let Added::Lines(first, last) = added(header)? else {
            continue;
        };
        if let Touched::Ranges(ranges) = files
            .entry(path.clone())
            .or_insert_with(|| Touched::Ranges(Vec::new()))
        {
            ranges.push((first, last));
        }
    }
    Some(files)
}

/// What one hunk left on the new side of its file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Added {
    /// These inclusive lines.
    Lines(u32, u32),
    /// No line: the hunk only removed some.
    Nothing,
}

/// What a hunk header says it left, or nothing when the header cannot be read.
fn added(header: &str) -> Option<Added> {
    let new_side = header.split(' ').find_map(|part| part.strip_prefix('+'))?;
    let (first, count) = match new_side.split_once(',') {
        Some((first, count)) => (number(first)?, number(count)?),
        None => (number(new_side)?, 1),
    };
    let Some(extra) = count.checked_sub(1) else {
        return Some(Added::Nothing);
    };
    first
        .checked_add(extra)
        .map(|last| Added::Lines(first, last))
}

/// A line number or a count as a hunk header writes it.
fn number(text: &str) -> Option<u32> {
    match text.parse::<u32>() {
        Ok(number) => Some(number),
        Err(_not_a_line_number) => None,
    }
}

/// The Rust files a change set names, keeping only what `include` already admits when it admits anything, or that it names none.
/// # Errors
/// Refuses a changed path which cannot be represented by the mutation glob language.
/// Silently omitting such a path would make a partial change set indistinguishable from the complete one the caller asked for.
pub fn within(change: &Change, include: &[Pattern]) -> Result<Within, GlobError> {
    let sources: Vec<&String> = change
        .files
        .iter()
        .filter(|path| Path::new(path).extension() == Some(std::ffi::OsStr::new("rs")))
        .filter(|path| include.is_empty() || include.iter().any(|pattern| pattern.matches(path)))
        .collect();
    if sources.is_empty() {
        return Ok(Within::Nothing {
            changed: change.files.clone(),
        });
    }
    sources
        .into_iter()
        .map(|path| Pattern::compile(path))
        .collect::<Result<Vec<Pattern>, GlobError>>()
        .map(Within::Changed)
}

/// Whether an empty answer is an answer.
/// A `HEAD` that resolves to nothing is nothing; a diff that lists nothing is a diff that lists nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Empty {
    Refuse,
    Accept,
}

/// Whether the leading whitespace of an answer carries meaning.
/// It does in `status --porcelain`, where the first two columns are the status and a space is one of the values they take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Trimmed,
    Verbatim,
}

/// Whether a path lies in one of the directories a run writes rather than one it is about.
fn is_under(path: &str, excluded: &[&str]) -> bool {
    excluded
        .iter()
        .any(|directory| path == *directory || path.starts_with(&format!("{directory}/")))
}

/// The path in one `git status --porcelain` line: two status characters, a space, then the path, with a rename written as `old -> new`.
fn porcelain_path(line: &str) -> Option<String> {
    let rest = line.get(3..)?.trim_start();
    if rest.is_empty() {
        return None;
    }
    let path = rest.rsplit(" -> ").next()?;
    Some(path.trim_matches('"').to_owned())
}

/// The output of one git command, or nothing when it could not be run or did not succeed.
/// Every variable that tells git to answer about a repository other than the one it is standing in.
///
/// A closed set, because leaving one out is the defect rather than a smaller version of it: each of these is enough on its own to make git answer about somewhere else.
const REDIRECTING: [&str; 10] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_PREFIX",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
];

/// `env` with the variables that would point git somewhere else taken out.
///
/// A run names the repository it verified.
/// `GIT_DIR` in the environment it happened to be started with — which is what a git hook sets, and what any wrapper may — makes git answer about that one instead, and the report then names another repository's commit as the thing it established something about.
/// That is a conclusion drawn from how the run was invoked rather than from what it looked at, so the invocation is not allowed to reach the question.
fn about_the_tree(env: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
    env.iter()
        .filter(|(name, _value)| {
            !REDIRECTING
                .iter()
                .any(|pointed| crate::vars::same_name(name, std::ffi::OsStr::new(pointed)))
        })
        .cloned()
        .collect()
}

fn ask<W: Watch>(
    asking: &Asking<'_, W>,
    empty: Empty,
    shape: Shape,
    arguments: &[&str],
) -> Option<String> {
    ask_up_to(asking, (empty, shape, ANSWER_LIMIT), arguments)
}

/// The same, reading at most `limit` bytes of the answer and refusing one longer than that rather than reading part of it.
fn ask_up_to<W: Watch>(
    asking: &Asking<'_, W>,
    (empty, shape, limit): (Empty, Shape, usize),
    arguments: &[&str],
) -> Option<String> {
    let mut argv: Vec<OsString> = vec![OsString::from("git")];
    argv.extend(arguments.iter().map(OsString::from));
    let mut spec = Spec::new(argv, Bound::After(crate::runner::PROBE));
    spec.dir = Some(asking.root.to_path_buf());
    spec.env = Some(about_the_tree(asking.env));
    spec.structured_stdout = Some(limit);

    let asked = run(&spec, asking.watch.cancel());
    asking.watch.exec(&spec, &asked);
    match asked.termination {
        crate::runner::Termination::Exited(crate::runner::ProcessExit::Code(0)) => {}
        crate::runner::Termination::NotStarted { .. }
        | crate::runner::Termination::Exited(_)
        | crate::runner::Termination::TimedOut
        | crate::runner::Termination::Stalled
        | crate::runner::Termination::StoppedByMonitor
        | crate::runner::Termination::MonitorFailed { .. }
        | crate::runner::Termination::Cancelled { .. }
        | crate::runner::Termination::WaitFailed { .. } => return None,
    }
    if asked.stdout_truncated {
        return None;
    }
    let raw = match std::str::from_utf8(&asked.stdout) {
        Ok(raw) => raw,
        Err(_non_utf8_git_protocol) => return None,
    };
    let answer = match shape {
        Shape::Trimmed => raw.trim().to_owned(),
        Shape::Verbatim => raw.trim_end_matches('\n').to_owned(),
    };
    if answer.is_empty() && empty == Empty::Refuse {
        return None;
    }
    Some(answer)
}
