// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test scratch and compilation caches with distinct cleanup owners.

use std::io;
use std::path::Path;

use serde::Serialize;

/// The marker schema a test-owned temporary directory carries.
pub const OWNER_SCHEMA: &str = "njutest-test-temp-owner-v1";

/// The marker file inside a test-owned temporary directory, naming its owner.
pub const MARKER: &str = "owner.json";

/// The diagnostic a directory whose removal never settled carries, so a leftover is never silent.
pub const UNREMOVED: &str = "unremoved.json";

/// A directory with no asynchronous producer, removed once by its test owner.
#[derive(Debug)]
pub struct Temporary {
    directory: Option<tempfile::TempDir>,
    path: std::path::PathBuf,
}

/// The result of one completed removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Removal {
    /// The single removal attempt.
    pub attempts: u32,
}

/// A compilation-cache directory disposed by the parent after this test process exits.
#[derive(Debug)]
pub struct CacheDirectory {
    path: std::path::PathBuf,
}

/// The parent suite's cache-owner marker schema.
pub const CACHE_OWNER_SCHEMA: &str = "njutest-suite-cache-owner-v1";

/// The parent process that retains cleanup authority over all compilation-cache writers.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ParentOwner {
    schema: String,
    pid: u32,
}

impl CacheDirectory {
    /// Makes a cache under the suite's parent-owned root without a child cleanup destructor.
    ///
    /// # Errors
    /// No parent root is configured, its marker is invalid, or a directory cannot be created.
    pub fn make(prefix: &str) -> io::Result<Self> {
        let parent = std::env::var_os("NJUTEST_TEST_CACHE_ROOT").ok_or_else(|| {
            io::Error::other("a compilation cache requires the parent owner from cargo xtask tidy")
        })?;
        Self::make_in(Path::new(&parent), prefix)
    }

    /// Makes a cache after checking the parent cleanup owner's marker.
    fn make_in(parent: &Path, prefix: &str) -> io::Result<Self> {
        let bytes = std::fs::read(parent.join(MARKER))?;
        let owner: ParentOwner =
            crate::strictjson::decode_slice(&bytes).map_err(io::Error::other)?;
        if owner.schema != CACHE_OWNER_SCHEMA || owner.pid == 0 {
            return Err(io::Error::other(
                "the compilation-cache root has no suite cleanup owner",
            ));
        }
        let directory = tempfile::Builder::new().prefix(prefix).tempdir_in(parent)?;
        Ok(Self {
            path: directory.keep(),
        })
    }

    /// The cache path retained by the parent until the producing process exits.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The marker a test-owned temporary directory carries, in the shape `cargo xtask tidy` reads back.
#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    schema: String,
    binary: String,
    test: String,
    pid: u32,
}

impl Temporary {
    /// Makes a new owned temporary directory named by `prefix`, with its owner marker inside it.
    ///
    /// # Errors
    /// The directory cannot be made, or its owner marker cannot be written.
    pub fn make(prefix: &str) -> io::Result<Self> {
        let directory = tempfile::Builder::new().prefix(prefix).tempdir()?;
        Self::adopt(directory)
    }

    /// Takes an existing temporary directory over as its owner, writing the marker that names this test.
    ///
    /// # Errors
    /// The owner marker cannot be written.
    pub fn adopt(directory: tempfile::TempDir) -> io::Result<Self> {
        let owner = Owner::now();
        let marker = std::fs::File::create(directory.path().join(MARKER))?;
        serde_json::to_writer(marker, &owner).map_err(io::Error::other)?;
        let path = directory.path().to_path_buf();
        Ok(Self {
            directory: Some(directory),
            path,
        })
    }

    /// The directory, lent: publication does not carry the cleanup, which this owner keeps.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Closes the directory once after its synchronous users have ended.
    ///
    /// # Errors
    /// The filesystem refuses the removal.
    pub fn settle(mut self) -> io::Result<Removal> {
        let directory = self
            .directory
            .take()
            .ok_or_else(|| io::Error::other("the temporary owner was already closed"))?;
        directory.close()?;
        Ok(Removal { attempts: 1 })
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        let Some(directory) = self.directory.take() else {
            return;
        };
        let path = directory.path().to_path_buf();
        if let Err(failure) = directory.close() {
            eprintln!(
                "temporary: {} could not be removed: {failure}",
                path.display()
            );
        }
    }
}

impl Owner {
    /// This process's marker, naming the test nextest runs it for where it knows one.
    fn now() -> Self {
        Self {
            schema: OWNER_SCHEMA.to_owned(),
            binary: labelled("NEXTEST_BINARY_ID", "an unlabelled process"),
            test: labelled("NEXTEST_TEST_NAME", "an unlabelled test"),
            pid: std::process::id(),
        }
    }
}

/// `name` from the environment, or `unlabelled` where this process was not started by nextest.
fn labelled(name: &str, unlabelled: &str) -> String {
    match std::env::var(name) {
        Ok(value) => value,
        Err(_absent) => unlabelled.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{CacheDirectory, Temporary};
    use crate::thread::ScopedThread;

    #[test]
    fn a_parent_owned_cache_survives_a_delayed_writer_and_closes_after_its_event() {
        let parent = tempfile::tempdir().expect("the parent owner");
        std::fs::write(
            parent.path().join(super::MARKER),
            format!(
                "{{\"schema\":\"{}\",\"pid\":{}}}",
                super::CACHE_OWNER_SCHEMA,
                std::process::id()
            ),
        )
        .expect("the parent marker");
        let cache =
            CacheDirectory::make_in(parent.path(), "late-writer-").expect("a parent-owned cache");
        let path = cache.path().to_path_buf();
        std::thread::scope(|scope| {
            let (release, ready) = std::sync::mpsc::sync_channel(1);
            let writer_path = &path;
            let writer = ScopedThread::launch(scope, move || {
                ready.recv().expect("the consumer returned");
                std::fs::create_dir_all(writer_path.join("modules")).expect("the delayed mkdir");
                std::fs::write(writer_path.join("modules/late"), "late work")
                    .expect("the delayed write");
            });
            drop(cache);
            assert!(path.try_exists().expect("the retained cache"));
            release
                .send(())
                .expect("the writer can continue after consumer disposal");
            writer.join().expect("the owned producer completion event");
        });
        let written =
            std::fs::read_to_string(path.join("modules/late")).expect("the late write completed");
        parent
            .close()
            .expect("cleanup after the owned producer ended");
        assert_eq!(written, "late work");
        assert!(!path.try_exists().expect("the disposed cache"));
    }

    #[test]
    fn a_cache_without_a_parent_owner_is_refused() {
        let parent = tempfile::tempdir().expect("a raw temporary directory");
        CacheDirectory::make_in(parent.path(), "unowned-")
            .expect_err("a raw temporary root has no parent owner");
        std::fs::write(
            parent.path().join(super::MARKER),
            "{\"schema\":\"unowned\",\"pid\":1}",
        )
        .expect("a mismatched marker");
        CacheDirectory::make_in(parent.path(), "unowned-")
            .expect_err("a mismatched marker supplies no cleanup owner");
    }

    #[test]
    fn a_synchronous_directory_closes_once_and_names_its_owner() {
        let owned = Temporary::make("njutest-synchronous-").expect("an owner");
        let path = owned.path().to_path_buf();
        let marker = std::fs::read_to_string(path.join(super::MARKER)).expect("the marker");
        let settled = owned.settle().expect("the one removal");
        assert_eq!(settled.attempts, 1);
        assert!(marker.contains(super::OWNER_SCHEMA));
        assert!(!path.try_exists().expect("the removal is observable"));
    }
}
