// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Owned fallback-library search inputs include each namespace, absence, alias and file content.

use std::ffi::OsStr;
use std::io;
use std::path::Path;

use sha2::{Digest as _, Sha256};

use super::toolchain::Identities;
use crate::capdir::{Dir, Entry, Kind, Name};
use crate::vars::Variables;

#[derive(Debug)]
pub(in crate::cargo) struct Inputs {
    fallback: Option<std::ffi::OsString>,
    digest: String,
}

impl Inputs {
    pub(in crate::cargo) fn of(env: &Variables, identities: &Identities) -> io::Result<Self> {
        let fallback = if cfg!(target_os = "macos") {
            env.var("DYLD_FALLBACK_LIBRARY_PATH")
                .filter(|value| !value.is_empty())
                .map(OsStr::to_os_string)
        } else {
            None
        };
        let mut digest = Sha256::new();
        super::field(&mut digest, b"fallback-library-inputs-v1");
        if let Some(value) = &fallback {
            super::field(&mut digest, value.as_encoded_bytes());
            for path in std::env::split_paths(value) {
                capture(&path, identities, &mut digest).map_err(|source| refused(&path, source))?;
            }
        }
        Ok(Self {
            fallback,
            digest: hex::encode(digest.finalize()),
        })
    }

    pub(in crate::cargo) fn admits(&self, name: &str, value: &OsStr) -> bool {
        name == "DYLD_FALLBACK_LIBRARY_PATH" && self.fallback.as_deref() == Some(value)
    }

    pub(in crate::cargo) fn digest(&self) -> &str {
        &self.digest
    }
}

fn capture(path: &Path, identities: &Identities, digest: &mut Sha256) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "a loader search directory must be absolute",
        ));
    }
    super::field(digest, path.as_os_str().as_encoded_bytes());
    let canonical = match std::fs::canonicalize(path) {
        Ok(canonical) => canonical,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            super::field(digest, b"absent");
            return Ok(());
        }
        Err(source) => return Err(source),
    };
    super::field(digest, canonical.as_os_str().as_encoded_bytes());
    let directory = Dir::open(&canonical)?;
    let before = directory.status()?;
    super::field(digest, &before.identity.volume.to_be_bytes());
    super::field(digest, &before.identity.object.to_be_bytes());
    let mut entries = directory.entries()?;
    entries.sort();
    for entry in &entries {
        capture_entry((&directory, &canonical), entry, identities, digest)?;
    }
    let mut after = directory.entries()?;
    after.sort();
    if entries != after
        || Dir::open(&std::fs::canonicalize(path)?)?.status()?.identity != before.identity
    {
        return Err(io::Error::other("the loader search namespace changed"));
    }
    Ok(())
}

fn capture_entry(
    (directory, root): (&Dir, &Path),
    text: &str,
    identities: &Identities,
    digest: &mut Sha256,
) -> io::Result<()> {
    let name = Name::new(text).map_err(io::Error::other)?;
    let path = root.join(text);
    let status = directory
        .status_at(name)?
        .ok_or_else(|| io::Error::other("a loader search entry disappeared"))?;
    super::field(digest, text.as_bytes());
    match status.kind {
        Kind::Directory => super::field(digest, b"directory"),
        Kind::File => match directory.open_entry(name)? {
            Entry::File(file) => captured_file(&path, &file, identities, digest)?,
            Entry::Dir(_) | Entry::Other => {
                return Err(io::Error::other("the loader search entry changed kind"));
            }
        },
        Kind::Other => capture_link(&path, identities, digest)?,
    }
    if directory.status_at(name)? != Some(status) {
        return Err(io::Error::other("the loader search entry changed identity"));
    }
    Ok(())
}

fn capture_link(path: &Path, identities: &Identities, digest: &mut Sha256) -> io::Result<()> {
    let target = std::fs::read_link(path)?;
    super::field(digest, target.as_os_str().as_encoded_bytes());
    match std::fs::canonicalize(path) {
        Ok(canonical) => {
            let file = crate::capdir::open_file_at(&canonical)?;
            let status = crate::capdir::file_status(&file)?;
            match status.kind {
                Kind::File => captured_file(&canonical, &file, identities, digest)?,
                Kind::Directory => {
                    super::field(digest, b"directory-alias");
                    super::field(digest, canonical.as_os_str().as_encoded_bytes());
                    super::field(digest, &status.identity.volume.to_be_bytes());
                    super::field(digest, &status.identity.object.to_be_bytes());
                }
                Kind::Other => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "a loader alias target is not a file or directory",
                    ));
                }
            }
            if std::fs::canonicalize(path)? != canonical
                || crate::capdir::file_status(&file)? != status
            {
                return Err(io::Error::other("the loader alias target changed"));
            }
            Ok(())
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            super::field(digest, b"absent-link-target");
            Ok(())
        }
        Err(source) => Err(source),
    }
}

