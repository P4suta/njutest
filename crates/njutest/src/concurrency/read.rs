// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading one package's sources for what they can start: every `.rs` file under its manifest directory outside build output and hidden directories, and every file the compiler read for its crates.

use std::path::{Path, PathBuf};

use super::proof::PackageScan;
use super::scan::scanned;
use crate::observe::SourceReadError;
use crate::observe::{Kind, Observed, listing, text};

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
    /// What the build under `target_dir` compiled: every rustc dependency file and every build script output in each profile directory under it, and under each target triple's.
    ///
    /// # Errors
    /// [`SourceReadError::Exhausted`] where the process ran out of descriptors or memory while reading.
    pub fn read(target_dir: &Path) -> Result<Self, SourceReadError> {
        let mut compiled = Self::default();
        let mut profiles: Vec<PathBuf> = Vec::new();
        for child in compiled.directories(target_dir)? {
            let nested = compiled.directories(&child)?;
            profiles.push(child);
            profiles.extend(nested);
        }
        for profile in profiles {
            for file in compiled.entries(&profile.join("deps"), Kind::File)? {
                let named = file.file_name().and_then(std::ffi::OsStr::to_str);
                let Some(stem) = named.and_then(|named| named.strip_suffix(".d")) else {
                    continue;
                };
                let Some(unit) = before_the_hash(stem) else {
                    continue;
                };
                match text(&file)? {
                    Observed::Present(text) => compiled
                        .sources
                        .entry(unit.to_owned())
                        .or_default()
                        .extend(listed(&text)),
                    Observed::Absent | Observed::Unreadable => {
                        compiled
                            .unknown
                            .entry(unit.to_owned())
                            .or_default()
                            .push(file.clone());
                    }
                }
            }
            for unit in compiled.directories(&profile.join("build"))? {
                let named = unit.file_name().and_then(std::ffi::OsStr::to_str);
                let Some(package) = named.and_then(before_the_hash) else {
                    continue;
                };
                let links = match text(&unit.join("output"))? {
                    Observed::Present(text) => links_something(&text),
                    Observed::Absent => false,
                    Observed::Unreadable => true,
                };
                if links {
                    compiled.linking.insert(package.to_owned());
                }
            }
        }
        if compiled.sources.is_empty() && compiled.unknown.is_empty() {
            compiled.unknown_everywhere(target_dir);
        }
        Ok(compiled)
    }

    /// The directories directly under `directory`.
    fn directories(&mut self, directory: &Path) -> Result<Vec<PathBuf>, SourceReadError> {
        self.entries(directory, Kind::Directory)
    }

    /// The entries of `kind` directly under `directory`: none where nothing is there, and `directory` held as unknown where it is there and cannot be listed or an entry's type cannot be told.
    fn entries(&mut self, directory: &Path, kind: Kind) -> Result<Vec<PathBuf>, SourceReadError> {
        match listing(directory)? {
            Observed::Present(entries) => {
                if entries.iter().any(|entry| entry.kind == Kind::Unknown) {
                    self.unknown_everywhere(directory);
                }
                Ok(entries
                    .into_iter()
                    .filter(|entry| entry.kind == kind)
                    .map(|entry| entry.path)
                    .collect())
            }
            Observed::Absent => Ok(Vec::new()),
            Observed::Unreadable => {
                self.unknown_everywhere(directory);
                Ok(Vec::new())
            }
        }
    }

    /// Holds `path` as something that leaves every crate's compiled files unknown.
    fn unknown_everywhere(&mut self, path: &Path) {
        self.unknown
            .entry(String::new())
            .or_default()
            .push(path.to_path_buf());
    }
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
        if let Observed::Present(text) = text(path)? {
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
        let Observed::Present(text) = text(&at)? else {
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

/// Every `.rs` file under `directory`, relative to `root`; a directory that cannot be listed, and an entry whose type cannot be told, is named as unread.
fn walked(
    root: &Path,
    directory: &Path,
    files: &mut Vec<String>,
    unread: &mut Vec<String>,
) -> Result<(), SourceReadError> {
    let Observed::Present(entries) = listing(directory)? else {
        unread.push(relative(root, directory));
        return Ok(());
    };
    for entry in entries {
        let hidden = entry
            .path
            .file_name()
            .is_some_and(|name| name.as_encoded_bytes().first() == Some(&b'.'));
        let build_output = entry.path.file_name().is_some_and(|name| name == "target");
        match entry.kind {
            Kind::Directory if !hidden && !build_output => {
                walked(root, &entry.path, files, unread)?;
            }
            Kind::File
                if entry
                    .path
                    .extension()
                    .is_some_and(|extension| extension == "rs") =>
            {
                files.push(relative(root, &entry.path));
            }
            Kind::Unknown => unread.push(relative(root, &entry.path)),
            Kind::Directory | Kind::File | Kind::Other => {}
        }
    }
    Ok(())
}
