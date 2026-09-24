// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading one package's sources for what they can start: every `.rs` file under its manifest directory, outside build output and hidden directories.

use std::path::{Path, PathBuf};

use super::proof::PackageScan;
use super::scan::scanned;
use crate::error::{self, ErrorCode};

/// Why the sources a proof rests on could not be read just now, which says nothing about the sources themselves.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SourceReadError {
    /// The process ran out of something every read needs, file descriptors or memory, so what it could not open is not known to be unreadable.
    #[error(
        "{}: reading {} ran out of what every read needs: {source}",
        error::SOURCES_UNREADABLE.code,
        path.display()
    )]
    Exhausted {
        /// What was being read.
        path: PathBuf,
        /// The operating system's refusal.
        #[source]
        source: std::io::Error,
    },
}

impl SourceReadError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Exhausted { .. } => error::SOURCES_UNREADABLE,
        }
    }
}

/// What reading one file came to.
enum Text {
    /// Its text.
    Read(String),
    /// Nothing is there.
    Absent,
    /// It is there, and it cannot be read or is not UTF-8.
    Unreadable,
}

/// What every source of `package`, and every file the compiler read for its crates, can start, with each file it could not read named rather than skipped.
///
/// # Errors
/// [`SourceReadError::Exhausted`] where the process ran out of descriptors or memory while reading.
pub fn package(
    package: &rust_mutants::cargo::Package,
    compiled: &Compiled,
) -> Result<PackageScan, SourceReadError> {
    let crates: Vec<String> = package
        .targets
        .iter()
        .filter(|target| !target.kind.iter().any(|kind| kind == "custom-build"))
        .map(|target| target.name.replace('-', "_"))
        .collect();
    compiled_directory(
        (
            &format!("{}@{}", package.name, package.version),
            package.links.is_some(),
        ),
        package.manifest_dir(),
        (&crates, compiled),
    )
}

/// What the build of one session says was compiled: every source file rustc read for each crate, and every package whose build script told the linker to link something.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Compiled {
    /// The source files of each crate, by crate name, as rustc's dependency files list them.
    pub sources: std::collections::BTreeMap<String, std::collections::BTreeSet<PathBuf>>,
    /// The packages, by name, whose build script output asks the linker to link a library or pass it an argument, or cannot be read to say it does not.
    pub linking: std::collections::BTreeSet<String>,
    /// What says which files a crate compiled and could not be read, by crate name, and under `""` what leaves every crate's files unknown.
    pub unknown: std::collections::BTreeMap<String, Vec<PathBuf>>,
}

impl Compiled {
    /// What the build under `target_dir` compiled: every rustc dependency file and every build script output under it.
    ///
    /// # Errors
    /// [`SourceReadError::Exhausted`] where the process ran out of descriptors or memory while reading.
    pub fn read(target_dir: &Path) -> Result<Self, SourceReadError> {
        let mut compiled = Self::default();
        let mut profiles: Vec<PathBuf> = Vec::new();
        for child in children(target_dir, &mut compiled)? {
            let nested = children(&child, &mut compiled)?;
            profiles.push(child);
            profiles.extend(nested);
        }
        for profile in profiles {
            for file in children(&profile.join("deps"), &mut compiled)? {
                let named = file.file_name().and_then(std::ffi::OsStr::to_str);
                let Some(stem) = named.and_then(|named| named.strip_suffix(".d")) else {
                    continue;
                };
                let Some(unit) = before_the_hash(stem) else {
                    continue;
                };
                match text_of(&file)? {
                    Text::Read(text) => compiled
                        .sources
                        .entry(unit.to_owned())
                        .or_default()
                        .extend(listed(&text)),
                    Text::Absent | Text::Unreadable => {
                        compiled
                            .unknown
                            .entry(unit.to_owned())
                            .or_default()
                            .push(file.clone());
                    }
                }
            }
            for unit in children(&profile.join("build"), &mut compiled)? {
                let named = unit.file_name().and_then(std::ffi::OsStr::to_str);
                let Some(package) = named.and_then(before_the_hash) else {
                    continue;
                };
                let links = match text_of(&unit.join("output"))? {
                    Text::Read(text) => links_something(&text),
                    Text::Absent => false,
                    Text::Unreadable => true,
                };
                if links {
                    compiled.linking.insert(package.to_owned());
                }
            }
        }
        if compiled.sources.is_empty() && compiled.unknown.is_empty() {
            compiled
                .unknown
                .entry(String::new())
                .or_default()
                .push(target_dir.to_path_buf());
        }
        Ok(compiled)
    }
}

