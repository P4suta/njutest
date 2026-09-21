// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the workspace is made of, read from cargo rather than written down again: a gate that enumerates the crates by hand is one that stops covering the crate somebody adds next.

#![expect(
    clippy::panic,
    reason = "support for tests reports a setup failure by panicking: a gate that cannot \
              read the workspace it is about has nothing left to assert"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// One workspace member, as much of it as a gate asks about.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Member {
    /// The package name.
    pub name: String,
    /// The directory holding its manifest.
    pub directory: PathBuf,
    /// The binaries it declares.
    pub binaries: BTreeSet<String>,
    /// Whether cargo would publish it.
    pub published: bool,
}

impl Member {
    /// Every `.rs` file of this member's integration suite directory, by name, and nothing when it keeps none.
    ///
    /// # Panics
    /// When the directory is there and cannot be read, which a gate does not carry on past.
    #[must_use]
    pub fn suites(&self) -> Vec<(String, PathBuf)> {
        let at = self.directory.join("tests");
        let entries = match std::fs::read_dir(&at) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(error) => panic!("{}: {error}", at.display()),
        };
        let mut found: Vec<(String, PathBuf)> = entries
            .map(|entry| entry.unwrap_or_else(|error| panic!("{}: {error}", at.display())))
            .filter(|entry| entry.path().extension().is_some_and(|one| one == "rs"))
            .map(|entry| match entry.file_name().into_string() {
                Ok(name) => (name, entry.path()),
                Err(name) => panic!(
                    "a test file name is not UTF-8: {}",
                    PathBuf::from(name).display()
                ),
            })
            .collect();
        found.sort();
        found
    }
}

/// Every member of the workspace rooted at `root`, in name order.
///
/// # Panics
/// When cargo cannot read the workspace, which is not a thing a gate recovers from.
#[must_use]
pub fn members(root: &Path) -> Vec<Member> {
    let mut command = cargo_metadata::MetadataCommand::new();
    command
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .other_options(["--locked".to_owned(), "--offline".to_owned()]);
    let metadata = command
        .exec()
        .unwrap_or_else(|error| panic!("workspace metadata: {error}"));
    let mut found: Vec<Member> = metadata
        .workspace_packages()
        .into_iter()
        .map(|package| Member {
            name: package.name.to_string(),
            directory: package
                .manifest_path
                .parent()
                .map_or_else(|| PathBuf::from("."), |at| PathBuf::from(at.as_std_path())),
            binaries: package
                .targets
                .iter()
                .filter(|target| target.is_bin())
                .map(|target| target.name.clone())
                .collect(),
            published: package.publish.is_none(),
        })
        .collect();
    found.sort();
    found
}
