// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Toolchain content identities memoized only while their filesystem change stamps agree.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use super::{File, file};
use crate::cargo::{CompileOptions, Toolchain};
use crate::vars::Variables;

#[derive(Debug, PartialEq, Eq)]
struct Stamp {
    length: u64,
    modified: SystemTime,
    readonly: bool,
    #[cfg(windows)]
    changed: (crate::capdir::Identity, i64),
    #[cfg(unix)]
    changed: (i64, i64, u64),
}

type Memo = BTreeMap<PathBuf, (Stamp, File)>;

#[derive(Debug, Clone)]
pub(in crate::cargo) struct Identities {
    files: Arc<Mutex<Memo>>,
}

impl Identities {
    pub(in crate::cargo) fn empty() -> Self {
        Self {
            files: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}

pub(super) fn inputs(
    toolchain: &Toolchain,
    options: &CompileOptions,
    env: &Variables,
    files: &mut BTreeMap<PathBuf, File>,
) -> io::Result<()> {
    let root = toolchain
        .sysroot()
        .ok_or_else(|| io::Error::other("unbound compiler sysroot"))?;
    environment(root, toolchain.rustc(), env)?;
    for path in [toolchain.cargo(), toolchain.rustc()] {
        files.insert(path.to_path_buf(), identity(path, toolchain.identities())?);
    }
    let target = match &options.build.target {
        Some(target)
            if Path::new(target)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json")) =>
        {
            return Err(io::Error::other("custom target toolchain inputs"));
        }
        Some(target) => target.as_str(),
        None => toolchain.host(),
    };
    directory(
        &root.join("lib/rustlib").join(target).join("lib"),
        files,
        toolchain.identities(),
    )?;
    let tools = root.join("lib/rustlib").join(toolchain.host()).join("bin");
    directory(&tools, files, toolchain.identities())?;
    for entry in std::fs::read_dir(root.join("lib"))? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let path = entry.path();
            files.insert(path.clone(), identity(&path, toolchain.identities())?);
        }
    }
    #[cfg(windows)]
    for entry in std::fs::read_dir(root.join("bin"))? {
        let entry = entry?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
        {
            files.insert(path.clone(), identity(&path, toolchain.identities())?);
        }
    }
    Ok(())
}

pub(in crate::cargo) fn environment(root: &Path, rustc: &Path, env: &Variables) -> io::Result<()> {
    for (name, value) in env.canonical() {
        let Some(name) = name.to_str() else {
            return Err(io::Error::other("non-textual environment name"));
        };
        let known_compiler = name == "RUSTC" && Path::new(&value) == rustc
            || name == "RUSTDOC"
                && Path::new(&value)
                    == root
                        .join("bin")
                        .join(format!("rustdoc{}", std::env::consts::EXE_SUFFIX));
        if !value.is_empty() && !known_compiler && opaque_variable(name) {
            return Err(io::Error::other(format!(
                "the {name} input names an opaque compiler, linker or loader graph"
            )));
        }
    }
    Ok(())
}

fn opaque_variable(name: &str) -> bool {
    matches!(
        name,
        "RUSTC_WRAPPER"
            | "RUSTC_WORKSPACE_WRAPPER"
            | "RUSTC"
            | "RUSTDOC"
            | "LD_PRELOAD"
            | "LD_LIBRARY_PATH"
            | "DYLD_INSERT_LIBRARIES"
            | "DYLD_LIBRARY_PATH"
            | "DYLD_FRAMEWORK_PATH"
            | "DYLD_FALLBACK_LIBRARY_PATH"
            | "DYLD_FALLBACK_FRAMEWORK_PATH"
            | "COMPILER_PATH"
            | "GCC_EXEC_PREFIX"
            | "LIBRARY_PATH"
    ) || name.starts_with("CARGO_")
        && [
            "_LINKER",
            "_RUNNER",
            "_RUSTC",
            "_RUSTDOC",
            "_RUSTC_WRAPPER",
            "_RUSTC_WORKSPACE_WRAPPER",
            "_RUSTFLAGS",
            "_RUSTDOCFLAGS",
        ]
        .iter()
        .any(|suffix| name.ends_with(suffix))
        && !matches!(
            name,
            "CARGO_ENCODED_RUSTFLAGS" | "CARGO_ENCODED_RUSTDOCFLAGS"
        )
}

fn directory(
    root: &Path,
    files: &mut BTreeMap<PathBuf, File>,
    identities: &Identities,
) -> io::Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            directory(&path, files, identities)?;
        } else {
            files.insert(path.clone(), identity(&path, identities)?);
        }
    }
    Ok(())
}

fn stamp(path: &Path) -> io::Result<Stamp> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::other("a toolchain input is not a regular file"));
    }
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt as _;
        (metadata.ctime(), metadata.ctime_nsec(), metadata.ino())
    };
    Ok(Stamp {
        length: metadata.len(),
        modified: metadata.modified()?,
        readonly: metadata.permissions().readonly(),
        #[cfg(windows)]
        changed: crate::capdir::change_stamp(&std::fs::File::open(path)?)?,
        #[cfg(unix)]
        changed,
    })
}

fn identity(path: &Path, identities: &Identities) -> io::Result<File> {
    let before = stamp(path)?;
    let mut memo = identities
        .files
        .lock()
        .map_err(|source| io::Error::other(source.to_string()))?;
    if let Some((previous, identity)) = memo.get(path)
        && reusable(previous, &before)
    {
        return Ok(identity.clone());
    }
    let identity = file(path)?;
    if stamp(path)? != before {
        return Err(io::Error::other(
            "the toolchain changed while its content was read",
        ));
    }
    memo.insert(path.to_path_buf(), (before, identity.clone()));
    drop(memo);
    Ok(identity)
}

fn reusable(previous: &Stamp, current: &Stamp) -> bool {
    #[cfg(windows)]
    if current.changed.1 <= 0 {
        return false;
    }
    previous == current
}

#[cfg(test)]
mod tests {
    #[test]
    fn changed_toolchain_bytes_are_hashed_even_when_the_old_mtime_is_restored() {
        let directory = tempfile::tempdir().expect("toolchain identity");
        let path = directory.path().join("compiler");
        std::fs::write(&path, b"first").expect("original bytes");
        let modified = std::fs::metadata(&path)
            .expect("original stamp")
            .modified()
            .expect("original mtime");
        let identities = super::Identities::empty();
        let before = super::identity(&path, &identities).expect("original digest");
        std::fs::write(&path, b"other").expect("changed bytes of the same length");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("compiler metadata")
            .set_modified(modified)
            .expect("restore the original mtime");
        let after = super::identity(&path, &identities).expect("changed digest");
        assert_ne!(before.digest, after.digest);
    }
}
