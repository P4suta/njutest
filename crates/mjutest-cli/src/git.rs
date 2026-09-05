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
    let Some(commit) = ask(root, env, &["rev-parse", "HEAD"], watch) else {
        return Git::unavailable();
    };
    let Some(branch) = ask(root, env, &["rev-parse", "--abbrev-ref", "HEAD"], watch) else {
        return Git::unavailable();
    };
    let Some(status) = ask(root, env, &["status", "--porcelain"], watch) else {
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

/// The trimmed output of one git command, or nothing when it could not be run or did not succeed. A `HEAD` that resolves to nothing is nothing.
fn ask(
    root: &Path,
    env: &[(OsString, OsString)],
    arguments: &[&str],
    watch: Watch<'_>,
) -> Option<String> {
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
    let answer = String::from_utf8_lossy(&asked.stdout).trim().to_owned();
    if answer.is_empty() && arguments.first() != Some(&"status") {
        return None;
    }
    if answer == UNAVAILABLE {
        return None;
    }
    Some(answer)
}
