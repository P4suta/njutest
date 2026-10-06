// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Lossless original source archives with a closed regular-file inventory.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::Digest as _;

const SCHEMA: &str = "njutest-original-source-v1";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema: String,
    archive: String,
    files: BTreeMap<String, File>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct File {
    sha256: String,
    mode: u32,
}

#[derive(Debug)]
struct Bytes {
    body: Vec<u8>,
    mode: u32,
}

/// A complete original source tree held as verified immutable bytes before extraction or reuse.
#[derive(Debug)]
pub struct OriginalTree {
    files: BTreeMap<String, Bytes>,
}

impl OriginalTree {
    /// Records the complete pre-producer tree without changing any source or configuration byte.
    ///
    /// # Errors
    /// An input is unreadable, unsafe, repeated or not a regular file, or publication fails.
    pub fn record(root: &Path, directory: &Path) -> io::Result<()> {
        let files = files(root)?;
        let mut archive = tar::Builder::new(Vec::new());
        for (relative, file) in &files {
            let mut header = tar::Header::new_ustar();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_size(u64::try_from(file.body.len()).map_err(io::Error::other)?);
            header.set_mode(file.mode);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(0);
            archive.append_data(&mut header, relative, file.body.as_slice())?;
        }
        let archive = archive.into_inner()?;
        let binding = Binding {
            schema: SCHEMA.to_owned(),
            archive: digest(&archive),
            files: bound(&files),
        };
        std::fs::create_dir_all(directory)?;
        std::fs::write(directory.join("source.tar"), archive)?;
        std::fs::write(
            directory.join("source.json"),
            serde_json::to_vec_pretty(&binding).map_err(io::Error::other)?,
        )?;
        Ok(())
    }

    /// Reads every archive entry and refuses an incomplete, hidden, mismatched or unsafe source.
    ///
    /// # Errors
    /// The directory, schema, complete inventory, archive digest, file bytes or permissions differ.
    pub fn read(directory: &Path) -> io::Result<Self> {
        let encoded = files(directory)?;
        if !encoded
            .keys()
            .map(String::as_str)
            .eq(["source.json", "source.tar"])
        {
            return Err(io::Error::other(
                "the original archive directory is not a closed source pair",
            ));
        }
        let manifest = encoded
            .get("source.json")
            .ok_or_else(|| io::Error::other("the original source binding is missing"))?;
        let archive = encoded
            .get("source.tar")
            .ok_or_else(|| io::Error::other("the original source archive is missing"))?;
        let binding: Binding =
            crate::strictjson::decode_slice(&manifest.body).map_err(io::Error::other)?;
        if binding.schema != SCHEMA
            || binding.files.is_empty()
            || binding.archive != digest(&archive.body)
        {
            return Err(io::Error::other(
                "the original source schema, archive or inventory differs",
            ));
        }
        let files = archived(&archive.body)?;
        if bound(&files) != binding.files {
            return Err(io::Error::other(
                "the complete original archive contains missing, hidden or mismatched files",
            ));
        }
        Ok(Self { files })
    }

    /// The independently derived digest of every original file, including manifests and configuration.
    #[must_use]
    pub fn digests(&self) -> BTreeMap<String, String> {
        self.files
            .iter()
            .map(|(name, bytes)| (name.clone(), digest(&bytes.body)))
            .collect()
    }

    /// Holds a current source tree to every original byte and permission before paying for a live run.
    ///
    /// # Errors
    /// The current tree is incomplete, contains a hidden file or differs from the actual original.
    pub fn check_source(&self, root: &Path) -> io::Result<()> {
        let actual = files(root)?;
        if !actual.keys().eq(self.files.keys())
            || actual.iter().any(|(name, file)| {
                !self
                    .files
                    .get(name)
                    .is_some_and(|held| held.body == file.body && modes_agree(held.mode, file.mode))
            })
        {
            return Err(io::Error::other(
                "the current complete source and configuration differ from the original producer input",
            ));
        }
        Ok(())
    }

    /// Extracts held bytes into a new owned directory without following archive paths or overwriting data.
    ///
    /// # Errors
    /// A directory or file cannot be created or its exact bytes and permissions cannot be restored.
    pub fn extract(&self) -> io::Result<tempfile::TempDir> {
        let root = tempfile::Builder::new()
            .prefix("njutest-original-")
            .tempdir()?;
        for (relative, bytes) in &self.files {
            let path = root.path().join(super::safe(relative)?);
            let parent = path
                .parent()
                .ok_or_else(|| io::Error::other("an original file has no parent"))?;
            std::fs::create_dir_all(parent)?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            io::Write::write_all(&mut file, &bytes.body)?;
            permissions(&path, bytes.mode)?;
        }
        self.check_source(root.path())?;
        Ok(root)
    }
}

