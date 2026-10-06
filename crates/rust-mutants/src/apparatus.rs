// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run executes, as it stood when the run built it, so a mutant whose tests change it is caught before the change is read as anybody's answer.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What distinguishes one file from the one a test put in its place: removing it, replacing it, or rewriting it in place changes it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Identity {
    len: u64,
    modified: Option<SystemTime>,
    file: (u64, u64),
}

#[cfg(unix)]
fn file_of(metadata: &std::fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt as _;
    (metadata.dev(), metadata.ino())
}

#[cfg(windows)]
fn file_of(metadata: &std::fs::Metadata) -> (u64, u64) {
    use std::os::windows::fs::MetadataExt as _;
    (metadata.creation_time(), 0)
}

fn identity(path: &Path) -> io::Result<Identity> {
    let metadata = std::fs::metadata(path)?;
    Ok(Identity {
        len: metadata.len(),
        modified: match metadata.modified() {
            Ok(at) => Some(at),
            Err(_unrecorded_here) => None,
        },
        file: file_of(&metadata),
    })
}

fn names(directory: &Path) -> io::Result<BTreeSet<OsString>> {
    let mut found: BTreeSet<OsString> = std::fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<io::Result<_>>()?;
    found.retain(|name| {
        !Path::new(name).extension().is_some_and(|extension| {
            ["rlib", "rmeta", "d"]
                .iter()
                .any(|compile_only| extension == std::ffi::OsStr::new(compile_only))
        })
    });
    Ok(found)
}

/// How a file the run executes, or one beside it, changed after the run built it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// It is gone.
    Missing(PathBuf),
    /// Something else is at its name now, or it was rewritten where it stood.
    Replaced(PathBuf),
}

impl std::fmt::Display for Change {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(path) => write!(f, "{} is gone", path.display()),
            Self::Replaced(path) => write!(f, "{} was replaced", path.display()),
        }
    }
}

/// How many changes a message names before it counts the rest.
pub const NAMED: usize = 3;

/// The changes a person reads first, the executables among them first, and how many more there were: a test that empties a directory changes hundreds of files, and a message naming each is one nobody reads.
#[must_use]
pub fn summary(changes: &[Change]) -> String {
    let named: Vec<String> = changes
        .iter()
        .take(NAMED)
        .map(ToString::to_string)
        .collect();
    match changes.len().checked_sub(NAMED) {
        Some(more) if more > 0 => format!("{}; and {more} more", named.join("; ")),
        Some(_) | None => named.join("; "),
    }
}

/// The mutants that ran beside the one a change was found after, as a message names them: any of them may have made it.
#[must_use]
pub fn alongside(beside: &[String]) -> String {
    if beside.is_empty() {
        return String::new();
    }
    let named: Vec<&str> = beside.iter().take(NAMED).map(String::as_str).collect();
    match beside.len().checked_sub(NAMED) {
        Some(more) if more > 0 => format!(
            " or one running beside it, {} and {more} more",
            named.join(", ")
        ),
        Some(_) | None => format!(" or one running beside it, {}", named.join(", ")),
    }
}

/// Every test executable and sibling runtime file as the build left them, excluding static Cargo outputs.
#[derive(Debug, Default)]
pub struct Apparatus {
    executables: Vec<(PathBuf, Identity)>,
    beside: Vec<(PathBuf, BTreeSet<OsString>)>,
    /// Which mutants' executions are running, so a change is charged to every execution that could have made it.
    pub running: std::sync::Mutex<Running>,
}

impl Apparatus {
    /// Surveys readable executables and sibling runtime names under `within`, excluding compile-only archives, metadata and dep-info.
    #[must_use]
    pub fn survey<'a>(executables: impl IntoIterator<Item = &'a Path>, within: &Path) -> Self {
        let mut surveyed = Self::default();
        let mut directories = BTreeSet::new();
        for executable in executables {
            if !executable.starts_with(within) {
                continue;
            }
            let Ok(was) = identity(executable) else {
                continue;
            };
            surveyed.executables.push((executable.to_path_buf(), was));
            if let Some(directory) = executable.parent() {
                directories.insert(directory.to_path_buf());
            }
        }
        for directory in directories {
            match names(&directory) {
                Ok(held) => surveyed.beside.push((directory, held)),
                Err(_unreadable_after_the_build) => {}
            }
        }
        surveyed
    }

    /// Everything that is not as the survey found it: an executable gone or replaced, and a name gone from beside one.
    /// A name that appeared is not a change, since a test may write beside itself without touching what the run executes.
    #[must_use]
    pub fn changed(&self) -> Vec<Change> {
        let mut changes = Vec::new();
        for (path, was) in &self.executables {
            match identity(path) {
                Ok(now) if &now == was => {}
                Ok(_other) => changes.push(Change::Replaced(path.clone())),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    changes.push(Change::Missing(path.clone()));
                }
                Err(_unreadable) => changes.push(Change::Replaced(path.clone())),
            }
        }
        for (directory, held) in &self.beside {
            match names(directory) {
                Ok(now) => changes.extend(
                    held.difference(&now)
                        .map(|name| Change::Missing(directory.join(name))),
                ),
                Err(_unreadable) => changes.push(Change::Missing(directory.clone())),
            }
        }
        changes
    }
}

/// Which mutants' executions are running, and every one that has started, so a change found after one execution can be charged to every execution that could have made it.
#[derive(Debug, Default)]
pub struct Running {
    now: BTreeSet<String>,
    started: Vec<String>,
}

/// Where a ledger stood when one execution started: who was running then, and how many had started.
#[derive(Debug, Clone)]
pub struct Began {
    running: BTreeSet<String>,
    from: usize,
}

impl Running {
    /// Notes that `mutant`'s execution starts now.
    pub fn begin(&mut self, mutant: &str) -> Began {
        let began = Began {
            running: self.now.clone(),
            from: self.started.len(),
        };
        self.now.insert(mutant.to_owned());
        self.started.push(mutant.to_owned());
        began
    }

    /// Notes that `mutant`'s execution has ended, and names every other execution that ran at any moment it did: those running when it began, and those that began after.
    pub fn end(&mut self, mutant: &str, began: Began) -> Vec<String> {
        self.now.remove(mutant);
        let mut beside = began.running;
        beside.extend(self.started.iter().skip(began.from).cloned());
        beside.remove(mutant);
        beside.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Apparatus, Change};

    #[test]
    fn cargo_can_collect_static_build_outputs_while_runtime_files_stay_guarded() {
        let directory = tempfile::tempdir().expect("the execution directory");
        let executable = directory.path().join("program");
        std::fs::write(&executable, b"the executable").expect("an executable");
        for name in ["old.rlib", "old.rmeta", "old.d", "runtime.dll"] {
            std::fs::write(directory.path().join(name), b"built").expect("a build output");
        }
        let apparatus = Apparatus::survey([executable.as_path()], directory.path());
        for name in ["old.rlib", "old.rmeta", "old.d"] {
            std::fs::remove_file(directory.path().join(name)).expect("Cargo collects old outputs");
        }
        assert!(
            apparatus.changed().is_empty(),
            "static build outputs cannot be loaded by a running harness: {:?}",
            apparatus.changed()
        );
        let runtime = directory.path().join("runtime.dll");
        std::fs::remove_file(&runtime).expect("a runtime file is removed");
        assert!(apparatus.changed().contains(&Change::Missing(runtime)));
        std::fs::remove_file(&executable).expect("the executable is removed");
        assert!(apparatus.changed().contains(&Change::Missing(executable)));
    }
}
