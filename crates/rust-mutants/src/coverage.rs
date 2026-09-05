// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which regions of which files one test reached.
//!
//! The columns `llvm-cov` reports are 1-based byte columns, the same unit the
//! engine's positions use, so a region and a mutant can be compared directly.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::runner::{Spec, run};

use crate::error::{self, ErrorCode};
use crate::runner::Watch;

/// What the tree is built with so that every region is instrumented.
pub const INSTRUMENT_FLAG: &str = "-C instrument-coverage";

/// Where a test process writes what it executed.
pub const PROFILE_ENV: &str = "LLVM_PROFILE_FILE";

/// The region kind that is ordinary code. Every other kind — an expansion, a skipped region, a gap, a branch — says something about the shape of the source rather than about what ran.
pub const REGION_KIND_CODE: u32 = 0;

/// A place in a file: a 1-based line and a 1-based byte column, which is the unit `llvm-cov` reports regions in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Point {
    /// The 1-based line.
    pub line: u32,
    /// The 1-based byte column.
    pub column: u32,
}

/// One region of one file, as `llvm-cov` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    /// Where it starts.
    pub start: Point,
    /// Where it ends, exclusive.
    pub end: Point,
    /// How many times it ran.
    pub count: u64,
    /// What kind of region it is; see [`REGION_KIND_CODE`].
    pub kind: u32,
}

/// The regions of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRegions {
    /// The file, as the export names it.
    pub path: PathBuf,
    /// Its regions, in the order the export listed them.
    pub regions: Vec<Region>,
}

/// One stretch of source a test really ran.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Block {
    /// The file.
    pub file: PathBuf,
    /// Where it starts.
    pub start: Point,
    /// Where it ends, exclusive.
    pub end: Point,
}

impl Block {
    /// Whether `position` in `file` lies inside this block. The column is a byte column; see the module documentation.
    #[must_use]
    pub fn contains(&self, file: &Path, position: Point) -> bool {
        self.file == file && position >= self.start && position < self.end
    }
}

/// The failure modes of this module, each with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoverageErrorKind {
    /// A coverage export could not be read.
    Unreadable,
    /// The LLVM tools the toolchain ships are not installed.
    ToolsMissing,
    /// One of them failed.
    ToolFailed,
    /// A test process wrote no profile at all.
    NothingWritten,
}

impl CoverageErrorKind {
    /// Every kind, in code order.
    pub const ALL: [Self; 4] = [
        Self::Unreadable,
        Self::ToolsMissing,
        Self::ToolFailed,
        Self::NothingWritten,
    ];

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(self) -> ErrorCode {
        match self {
            Self::Unreadable => error::COVERAGE_UNREADABLE,
            Self::ToolsMissing => error::COVERAGE_TOOLS_MISSING,
            Self::ToolFailed => error::COVERAGE_TOOL_FAILED,
            Self::NothingWritten => error::COVERAGE_NOTHING_WRITTEN,
        }
    }
}

/// Why coverage could not be read.
#[derive(Debug, thiserror::Error)]
#[error("{}: {message}", kind.code().code)]
pub struct CoverageError {
    kind: CoverageErrorKind,
    message: String,
}

impl CoverageError {
    /// What the tools, or the export, actually said.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The failure mode.
    #[must_use]
    pub const fn kind(&self) -> CoverageErrorKind {
        self.kind
    }

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.kind.code()
    }
}

/// The document `llvm-cov export --format=text` prints.
#[derive(Debug, Deserialize)]
struct Export {
    #[serde(rename = "type")]
    kind: String,
    data: Vec<Datum>,
}

#[derive(Debug, Deserialize)]
struct Datum {
    #[serde(default)]
    functions: Vec<Function>,
}

#[derive(Debug, Deserialize)]
struct Function {
    filenames: Vec<PathBuf>,
    regions: Vec<Vec<u64>>,
}

