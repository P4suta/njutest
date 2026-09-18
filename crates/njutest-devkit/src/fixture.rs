// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A throwaway copy of a fixture project, with the directories a run of one needs beside it.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "support for tests reports a setup failure by panicking: a test that cannot \
              copy the tree it is about has nothing left to assert"
)]

use std::path::{Path, PathBuf};

use sha2::Digest as _;

/// A copy of a fixture project, removed when the test drops it.
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
            .prefix("njutest-fixture-")
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
        .unwrap_or_else(|error| panic!("reading {}: {error}", dir.display()))
        .map(|entry| entry.expect("the entry"))
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    entries
}

/// `path` resolved, in the one spelling the products hold a directory in.
///
/// Windows answers `canonicalize` with the extended form, `\\?\C:\...`, and the
/// products put what they resolved back into the plain one: the engine states
/// the rule in `rust_mutants::canonical`, and the dependency direction
/// `cargo xtask deps` holds keeps this crate below it rather than above, so the
/// rule is stated again here for the suites. A fixture that handed a run the
/// extended spelling would have the run answer in a name no assertion here
/// writes.
fn canonical(path: &Path) -> PathBuf {
    let resolved = path.canonicalize().unwrap_or_else(|_error| path.to_owned());
    #[cfg(windows)]
    {
        let text = resolved.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\")
            && !rest.starts_with("UNC\\")
            && Path::new(rest).is_absolute()
        {
            return PathBuf::from(rest);
        }
    }
    resolved
}

/// The fence that opens the block of a fixture's README stating what a run of it establishes.
pub const FATES_FENCE: &str = "```fates";

/// One mutation of a fixture, and what a run of it establishes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fate {
    /// The file the mutation is in, relative to the fixture root.
    pub path: String,
    /// The 1-based line of the edit, or zero for a candidate the compiler refused.
    pub line: u32,
    /// The 1-based byte column of the edit, or zero for a refusal.
    pub column: u32,
    /// The rule that proposed it.
    pub rule: String,
    /// What a run establishes: an outcome, or `refused`.
    pub outcome: String,
}

impl std::fmt::Display for Fate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{} {} {}",
            self.path, self.line, self.column, self.rule, self.outcome
        )
    }
}

/// What one fixture's README says a run of it establishes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fates {
    /// Whether the README states a ledger at all.
    pub stated: bool,
    /// The arguments the run takes beyond `--tier all --offline --locked`, from the rest of the fence line.
    pub args: Vec<String>,
    /// Every mutation and its fate, in the order the block states them.
    pub rows: Vec<Fate>,
}

/// Every fate the `fates` block of `readme` states, in the order it states them.
#[must_use]
pub fn fates(readme: &str) -> Fates {
    let Some(after) = readme.split_once(FATES_FENCE).map(|(_, rest)| rest) else {
        return Fates::default();
    };
    let (fence, rest) = after.split_once('\n').unwrap_or((after, ""));
    let block = rest.split_once("```").map_or(rest, |(block, _)| block);
    Fates {
        stated: true,
        args: fence.split_whitespace().map(str::to_owned).collect(),
        rows: block.lines().filter_map(fate).collect(),
    }
}

/// One line of a fates block: `path:line:column rule outcome`.
fn fate(line: &str) -> Option<Fate> {
    let mut parts = line.split_whitespace();
    let (place, rule, outcome) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let mut at = place.rsplitn(3, ':');
    let column = at.next()?.parse().ok()?;
    let line = at.next()?.parse().ok()?;
    let path = at.next()?.to_owned();
    Some(Fate {
        path,
        line,
        column,
        rule: rule.to_owned(),
        outcome: outcome.to_owned(),
    })
}

/// The fence that opens the block of a fixture's README stating what its seams established.
pub const SEAMS_FENCE: &str = "```seams";

/// One question a fixture's seam licensed, and what a run of it establishes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Seam {
    /// The capability the seam serves.
    pub capability: String,
    /// Which exchange on that seam the question is about.
    pub seq: u64,
    /// The rule that asked it.
    pub rule: String,
    /// What the run established: `tests`, `proved`, `unnoticed`, or `unreached`.
    pub decision: String,
    /// Who decided it, where anybody did: the target that noticed, or the proof that discharged it.
    pub by: Option<String>,
}

impl std::fmt::Display for Seam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{} {} {}",
            self.capability, self.seq, self.rule, self.decision
        )?;
        self.by.as_ref().map_or(Ok(()), |who| write!(f, " {who}"))
    }
}

/// Every seam fate the `seams` block of `readme` states, in the order it states them.
#[must_use]
pub fn seams(readme: &str) -> Vec<Seam> {
    let Some(after) = readme.split_once(SEAMS_FENCE).map(|(_, rest)| rest) else {
        return Vec::new();
    };
    let (_fence, rest) = after.split_once('\n').unwrap_or((after, ""));
    let block = rest.split_once("```").map_or(rest, |(block, _)| block);
    block.lines().filter_map(seam).collect()
}

/// One line of a seams block: `capability:seq rule decision [by]`.
fn seam(line: &str) -> Option<Seam> {
    let mut parts = line.split_whitespace();
    let (place, rule, decision) = (parts.next()?, parts.next()?, parts.next()?);
    let by = parts.next().map(str::to_owned);
    if parts.next().is_some() {
        return None;
    }
    let (capability, seq) = place.rsplit_once(':')?;
    Some(Seam {
        capability: capability.to_owned(),
        seq: seq.parse().ok()?,
        rule: rule.to_owned(),
        decision: decision.to_owned(),
        by,
    })
}

/// Every seam fate the committed README of the fixture named states.
///
/// # Panics
/// When the fixture has no README, which `cargo xtask fixtures` refuses.
#[must_use]
pub fn stated_seams(name: &str) -> Vec<Seam> {
    seams(&readme_of(name))
}

/// Every fate the committed README of the fixture named states.
///
/// # Panics
/// When the fixture has no README, which `cargo xtask fixtures` refuses.
#[must_use]
pub fn stated_fates(name: &str) -> Fates {
    fates(&readme_of(name))
}

/// The committed README of the fixture named.
fn readme_of(name: &str) -> String {
    let path = crate::paths::workspace_root()
        .join("fixtures")
        .join(name)
        .join("README.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The directory of the newest stored run under `reports`, followed from the pointer a run writes.
///
/// Where a project stores its runs is the project's to say, so the caller
/// names the directory. A kit that guessed it would decide the layout for
/// every test that uses it.
///
/// # Panics
/// When the pointer is not there or does not name a document, which means no
/// run stored a report under `reports`.
#[must_use]
pub fn newest_run(reports: &Path) -> PathBuf {
    let directory = reports.to_path_buf();
    let pointer = directory.join("latest.json");
    let text = std::fs::read_to_string(&pointer)
        .unwrap_or_else(|error| panic!("{}: {error}", pointer.display()));
    let document: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{}: {error}", pointer.display()));
    let named = document
        .get("document")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("{} names no document: {text}", pointer.display()));
    directory
        .join(named)
        .parent()
        .unwrap_or_else(|| panic!("{named} is not inside a run's own directory"))
        .to_path_buf()
}

/// The report the newest stored run left, as text.
///
/// # Panics
/// When there is no such run, or its report cannot be read.
#[must_use]
pub fn stored_report(reports: &Path) -> String {
    let path = newest_run(reports).join("run-report-v1.json");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}
