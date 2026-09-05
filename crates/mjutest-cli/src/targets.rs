// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run measures: one test, named the way a person names it.
//!
//! A target here is a single `#[test]`, not a binary. The whole design rests
//! on that: coverage is measured per test so a mutant can be routed to the
//! tests that reach it, and a kill is attributed to the test that caught it.
//! One binary holds many targets, and a run starts it once per target.
//!
//! # Identity
//!
//! A target's identity is `sha256` over the domain `mjutest-target-v1`, the
//! package, the unit kind, and the libtest path, and no line number: the
//! path *is* the semantic position, and a test that moved down a file is the
//! same test. That is what lets a cached result survive an edit elsewhere in
//! the file.
//!
//! # A binary with its own harness
//!
//! `harness = false` means libtest is not there to be asked, so `--list`
//! answers nothing. Such a binary is one target whose path is
//! [`WHOLE_BINARY`], and the report carries that as a limitation rather than
//! pretending the tests inside it were routed.

use std::path::PathBuf;
use std::time::Duration;

use crate::trace::ExecRecord;
use rust_mutants::runner::{Spec, run};
use sha2::{Digest as _, Sha256};

use crate::error::{self, ErrorCode};
use crate::watch::Watch;

/// The domain separator hashed first for every target identity. It carries
/// the recipe version.
pub const TARGET_DOMAIN: &str = "mjutest-target-v1";

/// How much of the digest a target identity spells out.
pub const TARGET_ID_HEX_LENGTH: usize = 16;

/// The path of the one target a binary with its own harness has.
pub const WHOLE_BINARY: &str = "";

/// How long listing a binary's tests may take. Listing is a process start
/// and a print; a binary that has not answered by now is not going to.
pub const LIST_TIMEOUT: Duration = Duration::from_secs(60);

/// The kind of compilation unit a test lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnitKind {
    /// The library's own tests.
    Lib,
    /// A binary's own tests.
    Bin,
    /// An integration test.
    Test,
    /// An example built with `test = true`.
    Example,
}

impl UnitKind {
    /// Every kind, in the order a report lists them.
    pub const ALL: [Self; 4] = [Self::Lib, Self::Bin, Self::Test, Self::Example];

    /// The name used in an identity and in reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Lib => "lib",
            Self::Bin => "bin",
            Self::Test => "test",
            Self::Example => "example",
        }
    }

    /// The kind named `name`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// The kind of an engine target, which reads the same cargo metadata.
    #[must_use]
    pub const fn of(kind: rust_mutants::execute::TargetKind) -> Self {
        use rust_mutants::execute::TargetKind;
        match kind {
            TargetKind::Lib => Self::Lib,
            TargetKind::Bin => Self::Bin,
            TargetKind::Test => Self::Test,
            TargetKind::Example => Self::Example,
        }
    }
}

/// One built test binary, before its tests are known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// The package that owns it.
    pub package: String,
    /// What kind of unit it is.
    pub kind: UnitKind,
    /// The cargo target's name.
    pub name: String,
    /// The binary cargo built.
    pub executable: PathBuf,
    /// The directory it runs in.
    pub cwd: PathBuf,
}

/// One test a run measures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The stable identity; see the module documentation.
    pub id: String,
    /// The package that owns it.
    pub package: String,
    /// What kind of unit it lives in.
    pub unit: UnitKind,
    /// The cargo target's name.
    pub unit_name: String,
    /// The libtest path, or [`WHOLE_BINARY`] for a binary with its own
    /// harness.
    pub path: String,
    /// Whether libtest will skip it unless asked.
    pub ignored: bool,
    /// The binary that runs it.
    pub executable: PathBuf,
    /// The directory it runs in.
    pub cwd: PathBuf,
}

impl Target {
    /// How a person refers to it: `package/kind/unit::path`.
    #[must_use]
    pub fn name(&self) -> String {
        if self.path == WHOLE_BINARY {
            return format!("{}/{}/{}", self.package, self.unit.name(), self.unit_name);
        }
        format!(
            "{}/{}/{} {}",
            self.package,
            self.unit.name(),
            self.unit_name,
            self.path
        )
    }

    /// Whether the whole binary is the target, because libtest is not there
    /// to be asked.
    #[must_use]
    pub fn is_whole_binary(&self) -> bool {
        self.path == WHOLE_BINARY
    }
}

/// The stable identity of one test.
#[must_use]
pub fn target_id(package: &str, unit: UnitKind, path: &str) -> String {
    let mut hasher = Sha256::new();
    for field in [TARGET_DOMAIN, package, unit.name(), path] {
        let length = u32::try_from(field.len()).unwrap_or(u32::MAX);
        hasher.update(length.to_be_bytes());
        hasher.update(field.as_bytes());
    }
    let digest = hex::encode(hasher.finalize());
    digest
        .get(..TARGET_ID_HEX_LENGTH)
        .unwrap_or(&digest)
        .to_owned()
}

/// What a `--list --format terse` line said something was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A `#[test]`.
    Test,
    /// A `#[bench]`, which this runner does not measure.
    Benchmark,
}

/// One line of a terse listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The libtest path.
    pub path: String,
    /// What it is.
    pub kind: EntryKind,
}

