// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every workflow example the documentation shows, linted as the workflow a reader would paste, against this repository's own actions.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

/// One `yaml` fence of a page, made the workflow a reader would paste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    /// The page it is on.
    pub page: PathBuf,
    /// The line its fence opens on, counted from one.
    pub line: usize,
    /// The workflow it becomes: a list of steps wrapped in a job, a fragment given a trigger, and this repository's actions named by their local path.
    pub workflow: String,
}

/// Why the documented workflows could not be checked, or were refused.
#[derive(Debug, Error)]
pub enum DocflowsError {
    /// A page or an action could not be read, or the scratch repository written.
    #[error("{path}: {source}")]
    Io {
        /// The path the failure is about.
        path: String,
        /// The filesystem failure.
        source: std::io::Error,
    },
    /// actionlint could not be started.
    #[error("could not start actionlint: {0}")]
    Start(std::io::Error),
    /// actionlint said something that is not UTF-8, so what it refused cannot be named.
    #[error("actionlint wrote bytes that are not UTF-8: {0}")]
    Undecodable(std::string::FromUtf8Error),
    /// actionlint ran and said something other than which workflows it refused, so nothing was checked.
    #[error("actionlint did not check the documented workflows ({status}):\n{said}")]
    Failed {
        /// How it ended.
        status: std::process::ExitStatus,
        /// What it said instead.
        said: String,
    },
    /// actionlint refused at least one documented workflow.
    #[error("actionlint refused a workflow the documentation shows:\n{0}")]
    Refused(String),
}

impl crate::error::Coded for DocflowsError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Io { .. } | Self::Start(..) | Self::Undecodable(..) | Self::Failed { .. } => {
                crate::error::XtCode::DocflowsUnchecked
            }
            Self::Refused(..) => crate::error::XtCode::DocflowsRefused,
        }
    }
}

/// The prefixes by which a page names this repository's own actions and reusable workflows.
const OWN: [&str; 2] = ["<owner>/njutest/.github/", "P4suta/njutest/.github/"];

/// Every `yaml` fence of every markdown page under `root`'s documentation and its README.
///
/// # Errors
/// [`DocflowsError::Io`] when a page cannot be read.
pub fn snippets(root: &Path) -> Result<Vec<Snippet>, DocflowsError> {
    let mut pages = vec![root.join("README.md")];
    let listed = crate::repository::under(root, "docs").map_err(|failure| DocflowsError::Io {
        path: root.join("docs").display().to_string(),
        source: std::io::Error::other(failure.to_string()),
    })?;
    pages.extend(
        listed
            .into_iter()
            .filter(|page| page.extension().is_some_and(|extension| extension == "md")),
    );
    let mut found = Vec::new();
    for page in pages {
        let text = std::fs::read_to_string(&page).map_err(|source| DocflowsError::Io {
            path: page.display().to_string(),
            source,
        })?;
        for (line, fence) in fences(&text) {
            found.push(Snippet {
                page: page.clone(),
                line,
                workflow: workflow(&fence),
            });
        }
    }
    Ok(found)
}

/// The body of every `yaml` fence of `text`, with the line each opens on.
fn fences(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (number, line) in (1_usize..).zip(text.lines()) {
        let trimmed = line.trim();
        match open.take() {
            Some((start, body)) if trimmed == "```" => found.push((start, body)),
            Some((start, mut body)) => {
                body.push_str(line);
                body.push('\n');
                open = Some((start, body));
            }
            None if trimmed == "```yaml" => open = Some((number, String::new())),
            None => {}
        }
    }
    found
}

