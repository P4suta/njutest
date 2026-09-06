// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A throwaway copy of a fixture project, with the directories a run of one needs beside it.

#![expect(
    clippy::expect_used,
    reason = "support for tests reports a setup failure by panicking: a test that cannot \
              copy the tree it is about has nothing left to assert"
)]

use std::path::{Path, PathBuf};

use sha2::Digest as _;

/// A copy of a fixture project, removed when the test drops it.
///
/// The copy is what a test hands to the thing it is testing, so a run that
/// writes into the tree it measures writes into the copy. The temporary and
/// cache directories sit beside the tree rather than inside it: a cache under
/// the root would change the tree's own digest every time a run wrote to it.
#[derive(Debug)]
pub struct Fixture {
    root: PathBuf,
    temp: PathBuf,
    cache: PathBuf,
    _dir: tempfile::TempDir,
}

impl Fixture {
    /// Copies the fixture project `name` into a directory of this test's own.
    ///
    /// # Panics
    /// When the copy cannot be made, which a test cannot continue without.
    #[must_use]
    pub fn copy(name: &str) -> Self {
        Self::copy_with_siblings(name, &[])
    }

    /// Copies the fixture project `name`, and every fixture in `siblings` beside it, so a path dependency that climbs out of the tree has somewhere to land.
    ///
    /// # Panics
    /// When a copy cannot be made, which a test cannot continue without.
    #[must_use]
    pub fn copy_with_siblings(name: &str, siblings: &[&str]) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("mjutest-fixture-")
            .tempdir()
            .expect("a temporary directory");
        let trees = dir.path().join("trees");
        let fixtures = crate::paths::fixtures_dir();
        for tree in std::iter::once(name).chain(siblings.iter().copied()) {
            copy_tree(&fixtures.join(tree), &trees.join(tree));
        }
        let temp = dir.path().join("temp");
        let cache = dir.path().join("cache");
        std::fs::create_dir_all(&temp).expect("the temporary directory");
        std::fs::create_dir_all(&cache).expect("the cache directory");
        Self {
            root: canonical(&trees.join(name)),
            temp: canonical(&temp),
            cache: canonical(&cache),
            _dir: dir,
        }
    }

    /// The root of the copy, canonical, so a path a run reports compares equal to the one the test holds.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory a run of this fixture puts snapshots, build caches, and worker scratch in.
    #[must_use]
    pub fn temp(&self) -> &Path {
        &self.temp
    }

    /// The directory a run of this fixture keeps between-run caches in.
    #[must_use]
    pub fn cache(&self) -> &Path {
        &self.cache
    }

    /// The bytes of one file of the copy, by a root-relative path.
    ///
    /// # Panics
    /// When the file cannot be read.
    #[must_use]
    pub fn read(&self, relative: &str) -> Vec<u8> {
        std::fs::read(self.root.join(relative)).expect("the file")
    }

    /// Writes `contents` at a root-relative path of the copy, making the directories above it.
    ///
    /// # Panics
    /// When the file cannot be written.
    pub fn write(&self, relative: &str, contents: &[u8]) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the directory");
        }
        std::fs::write(&path, contents).expect("the file");
    }

    /// Every file of the copy with the digest of its bytes, by path, sorted: what a test compares before and after to say whether a run wrote into the tree.
    ///
    /// # Panics
    /// When the tree cannot be walked.
    #[must_use]
    pub fn fingerprint(&self) -> Vec<(String, String)> {
        fingerprint(&self.root)
    }
}

/// Copies a tree, skipping every `target` directory, following what a link stands for rather than copying the link.
///
/// # Panics
/// When the copy cannot be made.
pub fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the destination directory");
    for entry in sorted(from) {
        let name = entry.file_name();
        if name == "target" {
            continue;
        }
        let source = entry.path();
        let destination = to.join(&name);
        if source.is_dir() {
            copy_tree(&source, &destination);
        } else {
            let _copied = std::fs::copy(&source, &destination).expect("the file");
        }
    }
}

/// Every file under `root` with the digest of its bytes, by path, sorted, `target` directories skipped.
///
/// # Panics
/// When the tree cannot be walked.
#[must_use]
pub fn fingerprint(root: &Path) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in sorted(&dir) {
            let path = entry.path();
            if path.is_dir() {
                if entry.file_name() != "target" {
                    stack.push(path);
                }
                continue;
            }
            let bytes = std::fs::read(&path).expect("the file");
            entries.push((
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                hex::encode(sha2::Sha256::digest(&bytes)),
            ));
        }
    }
    entries.sort();
    entries
}

fn sorted(dir: &Path) -> Vec<std::fs::DirEntry> {
    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)
        .expect("the directory")
        .map(|entry| entry.expect("the entry"))
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    entries
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_error| path.to_owned())
}