/// Reads a `--list --format terse` listing.
///
/// A line that is not `path: test` or `path: benchmark` yields nothing:
/// a listing that could not be read is no listing, never a shorter one,
/// because a shorter one is exactly what a missed test looks like.
#[must_use]
pub fn parse_list(stdout: &[u8]) -> Vec<Entry> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| {
            let (path, kind) = line.rsplit_once(": ")?;
            if path.is_empty() {
                return None;
            }
            let kind = match kind.trim() {
                "test" => EntryKind::Test,
                "benchmark" => EntryKind::Benchmark,
                _ => return None,
            };
            Some(Entry {
                path: path.to_owned(),
                kind,
            })
        })
        .collect()
}

/// The failure modes of this module, each with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetErrorKind {
    /// A test binary could not be asked what it holds.
    ListFailed,
}

impl TargetErrorKind {
    /// Every kind, in code order.
    pub const ALL: [Self; 1] = [Self::ListFailed];

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(self) -> ErrorCode {
        match self {
            Self::ListFailed => error::TARGET_LIST_FAILED,
        }
    }
}

/// Why a unit's tests could not be named.
#[derive(Debug, thiserror::Error)]
#[error("{}: {unit}: {message}", kind.code().code)]
pub struct TargetError {
    kind: TargetErrorKind,
    unit: String,
    message: String,
}

impl TargetError {
    /// The failure mode.
    #[must_use]
    pub const fn kind(&self) -> TargetErrorKind {
        self.kind
    }

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.kind.code()
    }
}

/// Asks a built binary what tests it holds.
///
/// A binary that cannot answer — `harness = false`, or a harness that does
/// not implement `--list` — is one target for the whole binary rather than
/// an error: the tests inside it are real, and a run that refused to
/// measure them would be less useful than one that says it could not route
/// them.
///
/// # Errors
///
/// [`TargetErrorKind::ListFailed`] when the binary could not be started at
/// all, which is not the same as a binary that answered differently.
pub fn enumerate(unit: &Unit, watch: Watch<'_>) -> Result<Vec<Target>, TargetError> {
    let mut spec = Spec::new([
        unit.executable.as_os_str().to_owned(),
        std::ffi::OsString::from("--list"),
        std::ffi::OsString::from("--format"),
        std::ffi::OsString::from("terse"),
    ]);
    spec.dir = Some(unit.cwd.clone());
    spec.timeout = Some(LIST_TIMEOUT);
    spec.structured_stdout = Some(64 << 20);
    let listed = run(&spec, watch.cancel);
    watch.trace.exec(ExecRecord::of(&spec, &listed));
    if let Some(error) = &listed.error {
        return Err(TargetError {
            kind: TargetErrorKind::ListFailed,
            unit: unit.name.clone(),
            message: format!("cannot start the test binary: {error}"),
        });
    }
    let entries = if listed.ok() {
        parse_list(&listed.stdout)
    } else {
        Vec::new()
    };
    if entries.is_empty() {
        return Ok(vec![whole_binary(unit)]);
    }

    let mut ignored = ignored_paths(unit, watch);
    let mut targets: Vec<Target> = entries
        .into_iter()
        .filter(|entry| entry.kind == EntryKind::Test)
        .map(|entry| Target {
            id: target_id(&unit.package, unit.kind, &entry.path),
            package: unit.package.clone(),
            unit: unit.kind,
            unit_name: unit.name.clone(),
            ignored: ignored.remove(&entry.path),
            path: entry.path,
            executable: unit.executable.clone(),
            cwd: unit.cwd.clone(),
        })
        .collect();
    if targets.is_empty() {
        // Benchmarks only: nothing this runner measures.
        return Ok(Vec::new());
    }
    targets.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(targets)
}

/// The one target a binary with its own harness has.
fn whole_binary(unit: &Unit) -> Target {
    Target {
        id: target_id(&unit.package, unit.kind, WHOLE_BINARY),
        package: unit.package.clone(),
        unit: unit.kind,
        unit_name: unit.name.clone(),
        path: WHOLE_BINARY.to_owned(),
        ignored: false,
        executable: unit.executable.clone(),
        cwd: unit.cwd.clone(),
    }
}

/// Which of a binary's tests libtest will skip unless asked.
///
/// A binary that will not answer this second question is not an error: the
/// worst outcome is that a run treats an ignored test as ordinary and reads
/// its "0 passed, 1 ignored" honestly anyway.
fn ignored_paths(unit: &Unit, watch: Watch<'_>) -> std::collections::BTreeSet<String> {
    let mut spec = Spec::new([
        unit.executable.as_os_str().to_owned(),
        std::ffi::OsString::from("--list"),
        std::ffi::OsString::from("--ignored"),
        std::ffi::OsString::from("--format"),
        std::ffi::OsString::from("terse"),
    ]);
    spec.dir = Some(unit.cwd.clone());
    spec.timeout = Some(LIST_TIMEOUT);
    spec.structured_stdout = Some(64 << 20);
    let listed = run(&spec, watch.cancel);
    watch.trace.exec(ExecRecord::of(&spec, &listed));
    if !listed.ok() {
        return std::collections::BTreeSet::new();
    }
    parse_list(&listed.stdout)
        .into_iter()
        .filter(|entry| entry.kind == EntryKind::Test)
        .map(|entry| entry.path)
        .collect()
}