/// Reads a coverage export into the regions of each file.
///
/// # Errors
/// [`CoverageErrorKind::Unreadable`].
pub fn parse_export(json: &[u8]) -> Result<Vec<FileRegions>, CoverageError> {
    let refuse = |message: String| CoverageError {
        kind: CoverageErrorKind::Unreadable,
        message,
    };
    let export: Export = serde_json::from_slice(json)
        .map_err(|error| refuse(format!("the coverage export could not be read: {error}")))?;
    if export.kind != "llvm.coverage.json.export" {
        return Err(refuse(format!(
            "the document is a {:?}, not a coverage export",
            export.kind
        )));
    }
    let mut by_file: std::collections::BTreeMap<PathBuf, Vec<Region>> =
        std::collections::BTreeMap::new();
    for datum in &export.data {
        for function in &datum.functions {
            for region in &function.regions {
                let (path, region) = read_region(function, region).map_err(refuse)?;
                by_file.entry(path).or_default().push(region);
            }
        }
    }
    Ok(by_file
        .into_iter()
        .map(|(path, regions)| FileRegions { path, regions })
        .collect())
}

/// One region tuple, with the file its `file_id` names.
fn read_region(function: &Function, region: &[u64]) -> Result<(PathBuf, Region), String> {
    let field = |at: usize| -> Result<u64, String> {
        region
            .get(at)
            .copied()
            .ok_or_else(|| format!("a region has {} fields, not 8", region.len()))
    };
    let small = |at: usize| -> Result<u32, String> {
        u32::try_from(field(at)?).map_err(|_error| "a region field is out of range".to_owned())
    };
    let file_id = usize::try_from(field(5)?)
        .map_err(|_error| "a region names a file id out of range".to_owned())?;
    let path = function
        .filenames
        .get(file_id)
        .ok_or_else(|| format!("a region names file {file_id}, which the function does not have"))?
        .clone();
    Ok((
        path,
        Region {
            start: Point {
                line: small(0)?,
                column: small(1)?,
            },
            end: Point {
                line: small(2)?,
                column: small(3)?,
            },
            count: field(4)?,
            kind: small(7)?,
        },
    ))
}

/// The blocks a run really executed: the code regions with a non-zero count, deduplicated, in file and position order.
#[must_use]
pub fn covered(files: &[FileRegions]) -> BTreeSet<Block> {
    files
        .iter()
        .flat_map(|file| {
            file.regions
                .iter()
                .filter(|region| region.kind == REGION_KIND_CODE && region.count > 0)
                .map(|region| Block {
                    file: file.path.clone(),
                    start: region.start,
                    end: region.end,
                })
        })
        .collect()
}

/// The regions a build instrumented, whether or not they ran: every code region of the export.
#[must_use]
pub fn instrumented(files: &[FileRegions]) -> BTreeSet<Block> {
    files
        .iter()
        .flat_map(|file| {
            file.regions
                .iter()
                .filter(|region| region.kind == REGION_KIND_CODE)
                .map(|region| Block {
                    file: file.path.clone(),
                    start: region.start,
                    end: region.end,
                })
        })
        .collect()
}

/// The two LLVM tools a coverage run drives, as rustup ships them beside the compiler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    /// `llvm-profdata`, which merges what the processes wrote.
    pub profdata: PathBuf,
    /// `llvm-cov`, which turns a merged profile into regions.
    pub cov: PathBuf,
}

impl Tools {
    /// Finds the tools beside the compiler that will build the tree.
    ///
    /// # Errors
    /// [`CoverageErrorKind::ToolsMissing`] when rustc could not be asked or
    /// the component is not installed.
    pub fn locate<W: Watch>(
        toolchain: &crate::cargo::Toolchain,
        dir: &Path,
        watch: &W,
    ) -> Result<Self, CoverageError> {
        let refuse = |message: String| CoverageError {
            kind: CoverageErrorKind::ToolsMissing,
            message,
        };
        let mut spec = Spec::new([
            toolchain.rustc().as_os_str().to_owned(),
            std::ffi::OsString::from("--print"),
            std::ffi::OsString::from("target-libdir"),
        ]);
        spec.dir = Some(dir.to_path_buf());
        spec.env = toolchain
            .env()
            .map(<[(std::ffi::OsString, std::ffi::OsString)]>::to_vec);
        spec.structured_stdout = Some(64 * 1024);
        let printed = run(&spec, watch.cancel());
        watch.exec(&spec, &printed);
        if !printed.ok() {
            return Err(refuse(format!(
                "cannot ask {} where its libraries are: {}",
                toolchain.rustc().display(),
                String::from_utf8_lossy(&printed.output).trim()
            )));
        }
        let libdir = PathBuf::from(String::from_utf8_lossy(&printed.stdout).trim().to_owned());
        let bin = libdir
            .parent()
            .ok_or_else(|| refuse(format!("{} has no parent", libdir.display())))?
            .join("bin");
        let profdata = bin.join(executable_name("llvm-profdata"));
        let cov = bin.join(executable_name("llvm-cov"));
        for tool in [&profdata, &cov] {
            if !tool.is_file() {
                return Err(refuse(format!(
                    "{} is missing; install the llvm-tools component (rustup component add llvm-tools)",
                    tool.display()
                )));
            }
        }
        Ok(Self { profdata, cov })
    }

