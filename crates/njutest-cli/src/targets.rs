// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run measures: one test, named the way a person names it.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use crate::trace::ExecRecord;
use rust_mutants::runner::{Spec, run};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use crate::error::{self, ErrorCode};
use crate::watch::Watch;

/// The domain separator hashed first for every target identity. It carries the recipe version.
pub const TARGET_DOMAIN: &str = "njutest-target-v2";

/// The complete SHA-256 target identity in lowercase hexadecimal.
pub const TARGET_ID_HEX_LENGTH: usize = 64;

/// A target identity in its one canonical spelling.
///
/// The private field prevents arbitrary report text from becoming an identity.
/// Every constructed or deserialized value is exactly 64 lowercase hexadecimal
/// characters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TargetId(String);

impl TargetId {
    /// Parses the one canonical wire spelling.
    ///
    /// # Errors
    /// Returns the exact violated identity invariant.
    pub fn parse(value: &str) -> Result<Self, TargetIdError> {
        if value.len() != TARGET_ID_HEX_LENGTH {
            return Err(TargetIdError::Length {
                actual: value.len(),
            });
        }
        if let Some((index, byte)) = value
            .bytes()
            .enumerate()
            .find(|(_index, byte)| !matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(TargetIdError::Character { index, byte });
        }
        Ok(Self(value.to_owned()))
    }

    /// Borrows the canonical wire spelling explicitly.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TargetId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for TargetId {
    type Err = TargetIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl Serialize for TargetId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TargetId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// Why text cannot be a target identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TargetIdError {
    /// The spelling is not the complete SHA-256 digest length.
    #[error("a target identity is {TARGET_ID_HEX_LENGTH} bytes, not {actual}")]
    Length {
        /// The byte length that was supplied.
        actual: usize,
    },
    /// The spelling contains something other than lowercase hexadecimal.
    #[error("target identity byte {index} is 0x{byte:02x}, not lowercase hexadecimal")]
    Character {
        /// The zero-based byte position.
        index: usize,
        /// The noncanonical byte.
        byte: u8,
    },
    /// One identity field cannot be framed by the stable 64-bit recipe.
    #[error("target identity field {field} is longer than the v2 64-bit frame")]
    FieldTooLong {
        /// The recipe field whose length could not be framed.
        field: &'static str,
    },
}

/// The path of the one target a binary with its own harness has.
pub const WHOLE_BINARY: &str = "";

/// How long listing a binary's tests may take. Listing is a process start and a print; a binary that has not answered by now is not going to.
pub const LIST_TIMEOUT: Duration = Duration::from_secs(60);

/// The kind of compilation unit a test lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, njutest_macros::AllVariants)]
pub enum UnitKind {
    /// The library's own tests.
    Lib,
    /// A binary's own tests.
    Bin,
    /// An integration test.
    Test,
    /// An example built with `test = true`.
    Example,
    /// A procedural macro crate's own unit tests.
    ProcMacro,
    /// The documentation of a library, run as one target.
    Doc,
}

impl UnitKind {
    /// The name used in an identity and in reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Lib => "lib",
            Self::Bin => "bin",
            Self::Test => "test",
            Self::Example => "example",
            Self::ProcMacro => "proc-macro",
            Self::Doc => "doc",
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
            TargetKind::Test => Self::Test,
            TargetKind::Example => Self::Example,
            TargetKind::ProcMacro => Self::ProcMacro,
            TargetKind::Doc => Self::Doc,
            TargetKind::Bin => Self::Bin,
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
    /// Whether the binary implements libtest's listing protocol.
    pub harness: bool,
    /// The binary cargo built.
    pub executable: PathBuf,
    /// The directory it runs in.
    pub cwd: PathBuf,
    /// The complete environment its processes run with: the run's own, what cargo sets for this target, and the scratch build layer. An argument rather than an inheritance, so a listing and an execution see the same machine and a test can say what that is.
    pub env: Vec<(OsString, OsString)>,
}

/// One test a run measures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The stable identity; see the module documentation.
    pub id: TargetId,
    /// The package that owns it.
    pub package: String,
    /// What kind of unit it lives in.
    pub unit: UnitKind,
    /// The cargo target's name.
    pub unit_name: String,
    /// The libtest path, or [`WHOLE_BINARY`] for a binary with its own harness.
    pub path: String,
    /// Whether libtest will skip it unless asked.
    pub ignored: bool,
    /// The binary that runs it.
    pub executable: PathBuf,
    /// The directory it runs in.
    pub cwd: PathBuf,
    /// The complete environment its process runs with, from the unit it came from.
    pub env: Vec<(OsString, OsString)>,
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

    /// Whether the whole binary is the target, because libtest is not there to be asked.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn is_whole_binary(&self) -> bool {
        self.path == WHOLE_BINARY
    }
}

