// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading one package's sources for what they can start: every `.rs` file under its manifest directory, outside build output and hidden directories.

use std::path::Path;

use super::proof::PackageScan;
use super::scan::scanned;

/// What every source of `package`, and every file the compiler read for its crates, can start, with each file it could not read named rather than skipped.
#[must_use]
pub fn package(package: &rust_mutants::cargo::Package, compiled: &Compiled) -> PackageScan {
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
    pub sources: std::collections::BTreeMap<String, std::collections::BTreeSet<std::path::PathBuf>>,
    /// The packages, by name, whose build script output asks the linker to link a library or pass it an argument.
    pub linking: std::collections::BTreeSet<String>,
}

impl Compiled {
    /// What the build under `target_dir` compiled: every rustc dependency file and every build script output under it.
    #[must_use]
    pub fn read(target_dir: &Path) -> Self {
        let mut compiled = Self::default();
        let profiles: Vec<std::path::PathBuf> = children(target_dir)
            .into_iter()
            .flat_map(|child| {
                let nested = children(&child);
                std::iter::once(child).chain(nested)
            })
            .collect();
        for profile in profiles {
            for file in children(&profile.join("deps")) {
                let named = file.file_name().and_then(std::ffi::OsStr::to_str);
                let Some(stem) = named.and_then(|named| named.strip_suffix(".d")) else {
                    continue;
                };
                let Some(unit) = before_the_hash(stem) else {
                    continue;
                };
                if let Some(text) = text_of(&file) {
                    compiled
                        .sources
                        .entry(unit.to_owned())
                        .or_default()
                        .extend(listed(&text));
                }
            }
            for unit in children(&profile.join("build")) {
                let named = unit.file_name().and_then(std::ffi::OsStr::to_str);
                let Some(package) = named.and_then(before_the_hash) else {
                    continue;
                };
                if text_of(&unit.join("output")).is_some_and(|text| links_something(&text)) {
                    compiled.linking.insert(package.to_owned());
                }
            }
        }
        compiled
    }
}

/// The directories directly under `directory`, and none where it cannot be listed.
fn children(directory: &Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        found.push(entry.path());
    }
    found
}

/// The name in `<name>-<hash>`, which is how cargo spells a unit's dependency file and build directory.
fn before_the_hash(spelled: &str) -> Option<&str> {
    spelled.rfind('-').and_then(|at| spelled.get(..at))
}

/// Every source a rustc dependency file lists, with the escaped spaces of its paths put back.
fn listed(text: &str) -> Vec<std::path::PathBuf> {
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
                        found.push(std::path::PathBuf::from(std::mem::take(&mut word)));
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
#[must_use]
pub fn compiled_directory(
    (label, links): (&str, bool),
    root: &Path,
    (crates, compiled): (&[String], &Compiled),
) -> PackageScan {
    let package = label.split('@').next().unwrap_or(label);
    let mut unread = Vec::new();
    let mut files = Vec::new();
    walked(root, root, &mut files, &mut unread);
    let mut listed: Vec<&std::path::PathBuf> = Vec::new();
    for name in crates {
        if let Some(sources) = compiled.sources.get(name) {
            listed.extend(sources);
        }
    }
    let rust = |path: &Path| path.extension().is_some_and(|extension| extension == "rs");
    let includes_code = listed
        .iter()
        .filter(|path| rust(path))
        .any(|path| text_of(path).is_some_and(|text| text.contains("include!(")));
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
fn read_all(mut scan: PackageScan, root: &Path, mut files: Vec<String>) -> PackageScan {
    files.sort();
    for relative in files {
        let at = if Path::new(&relative).is_absolute() {
            std::path::PathBuf::from(&relative)
        } else {
            root.join(&relative)
        };
        let Some(text) = text_of(&at) else {
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
    scan
}

/// The file's text, or nothing where it cannot be read or is not UTF-8.
fn text_of(path: &Path) -> Option<String> {
    match std::fs::read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => Some(text),
            Err(_not_utf8) => None,
        },
        Err(_unreadable) => None,
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
fn walked(root: &Path, directory: &Path, files: &mut Vec<String>, unread: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        unread.push(relative(root, directory));
        return;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            unread.push(relative(root, directory));
            continue;
        };
        let path = entry.path();
        let name = entry.file_name();
        let hidden = name.as_encoded_bytes().first() == Some(&b'.');
        let Ok(kind) = entry.file_type() else {
            unread.push(relative(root, &path));
            continue;
        };
        if kind.is_dir() {
            if !hidden && name != "target" {
                walked(root, &path, files, unread);
            }
        } else if kind.is_file() && path.extension().is_some_and(|extension| extension == "rs") {
            files.push(relative(root, &path));
        }
    }
}