fn captured_file(
    path: &Path,
    file: &std::fs::File,
    identities: &Identities,
    digest: &mut Sha256,
) -> io::Result<()> {
    let held = crate::capdir::file_status(file)?;
    match held.kind {
        Kind::File => {}
        Kind::Directory | Kind::Other => {
            return Err(io::Error::other("a loader input is not a regular file"));
        }
    }
    let before = crate::capdir::open_file_at(path)?;
    if crate::capdir::file_status(&before)? != held {
        return Err(io::Error::other("the loader input changed identity"));
    }
    let state = super::toolchain::identity(path, identities)?;
    let after = crate::capdir::open_file_at(path)?;
    if crate::capdir::file_status(&after)? != held {
        return Err(io::Error::other("the loader input changed identity"));
    }
    super::field(digest, path.as_os_str().as_encoded_bytes());
    super::field(digest, state.digest.as_bytes());
    super::field(digest, &state.mode.to_be_bytes());
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
#[error("{code}: the loader input {} could not be bound ({:?})", path.display(), source.kind(), code = LoaderInputError::code().code)]
struct LoaderInputError {
    path: std::path::PathBuf,
    source: io::Error,
}

impl LoaderInputError {
    const fn code() -> crate::error::ErrorCode {
        crate::error::COMPILER_INPUT_UNREADABLE
    }
}

fn refused(path: &Path, source: io::Error) -> io::Error {
    io::Error::new(
        source.kind(),
        LoaderInputError {
            path: path.to_path_buf(),
            source,
        },
    )
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{Inputs, LoaderInputError};
    use crate::cargo::build_cache::toolchain::Identities;
    use crate::vars::Variables;

    #[test]
    fn a_directory_alias_is_a_bound_search_entry_and_retargeting_changes_it() {
        let directory = tempfile::tempdir().expect("owned loader parent");
        let search = directory.path().join("search");
        let target = directory.path().join("directory");
        std::fs::create_dir_all(&search).expect("owned search directory");
        std::fs::create_dir_all(&target).expect("owned directory target");
        let alias = search.join("candidate");
        std::os::unix::fs::symlink(&target, &alias).expect("directory alias");
        let mut env = Variables::default();
        env.set("DYLD_FALLBACK_LIBRARY_PATH", &search);
        let identities = Identities::empty();
        let before = Inputs::of(&env, &identities).expect("a directory alias is not a library");
        assert_eq!(
            before.digest(),
            Inputs::of(&env, &identities)
                .expect("unchanged alias")
                .digest()
        );
        let library = directory.path().join("library");
        std::fs::write(&library, b"actual library input").expect("owned library target");
        std::fs::remove_file(&alias).expect("retarget owned alias");
        std::os::unix::fs::symlink(&library, &alias).expect("library alias");
        assert_ne!(
            before.digest(),
            Inputs::of(&env, &identities)
                .expect("verified library alias")
                .digest()
        );
    }

    #[test]
    fn the_native_system_library_namespace_has_complete_typed_inputs() {
        let mut env = Variables::default();
        env.set("DYLD_FALLBACK_LIBRARY_PATH", "/usr/lib");
        Inputs::of(&env, &Identities::empty()).expect("actual native system library namespace");
    }

    #[test]
    fn a_fallback_library_change_is_bound_even_with_its_original_mtime() {
        let directory = tempfile::tempdir().expect("owned loader search");
        let path = directory.path().join("libcandidate.dylib");
        std::fs::write(&path, b"first").expect("original library");
        let modified = std::fs::metadata(&path)
            .expect("original metadata")
            .modified()
            .expect("original mtime");
        let mut env = Variables::default();
        env.set("DYLD_FALLBACK_LIBRARY_PATH", directory.path());
        let identities = Identities::empty();
        let before = Inputs::of(&env, &identities).expect("original inputs");
        assert!(before.admits("DYLD_FALLBACK_LIBRARY_PATH", directory.path().as_os_str()));
        assert!(!before.admits("DYLD_INSERT_LIBRARIES", directory.path().as_os_str()));
        assert_eq!(
            before.digest(),
            Inputs::of(&env, &identities)
                .expect("unchanged inputs")
                .digest()
        );
        std::fs::write(&path, b"other").expect("changed bytes of the same length");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("owned library")
            .set_modified(modified)
            .expect("restore mtime");
        assert_ne!(
            before.digest(),
            Inputs::of(&env, &identities)
                .expect("changed inputs")
                .digest()
        );
    }

    #[test]
    fn an_absent_fallback_directory_cannot_certify_a_later_library() {
        let directory = tempfile::tempdir().expect("owned loader parent");
        let search = directory.path().join("search");
        let mut env = Variables::default();
        env.set("DYLD_FALLBACK_LIBRARY_PATH", &search);
        let identities = Identities::empty();
        let absent = Inputs::of(&env, &identities).expect("observed absent directory");
        std::fs::create_dir_all(&search).expect("new search directory");
        std::fs::write(search.join("library"), b"actual bytes").expect("new library");
        assert_ne!(
            absent.digest(),
            Inputs::of(&env, &identities)
                .expect("present directory")
                .digest()
        );
    }

    #[test]
    fn relative_and_empty_search_entries_retain_typed_input_refusals() {
        for value in ["relative", ":/absolute"] {
            let mut env = Variables::default();
            env.set("DYLD_FALLBACK_LIBRARY_PATH", value);
            let error = Inputs::of(&env, &Identities::empty())
                .expect_err("an unbound search entry must be refused");
            assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
            let named = error
                .get_ref()
                .and_then(|source| source.downcast_ref::<LoaderInputError>())
                .expect("named loader input cause");
            assert!(!named.path.is_absolute());
            assert_eq!(named.source.kind(), std::io::ErrorKind::Unsupported);
        }
    }
}