/// The stable identity of one test: the binary it is in, and its path inside that binary.
/// # Errors
/// Returns [`TargetIdError::FieldTooLong`] if this platform can represent a
/// string too large for the stable 64-bit length frame.
pub fn target_id(
    package: &str,
    unit: UnitKind,
    unit_name: &str,
    path: &str,
) -> Result<TargetId, TargetIdError> {
    let mut hasher = Sha256::new();
    for (name, field) in [
        ("domain", TARGET_DOMAIN),
        ("package", package),
        ("unit-kind", unit.name()),
        ("unit-name", unit_name),
        ("test-path", path),
    ] {
        let length = u64::try_from(field.len())
            .map_err(|_overflow| TargetIdError::FieldTooLong { field: name })?;
        hasher.update(length.to_be_bytes());
        hasher.update(field.as_bytes());
    }
    Ok(TargetId(hex::encode(hasher.finalize())))
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

/// Why a successful libtest listing cannot be interpreted exactly.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ListParseError {
    /// Libtest's protocol is UTF-8, and replacement characters would change a test's name.
    #[error("the test listing is not UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    /// The input contains more lines than this platform can number.
    #[error("the test listing has more lines than this platform can number")]
    TooManyLines,
    /// An explicit empty line would otherwise disappear from the listing.
    #[error("test listing line {line} is empty")]
    EmptyLine {
        /// The one-based protocol line.
        line: usize,
    },
    /// A nonempty line is not the exact `path: kind` protocol shape.
    #[error("test listing line {line} is not `path: kind`: {text:?}")]
    Shape {
        /// The one-based protocol line.
        line: usize,
        /// The exact malformed text.
        text: String,
    },
    /// The path half of a protocol line is empty.
    #[error("test listing line {line} has an empty test path")]
    EmptyPath {
        /// The one-based protocol line.
        line: usize,
    },
    /// The kind is not one this version can classify without guessing.
    #[error("test listing line {line} has unknown kind {kind:?}")]
    UnknownKind {
        /// The one-based protocol line.
        line: usize,
        /// The unrecognised spelling.
        kind: String,
    },
}

/// Reads a `--list --format terse` listing without dropping malformed lines.
///
/// # Errors
/// Returns the first invalid UTF-8 byte or line that is not exactly a known
/// libtest entry. An entirely empty stream is a valid listing with no tests;
/// an explicit empty line is malformed protocol input.
pub fn parse_list(stdout: &[u8]) -> Result<Vec<Entry>, ListParseError> {
    std::str::from_utf8(stdout)?
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let line_number = index.checked_add(1).ok_or(ListParseError::TooManyLines)?;
            if line.is_empty() {
                return Err(ListParseError::EmptyLine { line: line_number });
            }
            let Some((path, kind)) = line.rsplit_once(": ") else {
                return Err(ListParseError::Shape {
                    line: line_number,
                    text: line.to_owned(),
                });
            };
            if path.is_empty() {
                return Err(ListParseError::EmptyPath { line: line_number });
            }
            let kind = match kind {
                "test" => EntryKind::Test,
                "benchmark" => EntryKind::Benchmark,
                _ => {
                    return Err(ListParseError::UnknownKind {
                        line: line_number,
                        kind: kind.to_owned(),
                    });
                }
            };
            Ok(Entry {
                path: path.to_owned(),
                kind,
            })
        })
        .collect()
}

/// The failure modes of this module, each with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, njutest_macros::AllVariants)]
pub enum TargetErrorKind {
    /// A test binary could not be asked what it holds.
    ListFailed,
}

impl TargetErrorKind {
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
    fn list(unit: &Unit, error: impl fmt::Display) -> Self {
        Self::invalid(unit.name.clone(), error)
    }

    pub(crate) fn invalid(unit: impl Into<String>, error: impl fmt::Display) -> Self {
        Self {
            kind: TargetErrorKind::ListFailed,
            unit: unit.into(),
            message: error.to_string(),
        }
    }

    /// A failure of `kind` about `unit`, saying `message`.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn new(kind: TargetErrorKind, unit: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind,
            unit: unit.into(),
            message: message.into(),
        }
    }

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.kind.code()
    }
}

