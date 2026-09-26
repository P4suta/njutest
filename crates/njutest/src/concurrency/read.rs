// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading one package's sources for what they can start: every `.rs` file under its manifest directory, outside build output and hidden directories.

use std::path::Path;

use super::proof::PackageScan;
use super::scan::scanned;

/// What every source of `package` can start, with each file it could not read named rather than skipped.
#[must_use]
pub fn package(package: &rust_mutants::cargo::Package) -> PackageScan {
    directory(
        &format!("{}@{}", package.name, package.version),
        package.links.is_some(),
        package.manifest_dir(),
    )
}

/// What every source under `root`, the directory of the package named `label`, can start.
#[must_use]
pub fn directory(label: &str, links: bool, root: &Path) -> PackageScan {
    let mut scan = PackageScan {
        package: label.to_owned(),
        links,
        found: Vec::new(),
        unread: Vec::new(),
    };
    let mut files = Vec::new();
    walked(root, root, &mut files, &mut scan.unread);
    files.sort();
    for relative in files {
        let Some(text) = text_of(&root.join(&relative)) else {
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