fn archived(bytes: &[u8]) -> io::Result<BTreeMap<String, Bytes>> {
    let mut archive = tar::Archive::new(bytes);
    archive.set_ignore_zeros(true);
    let mut files = BTreeMap::new();
    let mut portable = BTreeSet::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.header().entry_type() != tar::EntryType::Regular {
            return Err(io::Error::other(
                "an original archive entry is not a regular file",
            ));
        }
        let path = entry.path()?.into_owned();
        let relative = path
            .to_str()
            .ok_or_else(|| io::Error::other("an original path is not UTF-8"))?;
        let relative = super::safe(relative)?
            .to_str()
            .ok_or_else(|| io::Error::other("an original path is not UTF-8"))?
            .to_owned();
        if !portable.insert(relative.to_lowercase()) {
            return Err(io::Error::other(
                "an original archive repeats a portable path identity",
            ));
        }
        let mode = entry.header().mode()?;
        if mode & !0o777 != 0 {
            return Err(io::Error::other(
                "an original archive carries special permission bits",
            ));
        }
        let mut body = Vec::new();
        entry.read_to_end(&mut body)?;
        if files.insert(relative, Bytes { body, mode }).is_some() {
            return Err(io::Error::other(
                "an original archive repeats a source path",
            ));
        }
    }
    if files.is_empty() {
        return Err(io::Error::other("an original archive holds no source"));
    }
    Ok(files)
}

pub(super) fn producer_patch(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut files = archived(bytes)?;
    if !files.keys().map(String::as_str).eq(["producer.patch"]) {
        return Err(io::Error::other(
            "the producer source archive is not its complete original patch",
        ));
    }
    files
        .remove("producer.patch")
        .map(|file| file.body)
        .ok_or_else(|| io::Error::other("the producer source archive holds no original patch"))
}

fn files(root: &Path) -> io::Result<BTreeMap<String, Bytes>> {
    if !std::fs::symlink_metadata(root)?.file_type().is_dir() {
        return Err(io::Error::other(
            "an original source root is not an owned directory",
        ));
    }
    let mut files = BTreeMap::new();
    let mut directories: Vec<PathBuf> = vec![root.to_path_buf()];
    let mut portable = BTreeSet::new();
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_dir() {
                directories.push(path);
            } else if metadata.file_type().is_file() {
                let relative = relative(root, &path)?;
                super::safe(&relative)?;
                if !portable.insert(relative.to_lowercase()) {
                    return Err(io::Error::other(
                        "an original source repeats a portable path identity",
                    ));
                }
                let body = std::fs::read(&path)?;
                let mode = mode(&metadata);
                if mode & !0o777 != 0 {
                    return Err(io::Error::other(
                        "an original source carries special permission bits",
                    ));
                }
                if files.insert(relative, Bytes { body, mode }).is_some() {
                    return Err(io::Error::other("an original source repeats a path"));
                }
            } else {
                return Err(io::Error::other(
                    "an original source is not a regular file or directory",
                ));
            }
        }
    }
    if files.is_empty() {
        return Err(io::Error::other("an original source tree is empty"));
    }
    Ok(files)
}

fn relative(root: &Path, path: &Path) -> io::Result<String> {
    path.strip_prefix(root)
        .map_err(io::Error::other)?
        .components()
        .map(|part| match part {
            std::path::Component::Normal(name) => name
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| io::Error::other("an original path is not UTF-8")),
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::CurDir
            | std::path::Component::ParentDir => Err(io::Error::other(
                "an original path is not a relative file identity",
            )),
        })
        .collect::<io::Result<Vec<_>>>()
        .map(|parts| parts.join("/"))
}

fn bound(files: &BTreeMap<String, Bytes>) -> BTreeMap<String, File> {
    files
        .iter()
        .map(|(name, bytes)| {
            (
                name.clone(),
                File {
                    sha256: digest(&bytes.body),
                    mode: bytes.mode,
                },
            )
        })
        .collect()
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

#[cfg(unix)]
fn mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o7777
}

#[cfg(unix)]
const fn modes_agree(held: u32, actual: u32) -> bool {
    held == actual
}

#[cfg(windows)]
const fn modes_agree(held: u32, actual: u32) -> bool {
    (held & 0o200 == 0) == (actual & 0o200 == 0)
}

#[cfg(windows)]
fn mode(metadata: &std::fs::Metadata) -> u32 {
    if metadata.permissions().readonly() {
        0o444
    } else {
        0o644
    }
}

#[cfg(unix)]
fn permissions(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(windows)]
fn permissions(path: &Path, mode: u32) -> io::Result<()> {
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_readonly(mode & 0o200 == 0);
    std::fs::set_permissions(path, permissions)
}
