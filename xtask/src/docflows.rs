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
    /// actionlint refused at least one documented workflow.
    #[error("actionlint refused a workflow the documentation shows:\n{0}")]
    Refused(String),
}

/// The prefixes by which a page names this repository's own actions and reusable workflows.
const OWN: [&str; 2] = ["<owner>/njutest/.github/", "P4suta/njutest/.github/"];

/// Every `yaml` fence of every markdown page under `root`'s documentation and its README.
///
/// # Errors
/// [`DocflowsError::Io`] when a page cannot be read.
pub fn snippets(root: &Path) -> Result<Vec<Snippet>, DocflowsError> {
    let mut pages = vec![root.join("README.md")];
    for entry in walkdir::WalkDir::new(root.join("docs")).sort_by_file_name() {
        let entry = entry.map_err(|error| DocflowsError::Io {
            path: root.join("docs").display().to_string(),
            source: std::io::Error::other(error),
        })?;
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "md")
        {
            pages.push(entry.path().to_path_buf());
        }
    }
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

/// Lints every documented workflow with `actionlint` in a scratch repository holding this one's actions, so a `with:` key an action does not declare is refused.
///
/// # Errors
/// [`DocflowsError::Refused`] naming each page and fence actionlint refused, and the ways the check itself failed.
pub fn check(root: &Path, actionlint: &OsStr) -> Result<String, DocflowsError> {
    let found = snippets(root)?;
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
    let output = Command::new(actionlint)
        .args(["-no-color", "-shellcheck=", "-pyflakes="])
        .current_dir(scratch.path())
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
    for (index, snippet) in found.iter().enumerate().rev() {
        said = said.replace(
            &format!(".github/workflows/doc-{index}.yml"),
            &format!(
                "{} (the fence on line {})",
                match snippet.page.strip_prefix(root) {
                    Ok(relative) => relative.display(),
                    Err(_outside_the_root) => snippet.page.display(),
                },
                snippet.line
            ),
        );
    }
    Err(DocflowsError::Refused(said))
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), DocflowsError> {
    for entry in walkdir::WalkDir::new(from) {
        let entry = entry.map_err(|error| DocflowsError::Io {
            path: from.display().to_string(),
            source: std::io::Error::other(error),
        })?;
        let relative = entry
            .path()
            .strip_prefix(from)
            .map_err(|error| DocflowsError::Io {
                path: entry.path().display().to_string(),
                source: std::io::Error::other(error),
            })?;
        let target = to.join(relative);
        let copied = if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)
        } else {
            std::fs::copy(entry.path(), &target).map(|_bytes| ())
        };
        copied.map_err(|source| DocflowsError::Io {
            path: target.display().to_string(),
            source,
        })?;
    }
    Ok(())
}