    /// Merges what one target's processes wrote into one profile.
    ///
    /// # Errors
    /// [`CoverageErrorKind::ToolFailed`] with what the tool said.
    pub fn merge<W: Watch>(
        &self,
        raw: &[PathBuf],
        into: &Path,
        watch: &W,
    ) -> Result<(), CoverageError> {
        if raw.is_empty() {
            return Err(CoverageError {
                kind: CoverageErrorKind::NothingWritten,
                message: "the test process wrote no profile; the build was not instrumented, \
                          or the process did not exit normally"
                    .to_owned(),
            });
        }
        let mut argv = vec![
            self.profdata.as_os_str().to_owned(),
            std::ffi::OsString::from("merge"),
            std::ffi::OsString::from("-sparse"),
            std::ffi::OsString::from("-o"),
            into.as_os_str().to_owned(),
        ];
        argv.extend(raw.iter().map(|path| path.as_os_str().to_owned()));
        let spec = Spec::new(argv);
        let merged = run(&spec, watch.cancel());
        watch.exec(&spec, &merged);
        if merged.ok() {
            Ok(())
        } else {
            Err(CoverageError {
                kind: CoverageErrorKind::ToolFailed,
                message: format!(
                    "llvm-profdata merge failed: {}",
                    String::from_utf8_lossy(&merged.output).trim()
                ),
            })
        }
    }

    /// Turns a merged profile and the binary that wrote it into regions.
    ///
    /// # Errors
    /// [`CoverageErrorKind::ToolFailed`] and the refusals of
    /// [`parse_export`].
    pub fn export<W: Watch>(
        &self,
        profile: &Path,
        binary: &Path,
        watch: &W,
    ) -> Result<Vec<FileRegions>, CoverageError> {
        let mut spec = Spec::new([
            self.cov.as_os_str().to_owned(),
            std::ffi::OsString::from("export"),
            std::ffi::OsString::from(format!("--instr-profile={}", profile.display())),
            std::ffi::OsString::from("--format=text"),
            binary.as_os_str().to_owned(),
        ]);
        spec.structured_stdout = Some(1 << 30);
        let exported = run(&spec, watch.cancel());
        watch.exec(&spec, &exported);
        if !exported.ok() {
            return Err(CoverageError {
                kind: CoverageErrorKind::ToolFailed,
                message: format!(
                    "llvm-cov export failed: {}",
                    String::from_utf8_lossy(&exported.output).trim()
                ),
            });
        }
        if exported.stdout_truncated {
            return Err(CoverageError {
                kind: CoverageErrorKind::ToolFailed,
                message: "llvm-cov export printed more than the runner keeps".to_owned(),
            });
        }
        parse_export(&exported.stdout)
    }
}

/// The name of an executable on this platform.
fn executable_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_owned()
    }
}

/// The profile file pattern one target writes to: `%p` per process, so a test that forks is measured whole.
#[must_use]
pub fn profile_pattern(directory: &Path, target_id: &str) -> PathBuf {
    directory.join(format!("{target_id}.%p.profraw"))
}

/// The raw profiles one target wrote, in name order.
///
/// # Errors
/// [`CoverageErrorKind::NothingWritten`] when the directory cannot be read.
pub fn written_profiles(directory: &Path, target_id: &str) -> Result<Vec<PathBuf>, CoverageError> {
    let entries = std::fs::read_dir(directory).map_err(|error| CoverageError {
        kind: CoverageErrorKind::NothingWritten,
        message: format!("cannot read {}: {error}", directory.display()),
    })?;
    let mut profiles: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "profraw")
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&format!("{target_id}.")))
        })
        .collect();
    profiles.sort();
    Ok(profiles)
}
