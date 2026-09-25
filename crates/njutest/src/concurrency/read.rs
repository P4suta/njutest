// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading one package's sources for what they can start: every `.rs` file under its manifest directory outside build output and hidden directories, and every file the compiler read for its crates.

use std::path::{Path, PathBuf};

use super::proof::PackageScan;
use super::scan::scanned;
use crate::observe::SourceReadError;
use crate::observe::{Kind, Observed, listing, text};

/// What every source of `package`, and every file the compiler read for its units, can start, with each file it could not read named rather than skipped.
///
/// # Errors
/// [`SourceReadError::Exhausted`] where the process ran out of descriptors or memory while reading.
pub fn package(
    package: &rust_mutants::cargo::Package,
    compiled: &Compiled,
) -> Result<PackageScan, SourceReadError> {
    compiled_directory(
        (
            &format!("{}@{}", package.name, package.version),
            package.links.is_some(),
        ),
        package.manifest_dir(),
        (&package.id, compiled),
    )
}

/// What the build of one session compiled, as cargo reported it: every file the compiler read for each package's units, and every package whose build script told the linker to link something.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Compiled {
    /// Every file the compiler read for the units of each package, by package id.
    pub inputs: std::collections::BTreeMap<String, std::collections::BTreeSet<PathBuf>>,
    /// The packages, by id, whose build script asked the linker to link a library or pass it an argument, or whose build script output cannot be read to say it did not.
    pub linking: std::collections::BTreeSet<String>,
}

impl Compiled {
    /// What `compilation` says was compiled and linked.
    ///
    /// A build script's `-l` and `rustc-link-lib` reach cargo's own message; a `rustc-link-arg` does not, so the output it left beside the directory cargo gave it is read for that, and an output that is not there to read is taken to link.
    ///
    /// # Errors
    /// [`SourceReadError::Exhausted`] where the process ran out of descriptors or memory while reading.
    pub fn of(compilation: &rust_mutants::cargo::Compilation) -> Result<Self, SourceReadError> {
        let mut compiled = Self::default();
        for unit in &compilation.units {
            compiled
                .inputs
                .entry(unit.package_id.clone())
                .or_default()
                .extend(unit.inputs.iter().cloned());
        }
        for script in &compilation.build_scripts {
            let links = !script.linked_libs.is_empty()
                || match script.out_dir.as_deref().and_then(Path::parent) {
                    Some(unit) => match text(&unit.join("output"))? {
                        Observed::Present(output) => links_something(&output),
                        Observed::Absent | Observed::Unreadable => true,
                    },
                    None => true,
                };
            if links {
                compiled.linking.insert(script.package_id.clone());
            }
        }
        Ok(compiled)
    }
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

/// What every source under `root`, and every file the compiler read for the units of the package `id`, can start.
///
/// A file the compiler read that is not `.rs` is read as code only where a source of the package asks for code with `include!`: `include_str!` and `include_bytes!` read files the compiler lists too, a README behind a crate's documentation most often, and those are data.
///
/// # Errors
/// [`SourceReadError::Exhausted`] where the process ran out of descriptors or memory while reading.
pub fn compiled_directory(
    (label, links): (&str, bool),
    root: &Path,
    (id, compiled): (&str, &Compiled),
) -> Result<PackageScan, SourceReadError> {
    let mut unread = Vec::new();
    let mut files = Vec::new();
    walked(root, root, &mut files, &mut unread)?;
    let listed: Vec<&PathBuf> = compiled
        .inputs
        .get(id)
        .map_or_else(Vec::new, |inputs| inputs.iter().collect());
    let rust = |path: &Path| path.extension().is_some_and(|extension| extension == "rs");
    let mut includes_code = false;
    for path in listed.iter().filter(|path| rust(path)) {
        if let Observed::Present(source) = text(path)? {
            includes_code |= super::scan::includes_code(&source);
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
            links: links || compiled.linking.contains(id),
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