/// Asks a built binary what tests it holds.
///
/// # Errors
/// [`TargetErrorKind::ListFailed`] when the binary could not be started at
/// all, which is not the same as a binary that answered differently.
pub fn enumerate(unit: &Unit, watch: Watch<'_>) -> Result<Vec<Target>, TargetError> {
    let mut spec = Spec::new(
        [
            unit.executable.as_os_str().to_owned(),
            OsString::from("--list"),
            OsString::from("--format"),
            OsString::from("terse"),
        ],
        rust_mutants::runner::Bound::After(rust_mutants::runner::PROBE),
    );
    spec.dir = Some(unit.cwd.clone());
    spec.env = Some(unit.env.clone());
    spec.timeout = Some(LIST_TIMEOUT);
    spec.structured_stdout = Some(64 << 20);
    let listed = run(&spec, watch.cancel);
    watch.trace.exec_result(ExecRecord::of(&spec, &listed));
    if let Some(error) = listed.error() {
        return Err(TargetError {
            kind: TargetErrorKind::ListFailed,
            unit: unit.name.clone(),
            message: format!("cannot start the test binary: {error}"),
        });
    }
    if listed.stdout_truncated {
        return Err(TargetError::invalid(
            &unit.name,
            "the test listing exceeded its complete structured-output bound",
        ));
    }
    if !listed.succeeded() {
        if !unit.harness {
            return whole_binary(unit)
                .map(|target| vec![target])
                .map_err(|error| TargetError::list(unit, error));
        }
        return Err(TargetError::invalid(
            &unit.name,
            format!(
                "the libtest listing process ended as {:?}",
                listed.termination
            ),
        ));
    }
    let entries = parse_list(&listed.stdout).map_err(|error| TargetError::list(unit, error))?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let mut ignored = ignored_paths(unit, watch)?;
    let mut targets: Vec<Target> = entries
        .into_iter()
        .filter(|entry| entry.kind == EntryKind::Test)
        .map(|entry| {
            Ok(Target {
                id: target_id(&unit.package, unit.kind, &unit.name, &entry.path)?,
                package: unit.package.clone(),
                unit: unit.kind,
                unit_name: unit.name.clone(),
                ignored: ignored.remove(&entry.path),
                path: entry.path,
                executable: unit.executable.clone(),
                cwd: unit.cwd.clone(),
                env: unit.env.clone(),
            })
        })
        .collect::<Result<_, TargetIdError>>()
        .map_err(|error| TargetError::list(unit, error))?;
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    targets.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(targets)
}

/// The one target a binary with its own harness has.
/// # Errors
/// Returns an identity framing error rather than manufacturing an identifier.
pub fn whole_binary(unit: &Unit) -> Result<Target, TargetIdError> {
    Ok(Target {
        id: target_id(&unit.package, unit.kind, &unit.name, WHOLE_BINARY)?,
        package: unit.package.clone(),
        unit: unit.kind,
        unit_name: unit.name.clone(),
        path: WHOLE_BINARY.to_owned(),
        ignored: false,
        executable: unit.executable.clone(),
        cwd: unit.cwd.clone(),
        env: unit.env.clone(),
    })
}

/// Which of a binary's tests libtest will skip unless asked.
fn ignored_paths(
    unit: &Unit,
    watch: Watch<'_>,
) -> Result<std::collections::BTreeSet<String>, TargetError> {
    let mut spec = Spec::new(
        [
            unit.executable.as_os_str().to_owned(),
            OsString::from("--list"),
            OsString::from("--ignored"),
            OsString::from("--format"),
            OsString::from("terse"),
        ],
        rust_mutants::runner::Bound::After(rust_mutants::runner::PROBE),
    );
    spec.dir = Some(unit.cwd.clone());
    spec.env = Some(unit.env.clone());
    spec.timeout = Some(LIST_TIMEOUT);
    spec.structured_stdout = Some(64 << 20);
    let listed = run(&spec, watch.cancel);
    watch.trace.exec_result(ExecRecord::of(&spec, &listed));
    if let Some(error) = listed.error() {
        return Err(TargetError::invalid(
            &unit.name,
            format!("cannot list ignored tests: {error}"),
        ));
    }
    if listed.stdout_truncated {
        return Err(TargetError::invalid(
            &unit.name,
            "the ignored-test listing exceeded its complete structured-output bound",
        ));
    }
    if !listed.succeeded() {
        return Err(TargetError::invalid(
            &unit.name,
            format!(
                "the ignored-test listing process ended as {:?}",
                listed.termination
            ),
        ));
    }
    Ok(parse_list(&listed.stdout)
        .map_err(|error| TargetError::list(unit, error))?
        .into_iter()
        .filter(|entry| entry.kind == EntryKind::Test)
        .map(|entry| entry.path)
        .collect())
}
