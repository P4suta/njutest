// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the repository was when the run started.

use std::ffi::OsString;
use std::path::Path;

use rust_mutants::runner::{Spec, run};

use crate::report::{Git, UNAVAILABLE};
use crate::trace::ExecRecord;
use crate::watch::Watch;

/// The limitation a report states when git could not be asked.
pub const UNAVAILABLE_LIMITATION: &str = "git-metadata-unavailable";

/// Asks git about the tree at `root`.
#[must_use]
pub fn describe(root: &Path, env: &[(OsString, OsString)], watch: Watch<'_>) -> Git {
    let Some(commit) = ask(
        &Question {
            root,
            env,
            empty: Empty::Refuse,
            shape: Shape::Trimmed,
        },
        &["rev-parse", "HEAD"],
        watch,
    ) else {
        return Git::unavailable();
    };
    let Some(branch) = ask(
        &Question {
            root,
            env,
            empty: Empty::Refuse,
            shape: Shape::Trimmed,
        },
        &["rev-parse", "--abbrev-ref", "HEAD"],
        watch,
    ) else {
        return Git::unavailable();
    };
    let Some(status) = ask(
        &Question {
            root,
            env,
            empty: Empty::Accept,
            shape: Shape::Verbatim,
        },
        &["status", "--porcelain"],
        watch,
    ) else {
        return Git::unavailable();
    };
    Git {
        available: true,
        commit,
        branch,
        dirty: !status.trim().is_empty(),
        merge_base: None,
        changed_files: Vec::new(),
    }
}

/// Whether an empty answer is an answer. A `HEAD` that resolves to nothing is nothing; a diff that lists nothing is a diff that lists nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Empty {
    Refuse,
    Accept,
}

/// Whether the leading whitespace of the answer carries meaning. It does in `status --porcelain`, where the first two columns are the status and a space is one of the values they take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Trimmed,
    Verbatim,
}

/// The trimmed output of one git command, or nothing when it could not be run or did not succeed.
/// One question for git, and how to read the answer.
#[derive(Debug, Clone, Copy)]
struct Question<'a> {
    root: &'a Path,
    env: &'a [(OsString, OsString)],
    empty: Empty,
    shape: Shape,
}

fn ask(question: &Question<'_>, arguments: &[&str], watch: Watch<'_>) -> Option<String> {
    let Question {
        root,
        env,
        empty,
        shape,
    } = *question;
    let mut argv: Vec<OsString> = vec![OsString::from("git")];
    argv.extend(arguments.iter().map(OsString::from));
    let mut spec = Spec::new(argv);
    spec.dir = Some(root.to_path_buf());
    spec.env = Some(env.to_vec());
    spec.structured_stdout = Some(1 << 20);

    let asked = run(&spec, watch.cancel);
    watch.trace.exec(ExecRecord::of(&spec, &asked));
    if asked.error.is_some() || asked.exit_code != 0 {
        return None;
    }
    let raw = String::from_utf8_lossy(&asked.stdout);
    let answer = match shape {
        Shape::Trimmed => raw.trim().to_owned(),
        Shape::Verbatim => raw.trim_end_matches('\n').to_owned(),
    };
    if answer.is_empty() && empty == Empty::Refuse {
        return None;
    }
    if answer == UNAVAILABLE {
        return None;
    }
    Some(answer)
}

/// What a run about a change set found: the revision it compared against, the merge base with it, and every file that differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The revision the change set was computed against.
    pub base: String,
    /// The commit both branches share, when git could name one.
    pub merge_base: Option<String>,
    /// Every file that differs, as workspace-relative paths, sorted. Uncommitted changes included.
    pub files: Vec<String>,
}

/// The default revision a change set is computed against.
pub const DEFAULT_BASE: &str = "HEAD";

/// Every file that differs from `base`, committed and not.
///
/// Returns nothing when git could not be asked or does not know `base`,
/// which the caller states as a limitation rather than reading as an empty
/// change set: a run that verified nothing because it could not see what
/// changed must never look like a run that verified everything that did.
#[must_use]
pub fn changed(
    root: &Path,
    env: &[(OsString, OsString)],
    base: &str,
    watch: Watch<'_>,
) -> Option<Change> {
    let merge_base = ask(
        &Question {
            root,
            env,
            empty: Empty::Refuse,
            shape: Shape::Trimmed,
        },
        &["merge-base", base, "HEAD"],
        watch,
    );
    let against = merge_base.as_deref().unwrap_or(base);
    let committed = ask(
        &Question {
            root,
            env,
            empty: Empty::Accept,
            shape: Shape::Trimmed,
        },
        &["diff", "--name-only", against],
        watch,
    )?;
    let uncommitted = ask(
        &Question {
            root,
            env,
            empty: Empty::Accept,
            shape: Shape::Verbatim,
        },
        &["status", "--porcelain"],
        watch,
    )?;
    let mut files: Vec<String> = committed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    files.extend(uncommitted.lines().filter_map(porcelain_path));
    files.retain(|path| !is_written_by_a_run(path));
    files.sort();
    files.dedup();
    Some(Change {
        base: base.to_owned(),
        merge_base,
        files,
    })
}

/// Whether a path is one a run writes rather than one it is about. A report a previous run left is not a change to the code.
fn is_written_by_a_run(path: &str) -> bool {
    crate::evidence::tree::EXCLUDED_DIRECTORIES
        .iter()
        .any(|directory| path == *directory || path.starts_with(&format!("{directory}/")))
}

/// The path in one `git status --porcelain` line: two status characters, a space, then the path, with a rename written as `old -> new`.
fn porcelain_path(line: &str) -> Option<String> {
    let rest = line.get(3..)?.trim_start();
    if rest.is_empty() {
        return None;
    }
    let path = rest.rsplit(" -> ").next().unwrap_or(rest);
    Some(path.trim_matches('"').to_owned())
}