/// The workflow a fence becomes when a reader pastes it.
fn workflow(fence: &str) -> String {
    let mut own = fence.to_owned();
    for prefix in OWN {
        own = own.replace(prefix, "./.github/");
    }
    let local: String = own
        .lines()
        .map(|line| match line.split_once("uses: ./.github/") {
            Some((before, after)) => format!(
                "{before}uses: ./.github/{}",
                after.split('@').next().unwrap_or_default()
            ),
            None => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    if local.lines().any(|line| line.starts_with("- ")) {
        let mut steps = String::new();
        for line in local.lines() {
            steps.push_str("      ");
            steps.push_str(line);
            steps.push('\n');
        }
        return format!(
            "on: push\njobs:\n  snippet:\n    runs-on: ubuntu-latest\n    steps:\n{steps}"
        );
    }
    if local.lines().any(|line| line.starts_with("on:")) {
        return format!("{local}\n");
    }
    format!("on: push\n{local}\n")
}

/// Lints every documented workflow with `actionlint` against this repository's actions, refusing one that names an action this commit does not ship.
///
/// # Errors
/// [`DocflowsError::Refused`] naming each page and fence actionlint refused, and the ways the check itself failed.
pub fn check(root: &Path, actionlint: &OsStr) -> Result<String, DocflowsError> {
    let found = snippets(root)?;
    let missing = unshipped(root, &found);
    if !missing.is_empty() {
        return Err(DocflowsError::Refused(missing.join("\n")));
    }
    let scratch = tempfile::tempdir().map_err(|source| DocflowsError::Io {
        path: "a scratch repository".to_owned(),
        source,
    })?;
    copy_tree(
        &root.join(".github/actions"),
        &scratch.path().join(".github/actions"),
    )?;
    let workflows = scratch.path().join(".github/workflows");
    for directory in [workflows.clone(), scratch.path().join(".git")] {
        std::fs::create_dir_all(&directory).map_err(|source| DocflowsError::Io {
            path: directory.display().to_string(),
            source,
        })?;
    }
    for (index, snippet) in found.iter().enumerate() {
        let path = workflows.join(format!("doc-{index}.yml"));
        std::fs::write(&path, &snippet.workflow).map_err(|source| DocflowsError::Io {
            path: path.display().to_string(),
            source,
        })?;
    }
    let written: Vec<PathBuf> = (0..found.len())
        .map(|index| workflows.join(format!("doc-{index}.yml")))
        .collect();
    let output = Command::new(actionlint)
        .args(["-no-color", "-shellcheck=", "-pyflakes="])
        .args(&written)
        .current_dir(root)
        .output()
        .map_err(DocflowsError::Start)?;
    if output.status.success() {
        return Ok(format!(
            "docflows: {} documented workflows pass actionlint against this repository's actions",
            found.len()
        ));
    }
    let mut said = String::new();
    for stream in [output.stdout, output.stderr] {
        said.push_str(&String::from_utf8(stream).map_err(DocflowsError::Undecodable)?);
    }
    let refused = output.status.code() == Some(1)
        && said
            .lines()
            .any(|line| snippet_of(line, found.len()).is_some());
    if !refused {
        return Err(DocflowsError::Failed {
            status: output.status,
            said,
        });
    }
    let named: Vec<String> = said
        .lines()
        .map(|line| match snippet_of(line, found.len()) {
            Some((index, rest)) => found.get(index).map_or_else(
                || line.to_owned(),
                |snippet| {
                    format!(
                        "{} (the fence on line {}):{rest}",
                        match snippet.page.strip_prefix(root) {
                            Ok(relative) => relative.display(),
                            Err(_outside_the_root) => snippet.page.display(),
                        },
                        snippet.line
                    )
                },
            ),
            None => line.to_owned(),
        })
        .collect();
    said = named.join("\n");
    Err(DocflowsError::Refused(said))
}

/// Every documented use of this repository's own action that no `action.yml` of this commit answers, by page and fence, since actionlint passes a local action it cannot find.
fn unshipped(root: &Path, found: &[Snippet]) -> Vec<String> {
    let mut missing = Vec::new();
    for snippet in found {
        for line in snippet.workflow.lines() {
            let Some((_, named)) = line.split_once("uses: ./.github/") else {
                continue;
            };
            let directory = root.join(".github").join(named.trim());
            let mut why = "which this commit does not ship".to_owned();
            for file in ["action.yml", "action.yaml"] {
                match std::fs::symlink_metadata(directory.join(file)) {
                    Ok(metadata) if metadata.is_file() => {
                        why.clear();
                        break;
                    }
                    Ok(_not_a_file) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => why = format!("whose {file} cannot be read: {error}"),
                }
            }
            if why.is_empty() {
                continue;
            }
            let page = match snippet.page.strip_prefix(root) {
                Ok(relative) => relative.display(),
                Err(_outside_the_root) => snippet.page.display(),
            };
            missing.push(format!(
                "{page} (the fence on line {}): uses .github/{}, {why}",
                snippet.line,
                named.trim()
            ));
        }
    }
    missing
}

/// Which snippet a line of actionlint's report is about, and what the report says after its path, however the path was spelled.
fn snippet_of(line: &str, count: usize) -> Option<(usize, &str)> {
    let (before, rest) = line.split_once(".yml:")?;
    let (_, name) = before.rsplit_once("/.github/workflows/doc-")?;
    match name.parse::<usize>() {
        Ok(index) if index < count => Some((index, rest)),
        Ok(_) | Err(_) => None,
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), DocflowsError> {
    crate::repository::copy(from, to).map_err(|failure| DocflowsError::Io {
        path: from.display().to_string(),
        source: std::io::Error::other(failure.to_string()),
    })
}