/// The entries directly under `directory`: none where nothing is there, and `directory` held in `compiled` as unknown where it is there and cannot be listed.
fn children(directory: &Path, compiled: &mut Compiled) -> Result<Vec<PathBuf>, SourceReadError> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            exhausted(directory, source)?;
            compiled
                .unknown
                .entry(String::new())
                .or_default()
                .push(directory.to_path_buf());
            return Ok(Vec::new());
        }
    };
    let mut found = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => found.push(entry.path()),
            Err(source) => {
                exhausted(directory, source)?;
                compiled
                    .unknown
                    .entry(String::new())
                    .or_default()
                    .push(directory.to_path_buf());
            }
        }
    }
    Ok(found)
}

/// Refuses a failure that is the process running out of descriptors or memory, and lets every other one through as a fact about the path.
fn exhausted(path: &Path, source: std::io::Error) -> Result<(), SourceReadError> {
    if source.kind() == std::io::ErrorKind::OutOfMemory
        || source.raw_os_error().is_some_and(out_of_descriptors)
    {
        return Err(SourceReadError::Exhausted {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

/// Whether `code` says the process or the system has no descriptor left to open a file with.
#[cfg(unix)]
const fn out_of_descriptors(code: i32) -> bool {
    code == rustix::io::Errno::MFILE.raw_os_error()
        || code == rustix::io::Errno::NFILE.raw_os_error()
}

/// Whether `code` is Windows saying the process has no handle left to open a file with.
#[cfg(windows)]
const fn out_of_descriptors(code: i32) -> bool {
    const ERROR_TOO_MANY_OPEN_FILES: i32 = 4;
    code == ERROR_TOO_MANY_OPEN_FILES
}

/// Whether `code` says no descriptor is left, which this platform does not say.
#[cfg(not(any(unix, windows)))]
const fn out_of_descriptors(_code: i32) -> bool {
    false
}

/// The name in `<name>-<hash>`, which is how cargo spells a unit's dependency file and build directory.
fn before_the_hash(spelled: &str) -> Option<&str> {
    spelled.rfind('-').and_then(|at| spelled.get(..at))
}

/// Every source a rustc dependency file lists, with the escaped spaces of its paths put back.
fn listed(text: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for line in text.lines() {
        let Some(sources) = line
            .find(": ")
            .and_then(|at| line.get(at.saturating_add(2)..))
        else {
            continue;
        };
        let mut word = String::new();
        let mut escaped = false;
        for character in sources.chars().chain(std::iter::once(' ')) {
            match (escaped, character) {
                (false, '\\') => escaped = true,
                (false, ' ') => {
                    if !word.is_empty() {
                        found.push(PathBuf::from(std::mem::take(&mut word)));
                    }
                }
                (_, other) => {
                    escaped = false;
                    word.push(other);
                }
            }
        }
    }
    found
}

/// Whether a build script's output asks the linker to link a library or pass it an argument, in either spelling cargo reads.
fn links_something(output: &str) -> bool {
    output.lines().any(|line| {
        let asked = line
            .strip_prefix("cargo::")
            .or_else(|| line.strip_prefix("cargo:"))
            .unwrap_or_default();
        asked.starts_with("rustc-link-lib")
            || asked.starts_with("rustc-link-arg")
            || asked.starts_with("rustc-cdylib-link-arg")
    })
}

/// What every source under `root`, and every file the compiler read for the crates named `crates`, can start.
///
/// A file the compiler read that is not `.rs` is read as code only where a source of those crates uses `include!`: `include_str!` and `include_bytes!` read files the compiler lists too, a README behind a crate's documentation most often, and those are data.
///
/// # Errors
/// [`SourceReadError::Exhausted`] where the process ran out of descriptors or memory while reading.
pub fn compiled_directory(
    (label, links): (&str, bool),
    root: &Path,
    (crates, compiled): (&[String], &Compiled),
) -> Result<PackageScan, SourceReadError> {
    let package = label.split('@').next().unwrap_or(label);
    let mut unread = Vec::new();
    let mut files = Vec::new();
    walked(root, root, &mut files, &mut unread)?;
    let mut listed: Vec<&PathBuf> = Vec::new();
    for name in crates.iter().map(String::as_str).chain(std::iter::once("")) {
        if let Some(sources) = compiled.sources.get(name) {
            listed.extend(sources);
        }
        if let Some(unknown) = compiled.unknown.get(name) {
            unread.extend(unknown.iter().map(|path| path.display().to_string()));
        }
    }
    let rust = |path: &Path| path.extension().is_some_and(|extension| extension == "rs");
    let mut includes_code = false;
    for path in listed.iter().filter(|path| rust(path)) {
        if let Text::Read(text) = text_of(path)? {
            includes_code |= text.contains("include!(");
        }
    }
    for source in listed {
        if !rust(source) && !includes_code {
            continue;
        }
        let spelled = relative(root, source);
        if !files.contains(&spelled) {
            files.push(spelled);
        }
    }
    read_all(
        PackageScan {
            package: label.to_owned(),
            links: links || compiled.linking.contains(package),
            found: Vec::new(),
            unread,
        },
        root,
        files,
    )
}

/// `scan` with what each of `files`, relative to `root` or spelled whole, can start.
fn read_all(
    mut scan: PackageScan,
    root: &Path,
    mut files: Vec<String>,
) -> Result<PackageScan, SourceReadError> {
    files.sort();
    for relative in files {
        let at = if Path::new(&relative).is_absolute() {
            PathBuf::from(&relative)
        } else {
            root.join(&relative)
        };
        let Text::Read(text) = text_of(&at)? else {
            scan.unread.push(relative);
            continue;
        };
        match scanned(&relative, &text) {
            Ok(found) => scan
                .found
                .extend(found.into_iter().map(|one| (relative.clone(), one))),
            Err(_not_rust) => scan.unread.push(relative),
        }
    }
    Ok(scan)
}

/// What reading the file at `path` came to.
fn text_of(path: &Path) -> Result<Text, SourceReadError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(match String::from_utf8(bytes) {
            Ok(text) => Text::Read(text),
            Err(_not_utf8) => Text::Unreadable,
        }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(Text::Absent),
        Err(source) => {
            exhausted(path, source)?;
            Ok(Text::Unreadable)
        }
    }
}

/// `path` relative to `root`, with `/` between parts, or its whole spelling where it is outside `root` or a part of it is not UTF-8.
fn relative(root: &Path, path: &Path) -> String {
    let parts: Option<Vec<&str>> = match path.strip_prefix(root) {
        Ok(inside) => inside
            .components()
            .map(|part| part.as_os_str().to_str())
            .collect(),
        Err(_outside) => None,
    };
    parts.map_or_else(|| path.display().to_string(), |parts| parts.join("/"))
}

/// Every `.rs` file under `directory`, relative to `root`; a directory that cannot be listed is named as unread.
fn walked(
    root: &Path,
    directory: &Path,
    files: &mut Vec<String>,
    unread: &mut Vec<String>,
) -> Result<(), SourceReadError> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(source) => {
            exhausted(directory, source)?;
            unread.push(relative(root, directory));
            return Ok(());
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(source) => {
                exhausted(directory, source)?;
                unread.push(relative(root, directory));
                continue;
            }
        };
        let path = entry.path();
        let name = entry.file_name();
        let hidden = name.as_encoded_bytes().first() == Some(&b'.');
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(source) => {
                exhausted(&path, source)?;
                unread.push(relative(root, &path));
                continue;
            }
        };
        if kind.is_dir() {
            if !hidden && name != "target" {
                walked(root, &path, files, unread)?;
            }
        } else if kind.is_file() && path.extension().is_some_and(|extension| extension == "rs") {
            files.push(relative(root, &path));
        }
    }
    Ok(())
}
