// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where a completed verification is kept.

#[cfg(unix)]
use std::collections::{BTreeMap, BTreeSet};
use std::io;
#[cfg(unix)]
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use rust_mutants::id::{RunId, RunIdError, StoredRunId};
#[cfg(unix)]
use serde::Deserialize;

use crate::error::{self, ErrorCode};
use crate::report::{Report, ReportDocument, json};
#[cfg(unix)]
use rust_mutants::capdir::{Dir, Kind, Name, Privacy, Status};

/// One path component the store names, refused as input when it is not one.
#[cfg(unix)]
fn name(text: &str) -> io::Result<Name<'_>> {
    Name::new(text).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))
}

/// Whether a failure says the entry is not there.
#[cfg(unix)]
fn absent(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
}

/// Where the runs of one report directory live, and the only thing that knows the layout.
///
/// Every place that wanted a path used to join a constant, tests included, so the layout was a fact spread across the tree and the configuration could not name it without moving all of them.
/// Production and a test now ask the same value the same way, and a project that already means something by `reports/` can say so.
#[derive(Debug)]
pub struct Store {
    runs: PathBuf,
    root: PathBuf,
    configured: crate::config::ReportDirectory,
    #[cfg(unix)]
    workspace: Dir,
    #[cfg(unix)]
    report_root: std::sync::Arc<std::sync::OnceLock<Dir>>,
}

/// One workspace directory held before its configuration is read.
///
/// A report store can only be derived from this value, so parsing `.njutest.toml` and later opening the configured store share one directory identity instead of reopening a mutable pathname.
#[derive(Debug)]
pub(crate) struct WorkspaceRoot {
    path: PathBuf,
    #[cfg(unix)]
    directory: Dir,
}

/// Which default configuration source supplied one held workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigurationSource {
    /// No `.njutest.toml` existed, so typed defaults were used.
    Defaults,
    /// A regular `.njutest.toml` supplied the parsed bytes.
    WorkspaceFile,
}

/// Configuration bytes and provenance read from one held workspace object.
#[derive(Debug)]
pub(crate) struct LoadedConfiguration {
    pub(crate) config: crate::config::Config,
    pub(crate) source: ConfigurationSource,
}

#[cfg(unix)]
#[derive(Debug)]
struct StoreRoot {
    directory: Dir,
}

#[cfg(unix)]
#[derive(Debug)]
enum StoreRootState {
    Missing,
    Open(StoreRoot),
}

/// The one opened `runs` namespace a whole reader operation resolves against.
///
/// The only constructor opens it beneath a held [`StoreRoot`], and every census and run opening goes through the same value, so a run is never validated against one `runs` inode and read from another.
#[cfg(unix)]
#[derive(Debug)]
struct RunsRoot {
    directory: Dir,
}

#[cfg(unix)]
impl RunsRoot {
    fn open_at(root: &StoreRoot) -> Result<Self, StoreError> {
        open_directory_at(&root.directory, RUNS_NAME)
            .map(|directory| Self { directory })
            .map_err(|source| StoreError::NotKept {
                path: RUNS_NAME.to_owned(),
                source,
            })
    }
}

/// One stored run held open as the exact directory selected by an index or explicit canonical identity.
#[derive(Debug)]
pub(crate) struct StoredRun {
    id: StoredRunId,
    display: PathBuf,
    said_document: String,
    #[cfg(unix)]
    directory: Dir,
}

/// A closed file name that a stored report is allowed to expose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredFile {
    Document,
    Lines,
}

impl StoredFile {
    #[cfg(unix)]
    const fn name(self) -> &'static str {
        match self {
            Self::Document => DOCUMENT_NAME,
            Self::Lines => crate::report::lines::FILE_NAME,
        }
    }
}

/// Exclusive ownership of one unpublished run directory.
///
/// The private fields make it impossible for a persistence caller to claim that an arbitrary pre-existing directory was reserved for this report.
/// The canonical run path does not exist until this value is consumed by the checked publication transition.
#[derive(Debug)]
pub(crate) struct RunDirectory {
    store: Store,
    run_id: RunId,
    staging: PathBuf,
    published: PathBuf,
    capability: RunCapability,
    ownership: RunOwnership,
}

/// A report tree whose producer-visible pathname has been retired from the write protocol.
/// Only this state can cross the publication transition.
///
/// The retained artifact ledger is re-read through the held directory handle after the no-replace rename, before either parent is synced or ownership is disarmed.
#[derive(Debug)]
struct SealedRunDirectory {
    directory: RunDirectory,
    artifacts: Vec<crate::report::ModelArtifact>,
    files: Vec<PublicationFile>,
}

#[derive(Debug)]
struct PublicationFile {
    name: &'static str,
    bytes: Vec<u8>,
}

/// Open directory capabilities retained from exclusive creation through publication.
/// On Unix every model read, report write, tree sync, rename,
/// and parent sync is relative to these descriptors rather than a spelling that an ancestor replacement could redirect.
#[cfg(unix)]
#[derive(Debug)]
struct RunCapability {
    root: Dir,
    directory: Dir,
    staging_parent: Dir,
    published_parent: Dir,
}

/// Hosts without the Unix handle-relative backend refuse publication.
/// A path fallback cannot bind check, read, rename, and cleanup to the same object.
#[cfg(not(unix))]
#[derive(Debug)]
struct RunCapability;

/// Which unpublished filesystem object a [`RunDirectory`] still owns.
///
/// Keeping this as a closed state instead of a boolean matters after the no-replace rename: cleanup must remove the canonical directory rather than the now-absent staging spelling until its parent has been synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunOwnership {
    Staging,
    Published,
    /// The canonical directory is durable and may already be named by a derived index.
    /// It is no longer legal to remove it while unwinding a later validation or index failure.
    Committed,
    Disarmed,
}

impl RunDirectory {
    /// The fresh, unpublished directory in which report artifacts are built.
    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        &self.staging
    }

    fn write_new(&self, name: &str, bytes: &[u8]) -> Result<(), PublicationFailure> {
        self.capability
            .write_new(&self.staging, name, bytes)
            .map_err(|source| PublicationFailure::NotKept {
                path: self.staging.join(name).display().to_string(),
                source,
            })
    }

    fn sync_tree(&self) -> io::Result<()> {
        self.capability.sync_tree(&self.staging)
    }

    fn read_model_artifact(&self, artifact: &crate::report::ModelArtifact) -> io::Result<Vec<u8>> {
        self.capability.read_model_artifact(
            &self.staging,
            Path::new(artifact.path()),
            artifact.bytes(),
        )
    }

    fn read_publication_file(&self, file: &PublicationFile) -> io::Result<Vec<u8>> {
        self.capability.read_publication_file(
            &self.staging,
            file.name,
            u64::try_from(file.bytes.len()).map_err(|_outside_wire| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "publication file length cannot fit its retained boundary",
                )
            })?,
        )
    }

    fn validate_closed_tree(
        &self,
        files: &[PublicationFile],
        artifacts: &[crate::report::ModelArtifact],
    ) -> io::Result<()> {
        self.capability.validate_closed_tree(files, artifacts)
    }

    fn publish_entry(&self) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.capability.publish(self.run_id.as_str())
        }
        #[cfg(not(unix))]
        {
            let staging_parent = self.staging.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "staging path has no parent")
            })?;
            let published_parent = self.published.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "published path has no parent")
            })?;
            self.capability
                .publish(self.run_id.as_str(), staging_parent, published_parent)
        }
    }

    fn published_entry_matches(&self) -> io::Result<bool> {
        self.capability
            .published_entry_matches(self.run_id.as_str())
    }

    fn validate_run_spelling(&self) -> io::Result<()> {
        self.capability.validate_run_spelling(self.run_id.as_str())
    }

    fn staging_entry_matches(&self) -> io::Result<bool> {
        #[cfg(unix)]
        {
            self.capability.staging_entry_matches(self.run_id.as_str())
        }
        #[cfg(not(unix))]
        {
            let staging_parent = self.staging.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "staging path has no parent")
            })?;
            self.capability
                .staging_entry_matches(self.run_id.as_str(), staging_parent)
        }
    }

    fn sync_rename_parent(&self, published: bool) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.capability.sync_parent(published)
        }
        #[cfg(not(unix))]
        {
            let staging_parent = self.staging.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "staging path has no parent")
            })?;
            let published_parent = self.published.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "published path has no parent")
            })?;
            self.capability
                .sync_parent(published, staging_parent, published_parent)
        }
    }

    /// Explicitly abandons an unpublished run.
    ///
    /// # Errors
    /// Returns the cleanup failure rather than pretending a partial staging tree was removed.
    pub(crate) fn abort(mut self) -> Result<(), StoreError> {
        let result = match self.ownership {
            RunOwnership::Staging => self.remove_owned(false),
            RunOwnership::Published => self.remove_owned(true),
            RunOwnership::Committed | RunOwnership::Disarmed => {
                return Err(StoreError::UnsafePath {
                    path: self.staging.clone(),
                    message: "a committed or disarmed run-directory claim cannot be aborted"
                        .to_owned(),
                });
            }
        };
        self.finish_cleanup(result)
    }

    fn finish_cleanup(&mut self, result: Result<(), StoreError>) -> Result<(), StoreError> {
        match result {
            Ok(()) => {
                self.ownership = RunOwnership::Disarmed;
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    #[cfg_attr(
        not(unix),
        expect(
            clippy::missing_const_for_fn,
            clippy::unused_self,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    fn remove_owned(&self, published: bool) -> Result<(), StoreError> {
        #[cfg(unix)]
        {
            self.capability
                .remove_entry(published, self.run_id.as_str())
                .map_err(|source| StoreError::NotKept {
                    path: if published {
                        self.published.display().to_string()
                    } else {
                        self.staging.display().to_string()
                    },
                    source,
                })
        }
        #[cfg(not(unix))]
        {
            #[cfg_attr(
                not(unix),
                expect(
                    clippy::no_effect_underscore_binding,
                    reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
                )
            )]
            let _published = published;
            Err(StoreError::UnsupportedCapability)
        }
    }

    fn owned_path(&self) -> Option<&Path> {
        match self.ownership {
            RunOwnership::Staging => Some(&self.staging),
            RunOwnership::Published => Some(&self.published),
            RunOwnership::Committed | RunOwnership::Disarmed => None,
        }
    }
}

impl Drop for RunDirectory {
    fn drop(&mut self) {
        let published = match self.ownership {
            RunOwnership::Staging => false,
            RunOwnership::Published => true,
            RunOwnership::Committed | RunOwnership::Disarmed => return,
        };
        if self.owned_path().is_none() {
            return;
        }
        self.ownership = RunOwnership::Disarmed;
        if self.remove_owned(published).is_err() {
            std::process::abort();
        }
    }
}

#[cfg(unix)]
impl RunCapability {
    const fn supports_model_artifacts() -> bool {
        true
    }

    fn claim_beneath(root: &Dir, run: &str) -> io::Result<Self> {
        Self::claim_beneath_with_root(root, root.try_clone(), run)
    }

    fn claim_beneath_with_root(
        root: &Dir,
        retained_root: io::Result<Dir>,
        run: &str,
    ) -> io::Result<Self> {
        let retained_root = retained_root?;
        if root.status()?.identity != retained_root.status()?.identity {
            return Err(io::Error::other(
                "the retained report-root capability does not match its source",
            ));
        }
        let staging_parent = ensure_directory_at(root, STAGING_NAME)?;
        require_private_directory(&staging_parent)?;
        ensure_private_marker(&staging_parent)?;
        let published_parent = ensure_directory_at(root, RUNS_NAME)?;
        require_private_directory(&published_parent)?;
        ensure_private_marker(&published_parent)?;
        ensure_writable_spelling_at(&published_parent, run)?;
        if published_parent.status_at(name(run)?)?.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "the final run namespace already exists",
            ));
        }
        let directory = create_private_claim_directory(&staging_parent, run)?;
        let capability = Self {
            root: retained_root,
            directory,
            staging_parent,
            published_parent,
        };
        if let Err(primary) = capability.directory.sync() {
            return Err(claim_cleanup_error(
                primary,
                capability.remove_entry(false, run),
            ));
        }
        if let Err(primary) = capability.staging_parent.sync() {
            return Err(claim_cleanup_error(
                primary,
                capability.remove_entry(false, run),
            ));
        }
        Ok(capability)
    }

    fn write_new(&self, _staging: &Path, file_name: &str, bytes: &[u8]) -> io::Result<()> {
        if !matches!(
            Path::new(file_name)
                .components()
                .collect::<Vec<_>>()
                .as_slice(),
            [std::path::Component::Normal(_)]
        ) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "publication file name is not one ordinary path component",
            ));
        }
        let mut file = self.directory.create_file(name(file_name)?)?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    fn sync_tree(&self, _staging: &Path) -> io::Result<()> {
        sync_open_tree(&self.directory)
    }

    fn read_model_artifact(
        &self,
        _staging: &Path,
        relative: &Path,
        expected: u64,
    ) -> io::Result<Vec<u8>> {
        let mut directory = self.directory.try_clone()?;
        let mut components = relative.components().peekable();
        while let Some(component) = components.next() {
            let entry = match component {
                std::path::Component::Normal(entry) => entry.to_str(),
                _ => None,
            }
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "model artifact path is not canonical beneath the run capability",
                )
            })?;
            if components.peek().is_some() {
                directory = open_directory_at(&directory, entry)?;
                continue;
            }
            return read_regular_artifact(directory.open_file(name(entry)?)?, expected);
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "model artifact path is empty",
        ))
    }

    fn read_publication_file(
        &self,
        _staging: &Path,
        file_name: &str,
        expected: u64,
    ) -> io::Result<Vec<u8>> {
        read_regular_artifact(self.directory.open_file(name(file_name)?)?, expected)
    }

    fn validate_closed_tree(
        &self,
        files: &[PublicationFile],
        artifacts: &[crate::report::ModelArtifact],
    ) -> io::Result<()> {
        let mut expected_files = files
            .iter()
            .map(|file| file.name.to_owned())
            .collect::<BTreeSet<_>>();
        let mut expected_model = BTreeSet::new();
        for artifact in artifacts {
            let name = artifact.path().strip_prefix("model/").ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "retained model artifact is outside the model namespace",
                )
            })?;
            if !expected_model.insert(name.to_owned()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "retained model artifact path is duplicated",
                ));
            }
        }

        let root = self.directory.try_clone()?;
        let mut saw_model = false;
        for name in directory_names(&root)? {
            if name == "model" && !expected_model.is_empty() {
                let model = open_directory_at(&root, "model")?;
                validate_model_names(&model, &mut expected_model)?;
                saw_model = true;
            } else if expected_files.remove(&name) {
                require_regular_at(&root, &name)?;
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unretained entry {name:?} is present in the sealed report tree"),
                ));
            }
        }
        let expects_model = !artifacts.is_empty();
        if !expected_files.is_empty() || !expected_model.is_empty() || saw_model != expects_model {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the sealed report tree does not exactly match its retained file ledger",
            ));
        }
        Ok(())
    }

    fn publish(&self, run: &str) -> io::Result<()> {
        let run = name(run)?;
        self.staging_parent
            .rename_noreplace(run, &self.published_parent, run)
    }

    fn validate_run_spelling(&self, name: &str) -> io::Result<()> {
        self.require_runs_binding()?;
        ensure_writable_spelling_at(&self.published_parent, name)
    }

    /// Proves the name `runs` under the held report root still names the publication parent this capability retains.
    ///
    /// # Errors
    /// Returns the typed refusal for a replaced or non-directory namespace.
    fn require_runs_binding(&self) -> io::Result<()> {
        let named = self.root.status_at(name(RUNS_NAME)?)?;
        let held = self.published_parent.status()?;
        match named {
            Some(named) if named.identity == held.identity && named.kind == Kind::Directory => {
                Ok(())
            }
            Some(_) | None => Err(io::Error::other(
                "the runs namespace no longer names the held publication parent",
            )),
        }
    }

    fn published_entry_matches(&self, name: &str) -> io::Result<bool> {
        self.entry_matches(&self.published_parent, name)
    }

    fn staging_entry_matches(&self, name: &str) -> io::Result<bool> {
        self.entry_matches(&self.staging_parent, name)
    }

    fn entry_matches(&self, parent: &Dir, run: &str) -> io::Result<bool> {
        let named = open_directory_at(parent, run)?;
        Ok(self.directory.status()?.identity == named.status()?.identity)
    }

    fn sync_parent(&self, published: bool) -> io::Result<()> {
        if published {
            self.published_parent.sync()
        } else {
            self.staging_parent.sync()
        }
    }

    fn remove_entry(&self, published: bool, run: &str) -> io::Result<()> {
        let parent = if published {
            &self.published_parent
        } else {
            &self.staging_parent
        };
        let quarantine = quarantine_named_entry(parent, run, ".njutest-cleanup")?;
        let quarantined = open_directory_at(parent, &quarantine)?;
        if self.directory.status()?.identity != quarantined.status()?.identity {
            parent.rename_noreplace(name(&quarantine)?, parent, name(run)?)?;
            parent.sync()?;
            return Err(io::Error::other(
                "the owned run-directory name no longer refers to the held directory",
            ));
        }
        remove_open_tree(&quarantined)?;
        if !named_directory_matches(parent, &quarantine, &quarantined)? {
            return Err(io::Error::other(
                "the quarantined run directory changed identity during cleanup",
            ));
        }
        parent.remove_dir(name(&quarantine)?)?;
        parent.sync()
    }
}

#[cfg(unix)]
fn open_configured_root(workspace: &Dir, configured: &Path) -> io::Result<Dir> {
    let mut current = workspace.try_clone()?;
    for component in configured_components(configured)? {
        current = ensure_directory_at(&current, component)?;
    }
    Ok(current)
}

/// The components of a configured workspace-relative report directory, refused unless each is one plain UTF-8 name.
#[cfg(unix)]
fn configured_components(configured: &Path) -> io::Result<Vec<&str>> {
    let components = configured
        .components()
        .map(|component| {
            let std::path::Component::Normal(entry) = component else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "the configured report directory is not canonical workspace-relative",
                ));
            };
            entry.to_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "the configured report directory is not UTF-8",
                )
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    if components.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the configured report directory is empty",
        ));
    }
    Ok(components)
}

#[cfg(unix)]
enum ExistingStoreRoot {
    Missing,
    Present(Dir),
}

#[cfg(unix)]
fn open_existing_configured_root(
    workspace: &Dir,
    configured: &Path,
) -> io::Result<ExistingStoreRoot> {
    let mut current = workspace.try_clone()?;
    for component in configured_components(configured)? {
        current = match open_directory_at(&current, component) {
            Ok(directory) => directory,
            Err(error) if absent(&error) => return Ok(ExistingStoreRoot::Missing),
            Err(error) => return Err(error),
        };
    }
    Ok(ExistingStoreRoot::Present(current))
}

#[cfg(unix)]
fn ensure_directory_at(parent: &Dir, entry: &str) -> io::Result<Dir> {
    let entry_name = name(entry)?;
    match parent.open_dir(entry_name) {
        Ok(directory) => return Ok(directory),
        Err(error) if !absent(&error) => return Err(error),
        Err(_missing) => {}
    }
    let directory = match parent.create_private_dir_exclusive(entry_name) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return parent.open_dir(entry_name);
        }
        Err(error) => return Err(error),
    };
    let created = directory.status()?;
    let cleanup_created = |primary: io::Error| {
        claim_cleanup_error(primary, remove_empty_if_identity(parent, entry, &created))
    };
    directory.sync().map_err(&cleanup_created)?;
    parent.sync().map_err(&cleanup_created)?;
    Ok(directory)
}

#[cfg(unix)]
fn require_private_directory(directory: &Dir) -> io::Result<()> {
    if directory.status()?.kind != Kind::Directory {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a private report namespace is not a directory",
        ));
    }
    match directory.privacy()? {
        Privacy::OwnerOnly => Ok(()),
        Privacy::ForeignOwner => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "a private report namespace is owned by another user",
        )),
        Privacy::Loose => {
            directory.restrict_to_owner()?;
            directory.sync()?;
            if directory.privacy()? == Privacy::OwnerOnly {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "a same-owner report namespace could not be tightened to owner-only access",
                ))
            }
        }
    }
}

#[cfg(unix)]
fn create_private_claim_directory(parent: &Dir, final_name: &str) -> io::Result<Dir> {
    const ATTEMPTS: usize = 8;
    let final_entry = name(final_name)?;
    for _attempt in 0..ATTEMPTS {
        let mut token = [0_u8; 16];
        getrandom::fill(&mut token).map_err(|error| {
            io::Error::other(format!(
                "the private claim token could not be minted: {error}"
            ))
        })?;
        let temporary = format!(".njutest-claim-{}", hex::encode(token));
        let temporary_entry = name(&temporary)?;
        let directory = match parent.create_private_dir_exclusive(temporary_entry) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let created = directory.status()?;
        let cleanup_temporary = |primary: io::Error| {
            claim_cleanup_error(
                primary,
                remove_empty_if_identity(parent, &temporary, &created),
            )
        };
        if let Err(error) = require_private_directory(&directory) {
            return Err(cleanup_temporary(error));
        }
        if let Err(error) = parent.rename_noreplace(temporary_entry, parent, final_entry) {
            return Err(cleanup_temporary(error));
        }
        let cleanup_final = |primary: io::Error| {
            claim_cleanup_error(
                primary,
                remove_empty_if_identity(parent, final_name, &created),
            )
        };
        match parent.status_at(final_entry).map_err(&cleanup_final)? {
            Some(named) if named.identity == created.identity => return Ok(directory),
            Some(_) | None => {
                return Err(cleanup_final(io::Error::other(
                    "the atomic private claim name does not refer to the created directory",
                )));
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("could not reserve a private claim name after {ATTEMPTS} attempts"),
    ))
}

#[cfg(unix)]
fn ensure_private_marker(directory: &Dir) -> io::Result<()> {
    const MARKER: &str = ".gitignore";
    const CONTENTS: &[u8] = b"*\n";
    let marker_name = name(MARKER)?;
    match directory.open_file(marker_name) {
        Ok(file) => {
            let bytes = read_regular_artifact(
                file,
                u64::try_from(CONTENTS.len()).map_err(|_outside_wire| {
                    io::Error::new(io::ErrorKind::InvalidData, "private marker length overflow")
                })?,
            )?;
            if bytes == CONTENTS {
                return Ok(());
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the private report-directory marker has unexpected bytes",
            ));
        }
        Err(error) if !absent(&error) => return Err(error),
        Err(_missing) => {}
    }
    let mut marker = directory.create_file(marker_name)?;
    marker.write_all(CONTENTS)?;
    marker.sync_all()?;
    directory.sync()
}

#[cfg(unix)]
fn ensure_writable_spelling_at(directory: &Dir, run: &str) -> io::Result<()> {
    let wanted = StoredRunId::try_from(run).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("the writable run name is not canonical: {error}"),
        )
    })?;
    let folded = wanted.case_folded();
    for existing in directory_names(directory)? {
        if existing == ".gitignore" {
            require_regular_at(directory, &existing)?;
            continue;
        }
        let stored = StoredRunId::try_from(existing.as_str()).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown report-store entry {existing:?}: {error}"),
            )
        })?;
        let opened = open_directory_at(directory, &existing)?;
        if opened.status()?.kind != Kind::Directory {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a stored run is not one real directory",
            ));
        }
        if stored.case_folded() == folded && stored != wanted {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("stored run {stored:?} aliases new run {wanted:?} by ASCII case"),
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn claim_cleanup_error(primary: io::Error, cleanup: io::Result<()>) -> io::Error {
    match cleanup {
        Ok(()) => primary,
        Err(cleanup) => io::Error::other(format!(
            "{primary}; cleanup of the failed capability claim also failed: {cleanup}"
        )),
    }
}

#[cfg(unix)]
fn remove_empty_if_identity(parent: &Dir, entry: &str, expected: &Status) -> io::Result<()> {
    let entry_name = name(entry)?;
    let gone = || {
        io::Error::other("the failed claim name no longer refers to the directory that was created")
    };
    match parent.status_at(entry_name)? {
        Some(actual) if actual.identity == expected.identity => {}
        Some(_) | None => return Err(gone()),
    }
    let quarantine = quarantine_named_entry(parent, entry, ".njutest-empty-cleanup")?;
    let quarantine_name = name(&quarantine)?;
    match parent.status_at(quarantine_name)? {
        Some(quarantined) if quarantined.identity == expected.identity => {}
        Some(_) | None => {
            let restore = parent.rename_noreplace(quarantine_name, parent, entry_name);
            return Err(claim_cleanup_error(
                io::Error::other("the failed claim entry changed before it was quarantined"),
                restore,
            ));
        }
    }
    parent.remove_dir(quarantine_name)?;
    parent.sync()
}

#[cfg(unix)]
fn quarantine_named_entry(directory: &Dir, original: &str, stem: &str) -> io::Result<String> {
    quarantine_entry_with(directory, original, stem, || {
        let mut token = [0_u8; 16];
        getrandom::fill(&mut token).map_err(|error| {
            io::Error::other(format!(
                "the cleanup quarantine token could not be minted: {error}"
            ))
        })?;
        Ok(token)
    })
}

#[cfg(unix)]
fn quarantine_entry_with<F>(
    directory: &Dir,
    original: &str,
    stem: &str,
    mut token: F,
) -> io::Result<String>
where
    F: FnMut() -> io::Result<[u8; 16]>,
{
    const ATTEMPTS: usize = 8;
    let original_name = name(original)?;
    for _attempt in 0..ATTEMPTS {
        let candidate = format!("{stem}-{}", hex::encode(token()?));
        if original == candidate {
            continue;
        }
        match directory.rename_noreplace(original_name, directory, name(&candidate)?) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("could not reserve a cleanup quarantine after {ATTEMPTS} attempts"),
    ))
}

#[cfg(unix)]
fn named_directory_matches(parent: &Dir, entry: &str, expected: &Dir) -> io::Result<bool> {
    let actual = open_directory_at(parent, entry)?;
    Ok(expected.status()?.identity == actual.status()?.identity)
}

#[cfg(unix)]
fn require_same_directory(expected: &Dir, actual: &Dir) -> io::Result<()> {
    let expected = expected.status()?;
    let actual = actual.status()?;
    if expected.kind != Kind::Directory
        || actual.kind != Kind::Directory
        || expected.identity != actual.identity
    {
        return Err(io::Error::other(
            "the configured report root changed identity while it was bound",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn read_regular_artifact(file: std::fs::File, expected: u64) -> io::Result<Vec<u8>> {
    let status = rust_mutants::capdir::file_status(&file)?;
    if status.kind != Kind::File || status.len != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "model artifact is not one regular file of the retained size",
        ));
    }
    let limit = expected.checked_add(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "model artifact size is unbounded",
        )
    })?;
    let mut file = file;
    let mut bytes = Vec::new();
    io::Read::by_ref(&mut file)
        .take(limit)
        .read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    let read_size = u64::try_from(bytes.len()).map_err(|_outside_wire| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "model artifact read length cannot fit the retained wire",
        )
    })?;
    if read_size != expected || after.len() != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "model artifact changed while its retained bytes were read",
        ));
    }
    Ok(bytes)
}

#[cfg(unix)]
fn open_directory(path: &Path) -> io::Result<Dir> {
    Dir::open(path)
}

#[cfg(unix)]
fn read_optional_config_at(workspace: &Dir, path: &Path) -> Result<Option<String>, StoreError> {
    let opened = name(crate::config::FILE_NAME).and_then(|file| workspace.open_file(file));
    let file = match opened {
        Ok(file) => file,
        Err(error) if absent(&error) => return Ok(None),
        Err(source) => {
            return Err(StoreError::NotKept {
                path: path.display().to_string(),
                source,
            });
        }
    };
    read_configuration_descriptor(file, path).map(Some)
}

#[cfg(unix)]
fn read_configuration_descriptor(file: std::fs::File, path: &Path) -> Result<String, StoreError> {
    const MAX_CONFIG_BYTES: u64 = 1_048_576;
    let status =
        rust_mutants::capdir::file_status(&file).map_err(|source| StoreError::NotKept {
            path: path.display().to_string(),
            source,
        })?;
    if status.kind != Kind::File {
        return Err(StoreError::UnsafePath {
            path: path.to_path_buf(),
            message: "the configuration is not one regular file".to_owned(),
        });
    }
    if status.len > MAX_CONFIG_BYTES {
        return Err(StoreError::UnsafePath {
            path: path.to_path_buf(),
            message: format!("the configuration exceeds {MAX_CONFIG_BYTES} bytes"),
        });
    }
    let bytes = read_regular_artifact(file, status.len).map_err(|source| StoreError::NotKept {
        path: path.display().to_string(),
        source,
    })?;
    String::from_utf8(bytes).map_err(|source| StoreError::UnsafePath {
        path: path.to_path_buf(),
        message: format!("the configuration is not UTF-8: {source}"),
    })
}

/// Reads one explicitly named configuration without following its final component and from the same descriptor whose kind and length are checked.
///
/// # Errors
/// Refuses unsafe, oversized, changing, non-UTF-8, or unreadable bytes.
pub(crate) fn read_configuration(path: &Path) -> Result<String, StoreError> {
    #[cfg(unix)]
    {
        use rustix::fs::{Mode, OFlags};

        let descriptor = rustix::fs::open(
            path,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|error| StoreError::NotKept {
            path: path.display().to_string(),
            source: io::Error::from(error),
        })?;
        read_configuration_descriptor(std::fs::File::from(descriptor), path)
    }
    #[cfg(not(unix))]
    {
        std::fs::read_to_string(path).map_err(|source| StoreError::NotKept {
            path: path.display().to_string(),
            source,
        })
    }
}

#[cfg(unix)]
fn read_optional_regular_at(
    directory: &Dir,
    file_name: &str,
    path: &Path,
) -> Result<Option<Vec<u8>>, StoreError> {
    const MAX_STORED_FILE_BYTES: u64 = 67_108_864;
    let not_kept = |source: io::Error| StoreError::NotKept {
        path: path.display().to_string(),
        source,
    };
    let file = match name(file_name).and_then(|entry| directory.open_file(entry)) {
        Ok(file) => file,
        Err(error) if absent(&error) => return Ok(None),
        Err(source) => return Err(not_kept(source)),
    };
    let status = rust_mutants::capdir::file_status(&file).map_err(not_kept)?;
    if status.kind != Kind::File {
        return Err(StoreError::UnsafePath {
            path: path.to_path_buf(),
            message: "the stored report entry is not one regular file".to_owned(),
        });
    }
    if status.len > MAX_STORED_FILE_BYTES {
        return Err(StoreError::UnsafePath {
            path: path.to_path_buf(),
            message: format!("the stored report file exceeds {MAX_STORED_FILE_BYTES} bytes"),
        });
    }
    read_regular_artifact(file, status.len)
        .map(Some)
        .map_err(not_kept)
}

#[cfg(unix)]
fn open_directory_at(directory: &Dir, entry: &str) -> io::Result<Dir> {
    directory.open_dir(name(entry)?)
}

#[cfg(unix)]
fn directory_names(directory: &Dir) -> io::Result<Vec<String>> {
    directory.entries()
}

#[cfg(unix)]
fn require_regular_at(directory: &Dir, entry: &str) -> io::Result<()> {
    match directory.status_at(name(entry)?)? {
        Some(status) if status.kind == Kind::File => Ok(()),
        Some(_other) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("sealed report entry {entry:?} is not one regular file"),
        )),
        None => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("sealed report entry {entry:?} is missing"),
        )),
    }
}

#[cfg(unix)]
fn validate_model_names(directory: &Dir, expected: &mut BTreeSet<String>) -> io::Result<()> {
    for name in directory_names(directory)? {
        if !expected.remove(&name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unretained model artifact {name:?} is present in the sealed report tree"),
            ));
        }
        require_regular_at(directory, &name)?;
    }
    Ok(())
}

#[cfg(unix)]
fn remove_open_tree(directory: &Dir) -> io::Result<()> {
    directory.remove_contents()
}

#[cfg(not(unix))]
impl RunCapability {
    const fn supports_model_artifacts() -> bool {
        false
    }

    #[cfg_attr(
        not(unix),
        expect(
            clippy::missing_const_for_fn,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    fn claim(_store: &Store, _name: &str) -> Result<Self, StoreError> {
        Err(StoreError::UnsupportedCapability)
    }

    #[cfg_attr(
        not(unix),
        expect(
            clippy::unused_self,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    fn write_new(&self, _staging: &Path, _name: &str, _bytes: &[u8]) -> io::Result<()> {
        Err(unsupported_capability())
    }

    #[cfg_attr(
        not(unix),
        expect(
            clippy::unused_self,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    fn sync_tree(&self, _staging: &Path) -> io::Result<()> {
        Err(unsupported_capability())
    }

    #[cfg_attr(
        not(unix),
        expect(
            clippy::unused_self,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    fn read_model_artifact(
        &self,
        _staging: &Path,
        _relative: &Path,
        _expected: u64,
    ) -> io::Result<Vec<u8>> {
        Err(unsupported_capability())
    }

    #[cfg_attr(
        not(unix),
        expect(
            clippy::unused_self,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    fn read_publication_file(
        &self,
        _staging: &Path,
        _name: &str,
        _expected: u64,
    ) -> io::Result<Vec<u8>> {
        Err(unsupported_capability())
    }

    #[cfg_attr(
        not(unix),
        expect(
            clippy::unused_self,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    fn validate_closed_tree(
        &self,
        _files: &[PublicationFile],
        _artifacts: &[crate::report::ModelArtifact],
    ) -> io::Result<()> {
        Err(unsupported_capability())
    }

    #[cfg_attr(
        not(unix),
        expect(
            clippy::unused_self,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    fn publish(
        &self,
        _name: &str,
        _staging_parent: &Path,
        _published_parent: &Path,
    ) -> io::Result<()> {
        Err(unsupported_capability())
    }

    #[cfg_attr(
        not(unix),
        expect(
            clippy::unused_self,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    fn published_entry_matches(&self, _name: &str) -> io::Result<bool> {
        Err(unsupported_capability())
    }

    #[cfg_attr(
        not(unix),
        expect(
            clippy::unused_self,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    fn validate_run_spelling(&self, _name: &str) -> io::Result<()> {
        Err(unsupported_capability())
    }

    #[cfg_attr(
        not(unix),
        expect(
            clippy::unused_self,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    fn staging_entry_matches(&self, _name: &str, _staging_parent: &Path) -> io::Result<bool> {
        Err(unsupported_capability())
    }

    #[cfg_attr(
        not(unix),
        expect(
            clippy::unused_self,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    fn sync_parent(
        &self,
        _published: bool,
        _staging_parent: &Path,
        _published_parent: &Path,
    ) -> io::Result<()> {
        Err(unsupported_capability())
    }
}

#[cfg(not(unix))]
fn unsupported_capability() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "durable report publication requires a supported capability backend",
    )
}

impl WorkspaceRoot {
    /// Holds the workspace directory before any report configuration is read.
    ///
    /// # Errors
    /// Refuses a missing, non-directory, symlinked, or unreadable workspace.
    #[cfg_attr(
        not(unix),
        expect(
            clippy::unnecessary_wraps,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    pub(crate) fn open(path: &Path) -> Result<Self, StoreError> {
        #[cfg(unix)]
        let directory = open_directory(path).map_err(|source| StoreError::NotKept {
            path: path.display().to_string(),
            source,
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            #[cfg(unix)]
            directory,
        })
    }

    /// Reads the default configuration through this exact workspace object.
    ///
    /// # Errors
    /// Refuses non-regular, symlinked, oversized, non-UTF-8, malformed, or unreadable configuration bytes.
    pub(crate) fn load_config(&self) -> Result<LoadedConfiguration, StoreError> {
        #[cfg(unix)]
        {
            let path = self.path.join(crate::config::FILE_NAME);
            let Some(text) = read_optional_config_at(&self.directory, &path)? else {
                return Ok(LoadedConfiguration {
                    config: crate::config::Config::default(),
                    source: ConfigurationSource::Defaults,
                });
            };
            crate::config::Config::parse(&text, &path)
                .map(|config| LoadedConfiguration {
                    config,
                    source: ConfigurationSource::WorkspaceFile,
                })
                .map_err(StoreError::from)
        }
        #[cfg(not(unix))]
        {
            let path = self.path.join(crate::config::FILE_NAME);
            let source = match std::fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_file() => {
                    ConfigurationSource::WorkspaceFile
                }
                Ok(_unsafe_kind) => {
                    return Err(StoreError::UnsafePath {
                        path,
                        message: "the configuration is not one regular file".to_owned(),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    ConfigurationSource::Defaults
                }
                Err(source) => {
                    return Err(StoreError::NotKept {
                        path: path.display().to_string(),
                        source,
                    });
                }
            };
            crate::config::Config::load(&self.path)
                .map(|config| LoadedConfiguration { config, source })
                .map_err(StoreError::from)
        }
    }

    /// Derives a report store from the same workspace object that supplied the configuration.
    ///
    /// # Errors
    /// Refuses when the retained workspace descriptor cannot be duplicated.
    #[cfg_attr(
        not(unix),
        expect(
            clippy::unnecessary_wraps,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    pub(crate) fn store(
        &self,
        configured: &crate::config::ReportDirectory,
    ) -> Result<Store, StoreError> {
        let root = self.path.join(configured.as_path());
        #[cfg(unix)]
        let workspace = self
            .directory
            .try_clone()
            .map_err(|source| StoreError::NotKept {
                path: self.path.display().to_string(),
                source,
            })?;
        #[cfg(unix)]
        let report_root = {
            let bound = std::sync::Arc::new(std::sync::OnceLock::new());
            match open_existing_configured_root(&self.directory, configured.as_path()) {
                Ok(ExistingStoreRoot::Missing) => {}
                Ok(ExistingStoreRoot::Present(directory)) => {
                    if bound.set(directory).is_err() {
                        return Err(StoreError::UnsafePath {
                            path: root,
                            message: "a fresh store root capability was already initialized"
                                .to_owned(),
                        });
                    }
                }
                Err(source) => {
                    return Err(StoreError::NotKept {
                        path: root.display().to_string(),
                        source,
                    });
                }
            }
            bound
        };
        Ok(Store {
            runs: root.join(RUNS_NAME),
            root,
            configured: configured.clone(),
            #[cfg(unix)]
            workspace,
            #[cfg(unix)]
            report_root,
        })
    }
}

impl StoredRun {
    /// The canonical identity bound to this open directory.
    #[must_use]
    pub(crate) const fn id(&self) -> &StoredRunId {
        &self.id
    }

    /// A display-only document spelling for diagnostics.
    #[must_use]
    pub(crate) fn document_display(&self) -> String {
        self.display.join(DOCUMENT_NAME).display().to_string()
    }

    /// The canonical workspace-relative document spelling for presentation.
    #[must_use]
    pub(crate) fn said_document(&self) -> &str {
        &self.said_document
    }

    /// Reads one closed stored file through this held run capability.
    ///
    /// # Errors
    /// Refuses symlinks, non-regular entries, oversized bytes, concurrent changes, non-UTF-8 text, and unsupported hosts.
    #[cfg_attr(
        not(unix),
        expect(
            clippy::missing_const_for_fn,
            clippy::unused_self,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    pub(crate) fn read(&self, file: StoredFile) -> Result<Option<String>, StoreError> {
        #[cfg(unix)]
        {
            let path = self.display.join(file.name());
            let Some(bytes) = read_optional_regular_at(&self.directory, file.name(), &path)? else {
                return Ok(None);
            };
            String::from_utf8(bytes)
                .map(Some)
                .map_err(|source| StoreError::UnsafePath {
                    path,
                    message: format!("stored report text is not UTF-8: {source}"),
                })
        }
        #[cfg(not(unix))]
        {
            #[cfg_attr(
                not(unix),
                expect(
                    clippy::no_effect_underscore_binding,
                    reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
                )
            )]
            let _file = file;
            Err(StoreError::UnsupportedCapability)
        }
    }

    /// Reads the required canonical report document.
    ///
    /// # Errors
    /// Returns the exact capability read failure, including an absent file.
    pub(crate) fn document(&self) -> Result<String, StoreError> {
        self.read(StoredFile::Document)?
            .ok_or_else(|| StoreError::UnsafePath {
                path: self.display.join(DOCUMENT_NAME),
                message: "the stored run has no canonical report document".to_owned(),
            })
    }
}

impl Store {
    /// The store a command uses when it has read the configuration.
    ///
    /// # Errors
    /// Refuses an unreadable, malformed, or invalid configuration instead of silently selecting the default report root.
    pub fn read(root: &Path) -> Result<Self, StoreError> {
        let workspace = WorkspaceRoot::open(root)?;
        let loaded = workspace.load_config()?;
        workspace.store(&loaded.config.reports.directory)
    }

    /// The directory every run writes its own directory under.
    #[must_use]
    fn runs(&self) -> &Path {
        &self.runs
    }

    /// Where one previously-stored run is read.
    #[must_use]
    fn run(&self, id: &StoredRunId) -> PathBuf {
        self.runs.join(id.as_str())
    }

    /// Where one newly-created run writes.
    #[must_use]
    pub(crate) fn writable_run(&self, id: &RunId) -> PathBuf {
        self.run(&StoredRunId::from(id))
    }

    /// Exclusively reserves one unpublished report directory.
    ///
    /// # Errors
    /// Refuses an existing entry, an ASCII-case alias, or any filesystem failure.
    /// A model phase therefore cannot write into a stale or pre-populated report namespace.
    #[cfg(unix)]
    pub(crate) fn claim_writable_run(&self, run_id: &RunId) -> Result<RunDirectory, StoreError> {
        let staging_root = self.root.join(STAGING_NAME);
        let published = self.writable_run(run_id);
        let staging = staging_root.join(run_id.as_str());
        let store = self.try_clone().map_err(|source| StoreError::NotKept {
            path: self.root.display().to_string(),
            source,
        })?;
        let root = self.open_or_create_root()?;
        let capability = match RunCapability::claim_beneath(&root, run_id.as_str()) {
            Ok(capability) => capability,
            Err(source) => {
                return Err(StoreError::NotKept {
                    path: staging.display().to_string(),
                    source,
                });
            }
        };
        Ok(RunDirectory {
            store,
            run_id: run_id.clone(),
            staging,
            published,
            capability,
            ownership: RunOwnership::Staging,
        })
    }

    #[cfg(not(unix))]
    pub(crate) fn claim_writable_run(&self, run_id: &RunId) -> Result<RunDirectory, StoreError> {
        let staging = self.root.join(STAGING_NAME).join(run_id.as_str());
        let published = self.writable_run(run_id);
        let capability = RunCapability::claim(self, run_id.as_str())?;
        Ok(RunDirectory {
            store: self.try_clone().map_err(|source| StoreError::NotKept {
                path: self.root.display().to_string(),
                source,
            })?,
            run_id: run_id.clone(),
            staging,
            published,
            capability,
            ownership: RunOwnership::Staging,
        })
    }

    /// Opens one explicit canonical run beneath this held store root.
    ///
    /// # Errors
    /// Refuses absent, aliased, symlinked, replaced, or unsupported entries.
    #[cfg_attr(
        not(unix),
        expect(
            clippy::missing_const_for_fn,
            clippy::unused_self,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    pub(crate) fn open_run(&self, id: &StoredRunId) -> Result<StoredRun, StoreError> {
        #[cfg(unix)]
        {
            let root = match self.open_existing_root()? {
                StoreRootState::Missing => {
                    return Err(StoreError::UnsafePath {
                        path: self.root.clone(),
                        message: "the configured report store does not exist".to_owned(),
                    });
                }
                StoreRootState::Open(root) => root,
            };
            let runs = RunsRoot::open_at(&root)?;
            open_stored_run(self, &runs, id)
        }
        #[cfg(not(unix))]
        {
            #[cfg_attr(
                not(unix),
                expect(
                    clippy::no_effect_underscore_binding,
                    reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
                )
            )]
            let _id = id;
            Err(StoreError::UnsupportedCapability)
        }
    }

    /// Opens the exact run selected by one held and strictly decoded index.
    ///
    /// # Errors
    /// Returns the closed index or run-directory capability failure.
    #[cfg_attr(
        not(unix),
        expect(
            clippy::missing_const_for_fn,
            clippy::unused_self,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    pub(crate) fn pointed_run(&self, index: Index) -> Result<Option<StoredRun>, StoreError> {
        #[cfg(unix)]
        {
            let root = match self.open_existing_root()? {
                StoreRootState::Missing => return Ok(None),
                StoreRootState::Open(root) => root,
            };
            let runs = RunsRoot::open_at(&root)?;
            let Some(id) = pointed_at_root(self, &root, &runs, index)? else {
                return Ok(None);
            };
            open_stored_run(self, &runs, &id).map(Some)
        }
        #[cfg(not(unix))]
        {
            #[cfg_attr(
                not(unix),
                expect(
                    clippy::no_effect_underscore_binding,
                    reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
                )
            )]
            let _index = index;
            Err(StoreError::UnsupportedCapability)
        }
    }

    /// The canonical run identity one index names, retained only for authority-internal retention and tests.
    ///
    /// # Errors
    /// Returns the closed index error when the pointer or run namespace is unreadable, malformed, or unsafe.
    #[cfg(any(test, feature = "testkit"))]
    pub(crate) fn pointed_at(&self, index: Index) -> Result<Option<StoredRunId>, StoreError> {
        self.pointed_run(index)
            .map(|run| run.map(|opened| opened.id))
    }

    #[cfg(unix)]
    fn open_existing_root(&self) -> Result<StoreRootState, StoreError> {
        if let Some(directory) = self.bound_root()? {
            return Ok(StoreRootState::Open(StoreRoot { directory }));
        }
        match open_existing_configured_root(&self.workspace, self.configured.as_path()) {
            Ok(ExistingStoreRoot::Missing) => Ok(StoreRootState::Missing),
            Ok(ExistingStoreRoot::Present(directory)) => self
                .bind_root(directory)
                .map(|directory| StoreRootState::Open(StoreRoot { directory })),
            Err(source) => Err(StoreError::NotKept {
                path: self.root.display().to_string(),
                source,
            }),
        }
    }

    #[cfg(unix)]
    fn open_or_create_root(&self) -> Result<Dir, StoreError> {
        if let Some(directory) = self.bound_root()? {
            return Ok(directory);
        }
        let directory =
            open_configured_root(&self.workspace, self.configured.as_path()).map_err(|source| {
                StoreError::NotKept {
                    path: self.root.display().to_string(),
                    source,
                }
            })?;
        self.bind_root(directory)
    }

    #[cfg(unix)]
    fn bound_root(&self) -> Result<Option<Dir>, StoreError> {
        self.report_root
            .get()
            .map(Dir::try_clone)
            .transpose()
            .map_err(|source| StoreError::NotKept {
                path: self.root.display().to_string(),
                source,
            })
    }

    #[cfg(unix)]
    fn bind_root(&self, candidate: Dir) -> Result<Dir, StoreError> {
        if let Some(bound) = self.report_root.get() {
            require_same_directory(bound, &candidate).map_err(|source| StoreError::NotKept {
                path: self.root.display().to_string(),
                source,
            })?;
            return bound.try_clone().map_err(|source| StoreError::NotKept {
                path: self.root.display().to_string(),
                source,
            });
        }
        match self.report_root.set(candidate) {
            Ok(()) => self.bound_root()?.ok_or_else(|| StoreError::UnsafePath {
                path: self.root.clone(),
                message: "the configured report root was not retained after initialization"
                    .to_owned(),
            }),
            Err(candidate) => {
                let bound = self
                    .report_root
                    .get()
                    .ok_or_else(|| StoreError::UnsafePath {
                        path: self.root.clone(),
                        message: "a concurrent report-root initialization retained no capability"
                            .to_owned(),
                    })?;
                require_same_directory(bound, &candidate).map_err(|source| {
                    StoreError::NotKept {
                        path: self.root.display().to_string(),
                        source,
                    }
                })?;
                bound.try_clone().map_err(|source| StoreError::NotKept {
                    path: self.root.display().to_string(),
                    source,
                })
            }
        }
    }

    /// Where one index that names the newest run sits.
    #[must_use]
    fn index(&self, index: Index) -> PathBuf {
        self.root.join(index.file())
    }

    /// A display-only rendering of one index path.
    #[must_use]
    pub(crate) fn index_display(&self, index: Index) -> String {
        self.index(index).display().to_string()
    }

    fn named_exact(&self, run_id: &StoredRunId) -> String {
        format!("{}/{RUNS_NAME}/{run_id}", self.configured.as_str())
    }

    #[cfg_attr(
        not(unix),
        expect(
            clippy::unnecessary_wraps,
            reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
        )
    )]
    fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            runs: self.runs.clone(),
            root: self.root.clone(),
            configured: self.configured.clone(),
            #[cfg(unix)]
            workspace: self.workspace.try_clone()?,
            #[cfg(unix)]
            report_root: std::sync::Arc::clone(&self.report_root),
        })
    }

    /// Retires old runs through this store's already-held workspace root.
    ///
    /// # Errors
    /// Refuses an incomplete index view, unsafe run entry, unsupported host,
    /// or any deletion/synchronization failure.
    pub(crate) fn retain(&self, keep: u32) -> Result<Vec<PathBuf>, StoreError> {
        retain_with_capability(self, keep)
    }

    /// Publishes one complete report through this already-held workspace and store capability.
    ///
    /// # Errors
    /// Returns the same closed publication errors as the module-level keep.
    pub(crate) fn keep(&self, report: &Report) -> Result<Kept, StoreError> {
        let run_id =
            RunId::try_from(report.run_id()).map_err(|source| StoreError::RunId { source })?;
        let directory = self.claim_writable_run(&run_id)?;
        keep_claimed(&ReportDocument::Complete(report.clone()), directory)
    }
}

/// The directory runs sit in, under the report directory the configuration names.
const RUNS_NAME: &str = "runs";

/// Unpublished runs live beside, never inside, the reader-visible run set.
const STAGING_NAME: &str = ".pending-runs";

/// One of the two files that name the newest run.
///
/// A file name is not a path: joined onto the wrong root it reads as a run nobody stored, which is what happened to six tests when the report directory became configuration.
/// Only [`Store`] turns one into a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Index {
    /// The latest completed run of any scope.
    Any,
    /// The latest completed run that looked at the whole project.
    Full,
}

impl Index {
    /// What the file is called inside the report directory.
    #[must_use]
    pub const fn file(self) -> &'static str {
        match self {
            Self::Any => "latest-any.json",
            Self::Full => "latest-full.json",
        }
    }
}

/// The canonical document inside a run directory.
pub const DOCUMENT_NAME: &str = "njutest-assurance-report-v1.json";

/// The published schema, copied in beside the document it describes.
pub const SCHEMA_NAME: &str = "njutest-assurance-report-v1.schema.json";

const SCHEMA_TEXT: &str = include_str!("../../../../schema/njutest-assurance-report-v1.json");

/// The page a person opens.
pub const HTML_NAME: &str = "njutest-assurance-report-v1.html";

/// The findings, for a code-scanning surface.
pub const SARIF_NAME: &str = "njutest-assurance-report-v1.sarif";

/// The targets and findings, for a continuous integration surface.
pub const JUNIT_NAME: &str = "njutest-assurance-report-v1.junit.xml";

/// Why a report could not be kept.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The configured report root could not be established.
    #[error(transparent)]
    Configuration(#[from] crate::config::ConfigError),
    /// This non-Unix host lacks the handle-relative filesystem backend needed for authoritative report reads, writes, deletion, and publication.
    #[error(
        "{}: this platform has no supported capability-rooted report publication backend",
        error::REPORT_NOT_KEPT.code
    )]
    UnsupportedCapability,
    /// The report itself is not one that may be persisted.
    #[error(transparent)]
    Report(#[from] json::ReportError),
    /// A derived projection could not represent exact accounting.
    #[error(transparent)]
    Count(#[from] crate::report::CountError),
    /// The JUnit projection could not be rendered exactly.
    #[error(transparent)]
    Junit(#[from] crate::report::junit::JunitError),
    /// A report or index named a run with a value that is not one safe path component.
    #[error("{}: {source}", error::REPORT_NOT_KEPT.code)]
    RunId {
        /// Why the name is not canonical.
        #[source]
        source: RunIdError,
    },
    /// An index could not be read or decoded exactly.
    #[error("{}: reading {path}: {message}", error::REPORT_NOT_KEPT.code)]
    Index {
        /// The index.
        path: PathBuf,
        /// What made it unusable.
        message: String,
    },
    /// A path in the store is a symlink or another entry kind the store never follows.
    #[error("{}: unsafe store entry {path}: {message}", error::REPORT_NOT_KEPT.code)]
    UnsafePath {
        /// The entry that was refused.
        path: PathBuf,
        /// Which invariant it violated.
        message: String,
    },
    /// The run directory could not be written.
    #[error("{}: writing {path}: {source}", error::REPORT_NOT_KEPT.code)]
    NotKept {
        /// The path.
        path: String,
        /// The failure.
        #[source]
        source: io::Error,
    },
    /// A failed unpublished write was followed by a failed cleanup.
    #[error(
        "{}: {primary}; cleanup of {} also failed: {cleanup}",
        error::REPORT_NOT_KEPT.code,
        path.display()
    )]
    Abort {
        /// The write or path invariant that first failed.
        primary: PublicationFailure,
        /// The staging or just-published directory that could not be removed completely.
        path: PathBuf,
        /// What prevented cleanup.
        #[source]
        cleanup: io::Error,
    },
}

/// A failure that can occur after an unpublished run directory is owned.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PublicationFailure {
    /// The host cannot bind authoritative report bytes strongly enough to publish them.
    #[error("this platform has no supported capability-rooted report backend")]
    UnsupportedCapability,
    /// The completed document failed its own closed audit.
    #[error(transparent)]
    Report(#[from] json::ReportError),
    /// A report projection could not represent exact accounting.
    #[error(transparent)]
    Count(#[from] crate::report::CountError),
    /// The JUnit projection could not be rendered exactly.
    #[error(transparent)]
    Junit(#[from] crate::report::junit::JunitError),
    /// The completed document did not carry a canonical run component.
    #[error("run identity is not canonical: {source}")]
    RunId {
        /// Why the name is not canonical.
        #[source]
        source: RunIdError,
    },
    /// A path in the private staging tree changed to an unsafe entry kind.
    #[error("unsafe store entry {path}: {message}")]
    UnsafePath {
        /// The entry that was refused.
        path: PathBuf,
        /// Which invariant it violated.
        message: String,
    },
    /// Bytes could not be durably written or the completed tree could not be published.
    #[error("writing {path}: {source}")]
    NotKept {
        /// The path that failed.
        path: String,
        /// The filesystem failure.
        #[source]
        source: io::Error,
    },
}

impl StoreError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Configuration(error) => error.code(),
            Self::Report(error) => error.code(),
            Self::Count(_) | Self::Junit(_) => error::REPORT_UNSOUND,
            Self::UnsupportedCapability
            | Self::RunId { .. }
            | Self::Index { .. }
            | Self::UnsafePath { .. }
            | Self::NotKept { .. }
            | Self::Abort { .. } => error::REPORT_NOT_KEPT,
        }
    }
}

impl From<PublicationFailure> for StoreError {
    fn from(failure: PublicationFailure) -> Self {
        match failure {
            PublicationFailure::UnsupportedCapability => Self::UnsupportedCapability,
            PublicationFailure::Report(error) => Self::Report(error),
            PublicationFailure::Count(source) => Self::Count(source),
            PublicationFailure::Junit(source) => Self::Junit(source),
            PublicationFailure::RunId { source } => Self::RunId { source },
            PublicationFailure::UnsafePath { path, message } => Self::UnsafePath { path, message },
            PublicationFailure::NotKept { path, source } => Self::NotKept { path, source },
        }
    }
}

/// Where one run's report was kept.
#[derive(Debug)]
pub struct Kept {
    /// The testkit-only filesystem spelling used to inspect fixture bytes.
    ///
    /// It is never an authority handle and production code cannot obtain it;
    /// tests must not reopen it to make a publication or reader decision.
    #[cfg(any(test, feature = "testkit"))]
    pub directory: PathBuf,
    /// The canonical workspace-relative document spelling for presentation.
    pub said_document: String,
    /// Whether the derived latest-run indexes were updated after the canonical directory became durable.
    pub indexes: IndexPublication,
}

/// The non-authoritative index side of a durable report publication.
///
/// The canonical run directory is the authority.
/// Filesystem APIs cannot atomically rename that directory and two independent pointer files as one transaction, so an index failure after the directory rename is a successful publication with a repairable index failure, never `REPORT_NOT_KEPT`.
#[derive(Debug)]
pub enum IndexPublication {
    /// Shards are merge inputs and deliberately update no latest-complete pointer.
    NotRequired,
    /// Every pointer this complete report owns was updated.
    Complete,
    /// The report is durable, but this derived pointer was not updated.
    Incomplete {
        /// The first pointer that could not be updated.
        index: Index,
        /// The exact closed store failure.
        error: StoreError,
    },
    /// A derived index may already name the durable run, but the held run no longer matched its sealed ledger after the index update.
    /// The canonical directory is deliberately retained: deleting it would leave a durable pointer naming a missing run.
    RunChanged {
        /// The exact post-commit integrity failure.
        error: StoreError,
    },
}

impl IndexPublication {
    /// Whether retention can safely rely on the latest-run pointers.
    #[must_use]
    pub const fn permits_retention(&self) -> bool {
        matches!(self, Self::NotRequired | Self::Complete)
    }
}

/// Writes `report` into its own directory under `root`, then points the indexes at it.
///
/// # Errors
/// [`StoreError::Report`] when the report fails its own audit — nothing is written then — and [`StoreError::NotKept`] when the authoritative run directory cannot be published.
/// A derived index failure is returned in [`Kept::indexes`] because it cannot undo an already-durable authority.
#[cfg(feature = "testkit")]
pub fn keep(root: &Path, report: &Report) -> Result<Kept, StoreError> {
    keep_any(root, &ReportDocument::Complete(report.clone()))
}

/// Writes a checked complete or shard document into its own run directory.
///
/// A shard is retained only as canonical merge input: it does not update the latest-complete indexes and does not acquire complete-report projections.
///
/// # Errors
/// [`StoreError::Report`] when the document cannot be serialized or audited,
/// and [`StoreError::NotKept`] when the authoritative run directory cannot be published.
/// A derived index failure is returned in [`Kept::indexes`].
#[cfg(feature = "testkit")]
pub fn keep_any(root: &Path, report: &ReportDocument) -> Result<Kept, StoreError> {
    let run_id = RunId::try_from(report.run_id()).map_err(|source| StoreError::RunId { source })?;
    let directory = Store::read(root)?.claim_writable_run(&run_id)?;
    keep_claimed(report, directory)
}

/// Writes a document into the exact directory exclusively claimed before a post-lattice model phase began.
///
/// # Errors
/// Refuses a claim for another root or run, an unsound document, or any I/O failure.
pub(crate) fn keep_claimed(
    report: &ReportDocument,
    claimed: RunDirectory,
) -> Result<Kept, StoreError> {
    let run_id = match RunId::try_from(report.run_id()) {
        Ok(run_id) => run_id,
        Err(source) => {
            return Err(abort_after(claimed, PublicationFailure::RunId { source }));
        }
    };
    let document_text = match json::document_any(report) {
        Ok(document) => document,
        Err(error) => return Err(abort_after(claimed, PublicationFailure::Report(error))),
    };
    let store = &claimed.store;
    let published = store.writable_run(&run_id);
    let expected_staging = store.root.join(STAGING_NAME).join(run_id.as_str());
    let staging_is_directory = match claimed.staging_entry_matches() {
        Ok(matches) => matches,
        Err(source) => {
            let path = claimed.staging.display().to_string();
            return Err(abort_after(
                claimed,
                PublicationFailure::NotKept { path, source },
            ));
        }
    };
    if claimed.run_id != run_id
        || claimed.published != published
        || claimed.staging != expected_staging
        || !staging_is_directory
    {
        let error = PublicationFailure::UnsafePath {
            path: claimed.staging.clone(),
            message: "the run-directory claim does not belong to this report and store".to_owned(),
        };
        return Err(abort_after(claimed, error));
    }
    let said_document = format!(
        "{}/{DOCUMENT_NAME}",
        store.named_exact(&StoredRunId::from(&run_id))
    );
    let sealed = seal(claimed, report, &document_text)?;
    let publication = publish(sealed, |authority| {
        publish_indexes(authority, report, &run_id)
    })?;
    let indexes = publication.1;
    #[cfg(any(test, feature = "testkit"))]
    let directory = publication.0;
    Ok(Kept {
        #[cfg(any(test, feature = "testkit"))]
        directory,
        said_document,
        indexes,
    })
}

fn seal(
    claimed: RunDirectory,
    report: &ReportDocument,
    document_text: &str,
) -> Result<SealedRunDirectory, StoreError> {
    if !RunCapability::supports_model_artifacts() && !report.model_artifacts().is_empty() {
        return Err(abort_after(
            claimed,
            PublicationFailure::UnsupportedCapability,
        ));
    }
    if let Err(error) = validate_model_artifacts(&claimed, report) {
        return Err(abort_after(claimed, error));
    }
    let files = match write_publication(&claimed, report, document_text) {
        Ok(files) => files,
        Err(error) => return Err(abort_after(claimed, error)),
    };
    if let Err(source) = claimed.sync_tree() {
        let error = PublicationFailure::NotKept {
            path: claimed.staging.display().to_string(),
            source,
        };
        return Err(abort_after(claimed, error));
    }
    let artifacts = report
        .model_artifacts()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    if let Err(error) = validate_sealed_tree(&claimed, &files, &artifacts) {
        return Err(abort_after(claimed, error));
    }
    Ok(SealedRunDirectory {
        artifacts,
        files,
        directory: claimed,
    })
}

fn publish_indexes(
    authority: &RunDirectory,
    report: &ReportDocument,
    run_id: &RunId,
) -> IndexPublication {
    let ReportDocument::Complete(report) = report else {
        return IndexPublication::NotRequired;
    };
    if let Err(error) = point(authority, Index::Any, run_id) {
        return IndexPublication::Incomplete {
            index: Index::Any,
            error,
        };
    }
    if report.run_kind() == crate::report::RunKind::Full
        && let Err(error) = point(authority, Index::Full, run_id)
    {
        return IndexPublication::Incomplete {
            index: Index::Full,
            error,
        };
    }
    IndexPublication::Complete
}

fn write_publication(
    directory: &RunDirectory,
    report: &ReportDocument,
    document_text: &str,
) -> Result<Vec<PublicationFile>, PublicationFailure> {
    let mut files = Vec::new();
    retain_publication_file(
        directory,
        &mut files,
        DOCUMENT_NAME,
        document_text.as_bytes().to_vec(),
    )?;
    retain_publication_file(
        directory,
        &mut files,
        SCHEMA_NAME,
        SCHEMA_TEXT.as_bytes().to_vec(),
    )?;
    if let ReportDocument::Complete(report) = report {
        let lines = crate::report::lines::stream(report)?;
        retain_publication_file(
            directory,
            &mut files,
            crate::report::lines::FILE_NAME,
            lines.into_bytes(),
        )?;
        let html = crate::report::html::document(report)?;
        retain_publication_file(directory, &mut files, HTML_NAME, html.into_bytes())?;
        let sarif = crate::report::sarif::document(report)?;
        retain_publication_file(
            directory,
            &mut files,
            SARIF_NAME,
            format!("{sarif:#}\n").into_bytes(),
        )?;
        let junit = crate::report::junit::document(report)?;
        retain_publication_file(directory, &mut files, JUNIT_NAME, junit.into_bytes())?;
    }
    Ok(files)
}

fn retain_publication_file(
    directory: &RunDirectory,
    files: &mut Vec<PublicationFile>,
    name: &'static str,
    bytes: Vec<u8>,
) -> Result<(), PublicationFailure> {
    directory.write_new(name, &bytes)?;
    files.push(PublicationFile { name, bytes });
    Ok(())
}

fn validate_model_artifacts(
    directory: &RunDirectory,
    report: &ReportDocument,
) -> Result<(), PublicationFailure> {
    for artifact in report.model_artifacts() {
        validate_model_artifact(directory, artifact)?;
    }
    Ok(())
}

fn validate_sealed_tree(
    directory: &RunDirectory,
    files: &[PublicationFile],
    artifacts: &[crate::report::ModelArtifact],
) -> Result<(), PublicationFailure> {
    directory
        .validate_closed_tree(files, artifacts)
        .map_err(|source| PublicationFailure::NotKept {
            path: directory.staging.display().to_string(),
            source,
        })?;
    for file in files {
        let retained = directory.read_publication_file(file).map_err(|source| {
            PublicationFailure::NotKept {
                path: directory.staging.join(file.name).display().to_string(),
                source,
            }
        })?;
        if retained != file.bytes {
            return Err(PublicationFailure::UnsafePath {
                path: directory.staging.join(file.name),
                message: "publication bytes changed after the sealed file was written".to_owned(),
            });
        }
    }
    for artifact in artifacts {
        validate_model_artifact(directory, artifact)?;
    }
    Ok(())
}

fn validate_model_artifact(
    directory: &RunDirectory,
    artifact: &crate::report::ModelArtifact,
) -> Result<(), PublicationFailure> {
    let bytes =
        directory
            .read_model_artifact(artifact)
            .map_err(|source| PublicationFailure::NotKept {
                path: directory
                    .staging
                    .join(artifact.path())
                    .display()
                    .to_string(),
                source,
            })?;
    if !artifact.matches(&bytes) {
        return Err(PublicationFailure::UnsafePath {
            path: directory.staging.join(artifact.path()),
            message: "model artifact bytes no longer match their retained size and digest"
                .to_owned(),
        });
    }
    Ok(())
}

fn abort_after(claimed: RunDirectory, primary: PublicationFailure) -> StoreError {
    let path = match claimed.ownership {
        RunOwnership::Staging => claimed.staging.clone(),
        RunOwnership::Published => claimed.published.clone(),
        RunOwnership::Committed | RunOwnership::Disarmed => {
            return StoreError::UnsafePath {
                path: claimed.staging.clone(),
                message: format!(
                    "a committed or disarmed run-directory claim encountered publication failure: \
                     {primary}; the canonical directory was retained"
                ),
            };
        }
    };
    cleanup_after(primary, path, claimed.abort())
}

fn cleanup_after(
    primary: PublicationFailure,
    path: PathBuf,
    cleanup_result: Result<(), StoreError>,
) -> StoreError {
    match cleanup_result {
        Ok(()) => StoreError::from(primary),
        Err(StoreError::NotKept {
            path: _cleanup_path,
            source: cleanup,
        }) => StoreError::Abort {
            primary,
            path,
            cleanup,
        },
        Err(cleanup) => StoreError::Abort {
            primary,
            path,
            cleanup: io::Error::other(cleanup),
        },
    }
}

fn publish<I>(
    claimed: SealedRunDirectory,
    indexes: I,
) -> Result<(PathBuf, IndexPublication), StoreError>
where
    I: FnOnce(&RunDirectory) -> IndexPublication,
{
    publish_with(
        claimed,
        |claim, _from, _to| claim.publish_entry(),
        |claim, directory| {
            let published = directory == claim.store.runs();
            claim.sync_rename_parent(published)
        },
        indexes,
    )
}

fn publish_with<R, S, I>(
    sealed: SealedRunDirectory,
    rename: R,
    mut sync: S,
    indexes: I,
) -> Result<(PathBuf, IndexPublication), StoreError>
where
    R: FnOnce(&RunDirectory, &Path, &Path) -> io::Result<()>,
    S: FnMut(&RunDirectory, &Path) -> io::Result<()>,
    I: FnOnce(&RunDirectory) -> IndexPublication,
{
    let (mut directory, artifacts, files) = rename_sealed(sealed, rename)?;
    let checked = validate_named_sealed(
        &directory,
        &files,
        &artifacts,
        "the published name does not refer to the held staging directory",
    );
    directory = retain_checked_directory(directory, checked)?;
    let synced = sync_publication_parents(&directory, &mut sync);
    directory = retain_checked_directory(directory, synced)?;
    let checked = validate_named_sealed(
        &directory,
        &files,
        &artifacts,
        "the published name changed identity before commit",
    );
    directory = retain_checked_directory(directory, checked)?;
    directory.ownership = RunOwnership::Committed;
    let indexes = indexes(&directory);
    let checked = validate_named_sealed(
        &directory,
        &files,
        &artifacts,
        "the published name changed identity while derived indexes were written",
    );
    let indexes = match checked {
        Ok(()) => indexes,
        Err(error) => IndexPublication::RunChanged {
            error: StoreError::from(error),
        },
    };
    let published = directory.published.clone();
    directory.ownership = RunOwnership::Disarmed;
    Ok((published, indexes))
}

fn rename_sealed<R>(
    sealed: SealedRunDirectory,
    rename: R,
) -> Result<
    (
        RunDirectory,
        Vec<crate::report::ModelArtifact>,
        Vec<PublicationFile>,
    ),
    StoreError,
>
where
    R: FnOnce(&RunDirectory, &Path, &Path) -> io::Result<()>,
{
    let SealedRunDirectory {
        mut directory,
        artifacts,
        files,
    } = sealed;
    if let Err(source) = directory.validate_run_spelling() {
        let path = directory.published.display().to_string();
        return Err(abort_after(
            directory,
            PublicationFailure::NotKept { path, source },
        ));
    }
    if let Err(source) = rename(&directory, &directory.staging, &directory.published) {
        let path = directory.published.display().to_string();
        return Err(abort_after(
            directory,
            PublicationFailure::NotKept { path, source },
        ));
    }
    directory.ownership = RunOwnership::Published;
    Ok((directory, artifacts, files))
}

fn validate_named_sealed(
    directory: &RunDirectory,
    files: &[PublicationFile],
    artifacts: &[crate::report::ModelArtifact],
    mismatch: &str,
) -> Result<(), PublicationFailure> {
    let spelling =
        directory
            .validate_run_spelling()
            .map_err(|source| PublicationFailure::NotKept {
                path: directory.store.runs().display().to_string(),
                source,
            });
    match (spelling, directory.published_entry_matches()) {
        (Ok(()), Ok(true)) => validate_sealed_tree(directory, files, artifacts),
        (Err(error), _) => Err(error),
        (Ok(()), Ok(false)) => Err(PublicationFailure::UnsafePath {
            path: directory.published.clone(),
            message: mismatch.to_owned(),
        }),
        (Ok(()), Err(source)) => Err(PublicationFailure::NotKept {
            path: directory.published.display().to_string(),
            source,
        }),
    }
}

fn sync_publication_parents<S>(
    directory: &RunDirectory,
    sync: &mut S,
) -> Result<(), PublicationFailure>
where
    S: FnMut(&RunDirectory, &Path) -> io::Result<()>,
{
    let staging_parent =
        directory
            .staging
            .parent()
            .ok_or_else(|| PublicationFailure::UnsafePath {
                path: directory.staging.clone(),
                message: "the published staging spelling has no parent".to_owned(),
            })?;
    for parent in [directory.store.runs(), staging_parent] {
        sync(directory, parent).map_err(|source| PublicationFailure::NotKept {
            path: parent.display().to_string(),
            source,
        })?;
    }
    Ok(())
}

fn retain_checked_directory(
    directory: RunDirectory,
    checked: Result<(), PublicationFailure>,
) -> Result<RunDirectory, StoreError> {
    match checked {
        Ok(()) => Ok(directory),
        Err(error) => Err(abort_after(directory, error)),
    }
}

#[cfg(unix)]
fn sync_open_tree(directory: &Dir) -> io::Result<()> {
    for entry in directory.entries()? {
        let entry_name = name(&entry)?;
        match directory.status_at(entry_name)?.map(|status| status.kind) {
            Some(Kind::File) => {
                let file = directory.open_file(entry_name)?;
                if rust_mutants::capdir::file_status(&file)?.kind != Kind::File {
                    return Err(io::Error::other(format!(
                        "unpublished report entry {entry:?} changed kind while it was synced"
                    )));
                }
                file.sync_all()?;
            }
            Some(Kind::Directory) => sync_open_tree(&directory.open_dir(entry_name)?)?,
            Some(Kind::Other) | None => {
                return Err(io::Error::other(format!(
                    "unpublished report entry {entry:?} is neither a regular file nor a directory"
                )));
            }
        }
    }
    directory.sync()
}

/// Removes the oldest run directories beyond `keep`, newest first by name — which is chronological, because that is what a run identity is for.
///
/// # Errors
/// A run-directory entry could not be read, so no partial view is used to decide what may be removed.
#[cfg(feature = "testkit")]
pub fn retain(root: &Path, keep: u32) -> Result<Vec<PathBuf>, StoreError> {
    Store::read(root)?.retain(keep)
}

/// The retention quarantine stem, kept beside the removal it guards.
#[cfg(unix)]
const RETENTION_STEM: &str = ".njutest-retention";

#[cfg(unix)]
fn retain_with_capability(store: &Store, keep: u32) -> Result<Vec<PathBuf>, StoreError> {
    let root = match store.open_existing_root()? {
        StoreRootState::Missing => return Ok(Vec::new()),
        StoreRootState::Open(root) => root,
    };
    let runs = RunsRoot::open_at(&root)?;
    let spellings = stored_spellings_at(store, &runs)?;
    let mut names = spellings.into_values().collect::<Vec<_>>();
    names.sort();
    names.reverse();
    let mut protected = Vec::new();
    for index in Index::ALL {
        if let Some(run_id) = pointed_at_root(store, &root, &runs, index)? {
            protected.push(run_id);
        }
    }

    let keep = usize::try_from(keep).map_err(|source| StoreError::NotKept {
        path: store.root.display().to_string(),
        source: io::Error::other(source),
    })?;
    let mut removed = Vec::new();
    for (candidate, path) in names
        .into_iter()
        .skip(keep)
        .filter(|(candidate, _path)| !protected.contains(candidate))
    {
        let held = open_directory_at(&runs.directory, candidate.as_str()).map_err(|source| {
            StoreError::NotKept {
                path: path.display().to_string(),
                source,
            }
        })?;
        let (quarantine, quarantined) =
            quarantine_retained_run(&runs.directory, candidate.as_str(), &held).map_err(
                |source| StoreError::NotKept {
                    path: path.display().to_string(),
                    source,
                },
            )?;
        if let Some(index) = index_now_names(store, &root, &candidate)? {
            restore_from_quarantine(&runs.directory, &quarantine, candidate.as_str()).map_err(
                |source| StoreError::NotKept {
                    path: path.display().to_string(),
                    source,
                },
            )?;
            return Err(StoreError::Index {
                path: store.index(index),
                message: format!(
                    "retention refused: the index named run {candidate:?} after the \
                     protection census was taken"
                ),
            });
        }
        destroy_quarantined_run(&runs.directory, &quarantine, &quarantined).map_err(|source| {
            StoreError::NotKept {
                path: path.display().to_string(),
                source,
            }
        })?;
        removed.push(path);
    }
    Ok(removed)
}

/// Quarantines one retained run under an exclusive token and proves the quarantined directory is the one the census opened.
///
/// # Errors
/// Returns the refusal for a run whose name changed identity first.
#[cfg(unix)]
fn quarantine_retained_run(runs: &Dir, candidate: &str, held: &Dir) -> io::Result<(String, Dir)> {
    let quarantine = quarantine_named_entry(runs, candidate, RETENTION_STEM)?;
    let quarantined = open_directory_at(runs, &quarantine)?;
    if held.status()?.identity != quarantined.status()?.identity {
        return Err(io::Error::other(
            "the retained run name changed identity before quarantine",
        ));
    }
    Ok((quarantine, quarantined))
}

/// Destroys one quarantined run and removes its quarantine name, proving the name still matches the opened directory on both sides of the removal.
///
/// # Errors
/// Returns the refusal for a run that changed identity while it was removed.
#[cfg(unix)]
fn destroy_quarantined_run(runs: &Dir, quarantine: &str, quarantined: &Dir) -> io::Result<()> {
    remove_open_tree(quarantined)?;
    if !named_directory_matches(runs, quarantine, quarantined)? {
        return Err(io::Error::other(
            "the retained run changed identity while it was removed",
        ));
    }
    runs.remove_dir(name(quarantine)?)?;
    runs.sync()
}

/// Which index names `candidate` now, read after the candidate is unreachable by name so a pointer written since the protection census is seen.
///
/// # Errors
/// Returns the closed index error when an index is unreadable or malformed.
#[cfg(unix)]
fn index_now_names(
    store: &Store,
    root: &StoreRoot,
    candidate: &StoredRunId,
) -> Result<Option<Index>, StoreError> {
    for index in Index::ALL {
        let path = store.index(index);
        let Some(text) = read_index_at(&root.directory, index, &path)? else {
            continue;
        };
        let pointer: Pointer =
            crate::strictjson::decode_str(&text).map_err(|error| StoreError::Index {
                path: path.clone(),
                message: error.to_string(),
            })?;
        if pointer.schema != crate::report::SCHEMA {
            return Err(StoreError::Index {
                path,
                message: format!("unknown index schema {:?}", pointer.schema),
            });
        }
        if pointer.run_id == *candidate {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

/// Puts a quarantined directory back under its own name.
///
/// # Errors
/// Returns the refusal when the original name was taken while it was away.
#[cfg(unix)]
fn restore_from_quarantine(runs: &Dir, quarantine: &str, run: &str) -> io::Result<()> {
    runs.rename_noreplace(name(quarantine)?, runs, name(run)?)
}

#[cfg(not(unix))]
#[cfg_attr(
    not(unix),
    expect(
        clippy::missing_const_for_fn,
        reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
    )
)]
fn retain_with_capability(_store: &Store, _keep: u32) -> Result<Vec<PathBuf>, StoreError> {
    Err(StoreError::UnsupportedCapability)
}

/// The run one index names, if it names one.
///
/// # Errors
/// Returns the closed index error when the index or named run directory is unreadable, malformed, or unsafe to follow.
#[cfg(feature = "testkit")]
pub fn pointed_at(root: &Path, index: Index) -> Result<Option<StoredRunId>, StoreError> {
    let store = Store::read(root)?;
    store.pointed_at(index)
}

/// Writes one index.
fn point(authority: &RunDirectory, index: Index, run_id: &RunId) -> Result<(), StoreError> {
    let store = &authority.store;
    let path = store.index(index);
    let stored_run_id = StoredRunId::from(run_id);
    let mut text = serde_json::to_string_pretty(&serde_json::json!({
        "schema": crate::report::SCHEMA,
        "run_id": run_id.as_str(),
        "directory": store.named_exact(&stored_run_id),
    }))
    .map_err(|source| StoreError::Index {
        path: path.clone(),
        message: source.to_string(),
    })?;
    text.push('\n');
    write_index(authority, index, text.as_bytes())
}

#[cfg(unix)]
fn pointed_at_root(
    store: &Store,
    root: &StoreRoot,
    runs: &RunsRoot,
    index: Index,
) -> Result<Option<StoredRunId>, StoreError> {
    let path = store.index(index);
    let Some(text) = read_index_at(&root.directory, index, &path)? else {
        return Ok(None);
    };
    let pointer: Pointer =
        crate::strictjson::decode_str(&text).map_err(|error| StoreError::Index {
            path: path.clone(),
            message: error.to_string(),
        })?;
    if pointer.schema != crate::report::SCHEMA {
        return Err(StoreError::Index {
            path,
            message: format!("unknown index schema {:?}", pointer.schema),
        });
    }
    let expected = store.named_exact(&pointer.run_id);
    if pointer.directory != expected {
        return Err(StoreError::Index {
            path,
            message: format!(
                "directory {:?} does not name run {} as {:?}",
                pointer.directory, pointer.run_id, expected
            ),
        });
    }
    let spellings = stored_spellings_at(store, runs)?;
    match spellings.get(&pointer.run_id.case_folded()) {
        Some((stored, _path)) if stored == &pointer.run_id => {}
        Some((stored, path)) => {
            return Err(StoreError::Index {
                path: path.clone(),
                message: format!(
                    "index run {:?} aliases stored run {stored:?} by ASCII case",
                    pointer.run_id
                ),
            });
        }
        None => {
            return Err(StoreError::Index {
                path,
                message: format!("index names missing run {:?}", pointer.run_id),
            });
        }
    }
    Ok(Some(pointer.run_id))
}

#[cfg(unix)]
fn open_stored_run(
    store: &Store,
    runs: &RunsRoot,
    id: &StoredRunId,
) -> Result<StoredRun, StoreError> {
    let spellings = stored_spellings_at(store, runs)?;
    match spellings.get(&id.case_folded()) {
        Some((stored, _path)) if stored == id => {}
        Some((stored, path)) => {
            return Err(StoreError::Index {
                path: path.clone(),
                message: format!("requested run {id:?} aliases stored run {stored:?}"),
            });
        }
        None => {
            return Err(StoreError::UnsafePath {
                path: store.run(id),
                message: format!("stored run {id:?} does not exist"),
            });
        }
    }
    let directory =
        open_directory_at(&runs.directory, id.as_str()).map_err(|source| StoreError::NotKept {
            path: store.run(id).display().to_string(),
            source,
        })?;
    if !named_directory_matches(&runs.directory, id.as_str(), &directory).map_err(|source| {
        StoreError::NotKept {
            path: store.run(id).display().to_string(),
            source,
        }
    })? {
        return Err(StoreError::UnsafePath {
            path: store.run(id),
            message: "the stored run changed identity while it was opened".to_owned(),
        });
    }
    Ok(StoredRun {
        id: id.clone(),
        display: store.run(id),
        said_document: format!("{}/{DOCUMENT_NAME}", store.named_exact(id)),
        directory,
    })
}

#[cfg(unix)]
fn read_index_at(root: &Dir, index: Index, path: &Path) -> Result<Option<String>, StoreError> {
    const MAX_INDEX_BYTES: u64 = 65_536;
    let refused = |message: String| StoreError::Index {
        path: path.to_path_buf(),
        message,
    };
    let file = match name(index.file()).and_then(|entry| root.open_file(entry)) {
        Ok(file) => file,
        Err(error) if absent(&error) => return Ok(None),
        Err(error) => return Err(refused(error.to_string())),
    };
    let status =
        rust_mutants::capdir::file_status(&file).map_err(|error| refused(error.to_string()))?;
    if status.len > MAX_INDEX_BYTES {
        return Err(refused(format!(
            "index exceeds the {MAX_INDEX_BYTES}-byte protocol boundary"
        )));
    }
    let bytes =
        read_regular_artifact(file, status.len).map_err(|error| refused(error.to_string()))?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| refused(error.to_string()))
}

#[cfg(unix)]
fn stored_spellings_at(
    store: &Store,
    runs: &RunsRoot,
) -> Result<BTreeMap<String, (StoredRunId, PathBuf)>, StoreError> {
    let directory = &runs.directory;
    let mut spellings = BTreeMap::new();
    for name in directory_names(directory).map_err(|source| StoreError::NotKept {
        path: store.runs.display().to_string(),
        source,
    })? {
        let path = store.runs.join(&name);
        if name == ".gitignore" {
            let bytes =
                read_named_regular(directory, &name, 2).map_err(|source| StoreError::NotKept {
                    path: path.display().to_string(),
                    source,
                })?;
            if bytes != b"*\n" {
                return Err(StoreError::UnsafePath {
                    path,
                    message: "the private run-store marker has unexpected bytes".to_owned(),
                });
            }
            continue;
        }
        let run = StoredRunId::try_from(name.as_str()).map_err(|error| StoreError::Index {
            path: path.clone(),
            message: format!("an unknown report-store entry is not a run id: {error}"),
        })?;
        open_directory_at(directory, &name).map_err(|source| StoreError::NotKept {
            path: path.display().to_string(),
            source,
        })?;
        remember_unique_spelling(&mut spellings, &run, &path)?;
    }
    Ok(spellings)
}

#[cfg(unix)]
fn read_named_regular(directory: &Dir, entry: &str, expected: u64) -> io::Result<Vec<u8>> {
    read_regular_artifact(directory.open_file(name(entry)?)?, expected)
}

#[cfg(unix)]
fn write_index(authority: &RunDirectory, index: Index, bytes: &[u8]) -> Result<(), StoreError> {
    let store = &authority.store;
    let root = &authority.capability.root;
    let not_kept = |path: String| move |source: io::Error| StoreError::NotKept { path, source };
    let index_path = store.index(index).display().to_string();
    let index_name = name(index.file()).map_err(not_kept(index_path.clone()))?;
    match root
        .status_at(index_name)
        .map_err(not_kept(index_path.clone()))?
    {
        Some(status) if status.kind != Kind::File => {
            return Err(StoreError::UnsafePath {
                path: store.index(index),
                message: "the index target is not one regular file".to_owned(),
            });
        }
        Some(_) | None => {}
    }
    authority
        .capability
        .require_runs_binding()
        .map_err(not_kept(index_path.clone()))?;
    let run_scoped_temporary = format!(".{}.new-{}", index.file(), authority.run_id.as_str());
    let temporary_path = store.root.join(&run_scoped_temporary).display().to_string();
    let temporary_name = name(&run_scoped_temporary).map_err(not_kept(temporary_path.clone()))?;
    let mut file = root
        .create_file(temporary_name)
        .map_err(not_kept(temporary_path.clone()))?;
    file.write_all(bytes)
        .map_err(not_kept(temporary_path.clone()))?;
    file.sync_all().map_err(not_kept(temporary_path))?;
    root.rename_replace(temporary_name, root, index_name)
        .map_err(not_kept(index_path.clone()))?;
    let expected =
        rust_mutants::capdir::file_status(&file).map_err(not_kept(index_path.clone()))?;
    match root.status_at(index_name).map_err(not_kept(index_path))? {
        Some(published) if published.identity == expected.identity => {}
        Some(_) | None => {
            return Err(StoreError::UnsafePath {
                path: store.index(index),
                message: "the index name changed identity during atomic replacement".to_owned(),
            });
        }
    }
    root.sync()
        .map_err(not_kept(store.root.display().to_string()))
}

#[cfg(not(unix))]
#[cfg_attr(
    not(unix),
    expect(
        clippy::missing_const_for_fn,
        reason = "this platform has no capability-rooted backend, so the body is a refusal and the signature is the one the unix backend needs"
    )
)]
fn write_index(_authority: &RunDirectory, _index: Index, _bytes: &[u8]) -> Result<(), StoreError> {
    Err(StoreError::UnsupportedCapability)
}

#[cfg(unix)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pointer {
    schema: String,
    run_id: StoredRunId,
    directory: String,
}

#[cfg(unix)]
fn remember_unique_spelling(
    spellings: &mut BTreeMap<String, (StoredRunId, PathBuf)>,
    run: &StoredRunId,
    path: &Path,
) -> Result<(), StoreError> {
    let folded = run.case_folded();
    if let Some((other, other_path)) = spellings.insert(folded, (run.clone(), path.to_path_buf())) {
        return Err(StoreError::Index {
            path: path.to_path_buf(),
            message: format!(
                "run ids {other:?} at {} and {run:?} differ only by ASCII case",
                other_path.display()
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![cfg_attr(
        unix,
        expect(
            clippy::disallowed_methods,
            reason = "a test asserts what the filesystem says by asking it directly"
        )
    )]
    use super::Store;
    #[cfg(unix)]
    use super::{SealedRunDirectory, publish, publish_with, validate_model_artifact};
    use rust_mutants::id::RunId;
    #[cfg(unix)]
    use std::cell::{Cell, RefCell};
    use std::path::Path;
    #[cfg(unix)]
    use std::rc::Rc;

    fn store(root: &Path) -> Store {
        Store::read(root).expect("default report store")
    }

    #[cfg(unix)]
    fn sealed(directory: super::RunDirectory) -> SealedRunDirectory {
        SealedRunDirectory {
            directory,
            artifacts: Vec::new(),
            files: Vec::new(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn dropping_an_armed_claim_removes_its_private_namespace() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-aaaaaa").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let staging = claimed.path().to_path_buf();
        drop(claimed);
        assert!(matches!(
            std::fs::symlink_metadata(&staging),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        ));
        store
            .claim_writable_run(&run_id)
            .expect("the cleaned identity is claimable again")
            .abort()
            .expect("explicit cleanup");
    }

    #[test]
    fn malformed_configuration_never_selects_the_default_store() {
        let root = tempfile::tempdir().expect("temporary project");
        std::fs::write(root.path().join(crate::config::FILE_NAME), "not = [valid")
            .expect("malformed configuration fixture");
        assert!(
            Store::read(root.path()).is_err(),
            "a configuration failure cannot be reinterpreted as an absent configuration"
        );
    }

    #[cfg(unix)]
    #[test]
    fn configuration_and_claim_share_the_original_workspace_capability() {
        let parent = tempfile::tempdir().expect("temporary parent");
        let project = parent.path().join("project");
        let moved = parent.path().join("original-project");
        std::fs::create_dir_all(&project).expect("project root");
        std::fs::write(
            project.join(crate::config::FILE_NAME),
            "[reports]\ndirectory = \"held-reports\"\n",
        )
        .expect("configuration fixture");
        let workspace = super::WorkspaceRoot::open(&project).expect("held workspace root");
        let loaded = workspace.load_config().expect("held configuration bytes");

        std::fs::rename(&project, &moved).expect("move the configured workspace");
        std::fs::create_dir_all(&project).expect("replacement workspace");
        std::fs::write(
            project.join(crate::config::FILE_NAME),
            "[reports]\ndirectory = \"replacement-reports\"\n",
        )
        .expect("replacement configuration");

        let store = workspace
            .store(&loaded.config.reports.directory)
            .expect("store from the held workspace");
        let run_id = RunId::try_from("20260101t000000z-aaabba").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("claim under the original workspace object");
        assert!(
            moved
                .join("held-reports")
                .join(super::STAGING_NAME)
                .join(run_id.as_str())
                .is_dir()
        );
        assert!(
            !project.join("held-reports").exists() && !project.join("replacement-reports").exists(),
            "neither stale configuration nor a replacement root may redirect the claim"
        );
        claimed.abort().expect("remove the held claim");

        let original_run = "20260101T000000Z-AAABBC";
        std::fs::create_dir_all(
            moved
                .join("held-reports")
                .join(super::RUNS_NAME)
                .join(original_run),
        )
        .expect("original stored run fixture");
        let replacement_runs = project.join("held-reports").join(super::RUNS_NAME);
        std::fs::create_dir_all(&replacement_runs).expect("replacement run store fixture");
        std::fs::write(replacement_runs.join(".gitignore"), "*\n")
            .expect("replacement store marker");
        let replacement_run = replacement_runs.join("20260101T000000Z-AAABBD");
        std::fs::create_dir_all(&replacement_run).expect("replacement sentinel run");

        let retired = store.retain(0).expect("retention through the held store");
        assert_eq!(retired.len(), 1, "only the original store is retired");
        assert!(
            replacement_run.is_dir(),
            "retention cannot reopen the replacement workspace spelling"
        );
    }

    #[cfg(unix)]
    #[test]
    fn readers_and_retention_share_one_bound_report_root() {
        let parent = tempfile::tempdir().expect("temporary parent");
        let project = parent.path().join("project");
        std::fs::create_dir_all(&project).expect("project root");
        let original_id = rust_mutants::id::StoredRunId::try_from("20260101T000000Z-AAABBF")
            .expect("canonical stored run id");
        let configured = crate::config::Config::default().reports.directory;
        let original_root = project.join(configured.as_path());
        let original_runs = original_root.join(super::RUNS_NAME);
        std::fs::create_dir_all(&original_runs).expect("existing report root");
        std::fs::write(original_runs.join(".gitignore"), "*\n").expect("existing report marker");
        let original_run = original_runs.join(original_id.as_str());
        std::fs::create_dir_all(&original_run).expect("original stored run");
        std::fs::write(
            original_run.join(super::DOCUMENT_NAME),
            b"original bound report\n",
        )
        .expect("original document fixture");

        let store = store(&project);

        let moved_root = parent.path().join("bound-report-root");
        std::fs::rename(&store.root, &moved_root).expect("move the bound report root");
        let replacement_runs = store.root.join(super::RUNS_NAME);
        std::fs::create_dir_all(&replacement_runs).expect("replacement report root");
        std::fs::write(replacement_runs.join(".gitignore"), "*\n").expect("replacement marker");
        let replacement_id = "20260101T000000Z-AAABBD";
        let replacement_run = replacement_runs.join(replacement_id);
        std::fs::create_dir_all(&replacement_run).expect("replacement sentinel run");
        std::fs::write(
            replacement_run.join(super::DOCUMENT_NAME),
            b"replacement report\n",
        )
        .expect("replacement sentinel document");

        let opened = store
            .open_run(&original_id)
            .expect("open through the bound report root");
        assert_eq!(
            opened.document().expect("held original document"),
            "original bound report\n"
        );
        let retired = store.retain(0).expect("retire through the bound root");
        assert_eq!(retired.len(), 1, "only the original run is retired");
        assert!(
            replacement_run.is_dir(),
            "a replacement configured root cannot receive authoritative deletion"
        );
        assert!(
            !moved_root
                .join(super::RUNS_NAME)
                .join(original_id.as_str())
                .exists(),
            "the run selected through the bound root is retired"
        );
    }

    #[cfg(unix)]
    #[test]
    fn explicit_and_indexed_readers_keep_the_selected_run_directory_open() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-aaabbb").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let original = b"original held report\n";
        claimed
            .write_new(super::DOCUMENT_NAME, original)
            .expect("held document fixture");
        claimed.sync_tree().expect("durable fixture bytes");
        let sealed = SealedRunDirectory {
            directory: claimed,
            artifacts: Vec::new(),
            files: vec![super::PublicationFile {
                name: super::DOCUMENT_NAME,
                bytes: original.to_vec(),
            }],
        };
        publish(sealed, |_| super::IndexPublication::NotRequired).expect("published fixture");
        let stored = rust_mutants::id::StoredRunId::from(&run_id);
        let pointer = serde_json::json!({
            "schema": crate::report::SCHEMA,
            "run_id": run_id.as_str(),
            "directory": store.named_exact(&stored),
        });
        std::fs::write(store.index(super::Index::Any), pointer.to_string()).expect("index fixture");

        let explicit = store.open_run(&stored).expect("explicit held run");
        let indexed = store
            .pointed_run(super::Index::Any)
            .expect("strict index")
            .expect("indexed held run");
        let canonical = store.run(&stored);
        let moved = store.root.join("moved-selected-run");
        std::fs::rename(&canonical, &moved).expect("move selected run");
        std::fs::create_dir_all(&canonical).expect("replacement run directory");
        std::fs::write(canonical.join(super::DOCUMENT_NAME), b"replacement\n")
            .expect("replacement document");

        assert_eq!(
            explicit.document().expect("explicit held bytes"),
            "original held report\n"
        );
        assert_eq!(
            indexed.document().expect("indexed held bytes"),
            "original held report\n"
        );
    }

    #[cfg(not(unix))]
    #[test]
    fn authority_bearing_store_operations_are_typed_refusals_without_a_backend() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-aaaaab").expect("canonical run id");

        assert!(matches!(
            store.claim_writable_run(&run_id),
            Err(super::StoreError::UnsupportedCapability)
        ));
        assert!(matches!(
            store.pointed_at(super::Index::Any),
            Err(super::StoreError::UnsupportedCapability)
        ));
        assert!(matches!(
            super::retain_with_capability(&store, 1),
            Err(super::StoreError::UnsupportedCapability)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn tree_sync_refuses_a_symlink_instead_of_following_it() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temporary project");
        let outside = tempfile::tempdir().expect("outside directory");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-bbbbbb").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        symlink(outside.path(), claimed.path().join("escaped")).expect("adversarial link");
        assert!(claimed.sync_tree().is_err());
        claimed.abort().expect("remove the refused tree");
    }

    #[cfg(unix)]
    #[test]
    fn model_artifact_is_rehashed_from_the_held_directory_before_publication() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-bbbbbb").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let model = claimed.path().join("model");
        std::fs::create_dir_all(&model).expect("fresh model directory");
        let mutant = "a".repeat(64);
        let path = model.join(format!("{mutant}.json"));
        let original = b"proof";
        std::fs::write(&path, original).expect("retained artifact");
        let artifact = crate::report::ModelArtifact::checked(
            &format!("model/{mutant}.json"),
            u64::try_from(original.len()).expect("tiny fixture"),
            rust_mutants::id::digest(original),
        )
        .expect("canonical artifact evidence");
        validate_model_artifact(&claimed, &artifact).expect("the retained bytes match");

        std::fs::write(&path, b"forge").expect("same-sized adversarial replacement");
        assert!(
            validate_model_artifact(&claimed, &artifact).is_err(),
            "bytes changed after the proof cannot be published under the old digest"
        );
        claimed.abort().expect("remove the refused tree");
    }

    #[cfg(unix)]
    #[test]
    fn an_artifact_changed_after_sealing_is_removed_instead_of_published() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-bcbcbc").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let model = claimed.path().join("model");
        std::fs::create_dir_all(&model).expect("fresh model directory");
        let mutant = "b".repeat(64);
        let path = model.join(format!("{mutant}.json"));
        let original = b"proof";
        std::fs::write(&path, original).expect("retained artifact");
        let artifact = crate::report::ModelArtifact::checked(
            &format!("model/{mutant}.json"),
            u64::try_from(original.len()).expect("tiny fixture"),
            rust_mutants::id::digest(original),
        )
        .expect("canonical artifact evidence");
        validate_model_artifact(&claimed, &artifact).expect("the retained bytes match");
        claimed.sync_tree().expect("the staged tree is durable");
        let sealed = SealedRunDirectory {
            directory: claimed,
            artifacts: vec![artifact],
            files: Vec::new(),
        };

        std::fs::write(&path, b"forge").expect("same-sized post-seal replacement");
        assert!(
            publish(sealed, |_| super::IndexPublication::NotRequired).is_err(),
            "the held artifact ledger must be rechecked after the publication rename"
        );
        assert!(matches!(
            std::fs::symlink_metadata(store.writable_run(&run_id)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        ));
    }

    #[cfg(unix)]
    #[test]
    fn an_unretained_file_cannot_cross_the_sealed_publication_boundary() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-bdbdbd").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        claimed
            .write_new("unretained", b"foreign")
            .expect("adversarial extra file");
        assert!(
            publish(sealed(claimed), |_| super::IndexPublication::NotRequired).is_err(),
            "the sealed tree must equal its complete retained-file ledger"
        );
        assert!(matches!(
            std::fs::symlink_metadata(store.writable_run(&run_id)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_held_report_root_prevents_mixed_parent_capabilities() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let initializing = RunId::try_from("20260101t000000z-cacaca").expect("canonical run id");
        store
            .claim_writable_run(&initializing)
            .expect("initialize the report store")
            .abort()
            .expect("remove the initializing claim");
        let root_capability = super::open_directory(&store.root).expect("held report root");
        let moved_root = store.root.with_file_name("reports-held");
        std::fs::rename(&store.root, &moved_root).expect("move the visible report root");
        std::fs::create_dir_all(&store.root).expect("replacement report root");
        std::fs::create_dir_all(store.root.join(super::RUNS_NAME)).expect("replacement runs");
        std::fs::create_dir_all(store.root.join(super::STAGING_NAME)).expect("replacement staging");
        let run_id = RunId::try_from("20260101t000000z-cbcbcb").expect("canonical run id");
        let capability = super::RunCapability::claim_beneath(&root_capability, run_id.as_str())
            .expect("claim through the held root");
        assert!(
            moved_root
                .join(super::STAGING_NAME)
                .join(run_id.as_str())
                .is_dir()
        );
        assert!(
            !store
                .root
                .join(super::STAGING_NAME)
                .join(run_id.as_str())
                .exists()
        );
        capability
            .remove_entry(false, run_id.as_str())
            .expect("remove the held claim");
    }

    #[cfg(unix)]
    #[test]
    fn a_root_capability_clone_failure_creates_no_store_namespace() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        std::fs::create_dir_all(&store.root).expect("configured root fixture");
        let held = super::open_directory(&store.root).expect("held configured root");
        let run_id = RunId::try_from("20260101t000000z-cbcbcc").expect("canonical run id");
        let failed = super::RunCapability::claim_beneath_with_root(
            &held,
            Err(std::io::Error::other("injected descriptor exhaustion")),
            run_id.as_str(),
        );

        assert!(failed.is_err(), "the root capability failure is preserved");
        assert!(
            !store.root.join(super::STAGING_NAME).exists(),
            "no staging parent is created before every capability is retained"
        );
        assert!(
            !store.root.join(super::RUNS_NAME).exists(),
            "no publication parent is created before every capability is retained"
        );
    }

    #[cfg(unix)]
    #[test]
    fn derived_indexes_use_the_same_held_report_root_as_publication() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-ccccca").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let moved_root = store.root.with_file_name("reports-index-held");
        std::fs::rename(&store.root, &moved_root).expect("move the visible report root");
        std::fs::create_dir_all(&store.root).expect("replacement report root");

        super::write_index(&claimed, super::Index::Any, b"held-root\n")
            .expect("write through the retained root capability");
        assert_eq!(
            std::fs::read(moved_root.join(super::Index::Any.file()))
                .expect("held root receives the index"),
            b"held-root\n"
        );
        assert!(
            !store.index(super::Index::Any).exists(),
            "the replacement root must not receive derived index bytes"
        );
        claimed.abort().expect("remove the held run claim");
    }

    #[cfg(unix)]
    #[test]
    fn an_abandoned_index_temporary_cannot_block_a_later_run() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let abandoned = RunId::try_from("20260101t000000z-ccccab").expect("canonical run id");
        store
            .claim_writable_run(&abandoned)
            .expect("initialize the held report root")
            .abort()
            .expect("remove the first run claim");
        let orphan = store.root.join(format!(
            ".{}.new-{}",
            super::Index::Any.file(),
            abandoned.as_str()
        ));
        std::fs::write(&orphan, b"incomplete").expect("simulated killed index writer");

        let successor = RunId::try_from("20260101t000000z-ccccac").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&successor)
            .expect("a later run has an independent namespace");
        super::write_index(&claimed, super::Index::Any, b"successor\n")
            .expect("the abandoned run cannot block this index update");
        assert_eq!(
            std::fs::read(store.index(super::Index::Any)).expect("updated index"),
            b"successor\n"
        );
        assert_eq!(
            std::fs::read(orphan).expect("the hostile orphan remains isolated"),
            b"incomplete"
        );
        claimed.abort().expect("remove the successor claim");
    }

    #[cfg(unix)]
    #[test]
    fn configured_report_root_cannot_escape_the_workspace_capability() {
        let root = tempfile::tempdir().expect("temporary parent");
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).expect("project root");
        crate::config::ReportDirectory::try_from("../escaped-reports")
            .expect_err("a non-canonical configured path");
        assert!(
            !root.path().join("escaped-reports").exists(),
            "a non-canonical configured path must not create outside the workspace capability"
        );
    }

    #[cfg(unix)]
    #[test]
    fn index_reader_uses_one_nofollow_root_capability() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let initializing = RunId::try_from("20260101t000000z-cccdca").expect("canonical run id");
        store
            .claim_writable_run(&initializing)
            .expect("initialize the capability-rooted store")
            .abort()
            .expect("remove the initializing claim");
        let outside = root.path().join("outside-index.json");
        std::fs::write(&outside, b"{}\n").expect("external index target");
        symlink(&outside, store.index(super::Index::Any)).expect("adversarial index symlink");

        assert!(
            store.pointed_at(super::Index::Any).is_err(),
            "an index path must never be followed outside the held store root"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_removes_a_nested_tree_through_quarantined_entries() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-cccdcb").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let nested = claimed.path().join("nested");
        std::fs::create_dir_all(&nested).expect("nested directory");
        std::fs::write(nested.join("artifact"), b"retained").expect("nested artifact");
        let staging = claimed.path().to_path_buf();

        claimed
            .abort()
            .expect("quarantine and remove every held child entry");
        assert!(
            !staging.exists(),
            "cleanup must remove the owned root only after its exact child tree"
        );
    }

    #[cfg(unix)]
    #[test]
    fn prepopulated_quarantine_names_cannot_block_owned_cleanup() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-cccdcc").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let staging_parent = claimed
            .path()
            .parent()
            .expect("staging parent")
            .to_path_buf();
        std::fs::create_dir_all(staging_parent.join(".njutest-cleanup-0000000000000000"))
            .expect("hostile outer quarantine collision");
        std::fs::write(
            claimed.path().join(".njutest-remove-0000000000000000"),
            b"hostile",
        )
        .expect("hostile child quarantine collision");
        std::fs::write(claimed.path().join("artifact"), b"owned").expect("ordinary owned entry");
        let staging = claimed.path().to_path_buf();

        claimed
            .abort()
            .expect("exclusive quarantine selection skips hostile names");
        assert!(!staging.exists(), "the complete owned tree is removed");
        assert!(
            staging_parent
                .join(".njutest-cleanup-0000000000000000")
                .is_dir(),
            "cleanup never takes ownership of a pre-existing sibling"
        );
    }

    #[cfg(unix)]
    #[test]
    fn quarantine_collision_work_is_bounded_and_preserves_the_original() {
        let root = tempfile::tempdir().expect("temporary cleanup namespace");
        let held = super::open_directory(root.path()).expect("held cleanup namespace");
        std::fs::write(root.path().join("owned"), b"owned").expect("owned fixture");
        let tokens = (0_u8..8)
            .map(|byte| [byte; 16])
            .collect::<std::collections::VecDeque<_>>();
        for token in &tokens {
            std::fs::write(
                root.path()
                    .join(format!(".njutest-remove-{}", hex::encode(token))),
                b"foreign",
            )
            .expect("hostile quarantine collision");
        }
        let mut hostile_tokens = tokens;
        let result = super::quarantine_entry_with(&held, "owned", ".njutest-remove", || {
            hostile_tokens
                .pop_front()
                .ok_or_else(|| std::io::Error::other("the bounded collision fixture was exhausted"))
        });

        assert!(
            matches!(result, Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists),
            "a full bounded collision set is a typed refusal"
        );
        assert_eq!(
            std::fs::read(root.path().join("owned")).expect("original survives refusal"),
            b"owned"
        );
    }

    #[cfg(unix)]
    #[test]
    fn prepopulated_retention_quarantine_cannot_block_removal() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let initializing = RunId::try_from("20260101t000000z-cccdcd").expect("canonical run id");
        store
            .claim_writable_run(&initializing)
            .expect("initialize the report store")
            .abort()
            .expect("remove the initializing claim");
        let stored = "20260101t000000z-abcdef";
        let stored_path = store.runs().join(stored);
        std::fs::create_dir_all(&stored_path).expect("stored run fixture");
        let collision = store.runs().join(".njutest-retention-0000000000000000");
        std::fs::create_dir_all(&collision).expect("hostile retention quarantine collision");
        let runs = super::open_directory(store.runs()).expect("held runs namespace");

        let taken = super::quarantine_named_entry(&runs, stored, ".njutest-retention")
            .expect("exclusive quarantine selection skips the hostile name");
        assert_ne!(taken, ".njutest-retention-0000000000000000");
        assert!(
            store.runs().join(&taken).is_dir(),
            "the selected quarantine name is the one that was taken"
        );
        assert!(!stored_path.exists(), "the original name is moved away");
        assert!(collision.is_dir(), "the foreign collision is never taken");
    }

    #[cfg(unix)]
    #[test]
    fn a_preexisting_casefold_alias_refuses_publication_before_rename() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-cdcacb").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let alias = store.runs().join(run_id.as_str().to_ascii_uppercase());
        std::fs::create_dir_all(&alias).expect("casefold alias fixture");
        let filesystem_aliases_case = store.writable_run(&run_id).exists();

        assert!(
            publish(sealed(claimed), |_| super::IndexPublication::NotRequired).is_err(),
            "the capability-rooted census is repeated immediately before rename"
        );
        assert!(alias.is_dir(), "the alias was never owned by the publisher");
        if !filesystem_aliases_case {
            assert!(
                !store.writable_run(&run_id).exists(),
                "the refused exact spelling remains unpublished"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_casefold_alias_inserted_during_publish_aborts_the_owned_run() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-cdcaca").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let case_probe = store.runs().join("njutest-case-probe");
        std::fs::create_dir_all(&case_probe).expect("case-sensitivity probe");
        let case_insensitive = store.runs().join("NJUTEST-CASE-PROBE").exists();
        std::fs::remove_dir(&case_probe).expect("remove case-sensitivity probe");
        if case_insensitive {
            claimed.abort().expect("remove the unused claim");
            return;
        }
        let alias = store.runs().join(run_id.as_str().to_ascii_uppercase());
        let result = publish_with(
            sealed(claimed),
            |claim, _from, _to| {
                claim.publish_entry()?;
                std::fs::create_dir_all(&alias)
            },
            |claim, directory| {
                let published = directory == claim.store.runs();
                claim.sync_rename_parent(published)
            },
            |_| super::IndexPublication::NotRequired,
        );

        assert!(
            result.is_err(),
            "a casefold alias inserted after rename cannot cross commit"
        );
        assert!(
            alias.is_dir(),
            "the foreign alias is never treated as owned"
        );
        assert!(
            !store.writable_run(&run_id).exists(),
            "the owned exact spelling is removed after the refusal"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_never_unlinks_a_replacement_for_the_held_directory() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-cdcdcd").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let parent = claimed.path().parent().expect("staging parent");
        let moved = parent.join("held-elsewhere");
        std::fs::rename(claimed.path(), &moved).expect("move the held directory");
        std::fs::create_dir_all(claimed.path()).expect("replacement directory");

        assert!(
            claimed
                .capability
                .remove_entry(false, run_id.as_str())
                .is_err(),
            "the quarantined spelling must match the held inode before deletion"
        );
        assert!(claimed.path().is_dir(), "the replacement must be restored");
        assert!(
            moved.is_dir(),
            "the held directory must not be misreported as removed"
        );

        std::fs::remove_dir(claimed.path()).expect("remove the replacement");
        std::fs::rename(&moved, claimed.path()).expect("restore the held directory spelling");
        claimed.abort().expect("remove the restored held directory");
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_cleanup_keeps_the_claim_armed_for_drop() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-cecece").expect("canonical run id");
        let mut claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let failure = super::StoreError::UnsafePath {
            path: claimed.path().to_path_buf(),
            message: "injected cleanup refusal".to_owned(),
        };
        assert!(claimed.finish_cleanup(Err(failure)).is_err());
        assert_eq!(claimed.ownership, super::RunOwnership::Staging);
        claimed
            .abort()
            .expect("a later cleanup can still disarm the claim");
    }

    #[cfg(unix)]
    #[test]
    fn ancestor_replacement_cannot_redirect_held_writes_or_publication() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-ababab").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let staging_parent = claimed
            .path()
            .parent()
            .expect("staging leaf has a parent")
            .to_path_buf();
        let moved_parent = staging_parent.with_file_name(".pending-runs-held");
        std::fs::rename(&staging_parent, &moved_parent).expect("move the visible ancestor");
        std::fs::create_dir_all(&staging_parent).expect("adversarial replacement parent");
        let replacement_leaf = staging_parent.join(run_id.as_str());
        std::fs::create_dir_all(&replacement_leaf).expect("adversarial replacement leaf");

        claimed
            .write_new("probe", b"held")
            .expect("write through the held directory descriptor");
        assert_eq!(
            std::fs::read(moved_parent.join(run_id.as_str()).join("probe"))
                .expect("the held object received the write"),
            b"held"
        );
        assert!(
            !replacement_leaf.join("probe").exists(),
            "the replacement spelling must not receive publication bytes"
        );

        let (published, indexes) = publish(
            SealedRunDirectory {
                directory: claimed,
                artifacts: Vec::new(),
                files: vec![super::PublicationFile {
                    name: "probe",
                    bytes: b"held".to_vec(),
                }],
            },
            |_| super::IndexPublication::NotRequired,
        )
        .expect("capability-relative publication");
        assert_eq!(published, store.writable_run(&run_id));
        assert!(matches!(indexes, super::IndexPublication::NotRequired));
        assert_eq!(
            std::fs::read(published.join("probe")).expect("published held bytes"),
            b"held"
        );
    }

    #[cfg(unix)]
    #[test]
    fn publication_syncs_both_rename_parents_before_disarming_the_claim() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-cccccc").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let events = Rc::new(RefCell::new(Vec::new()));
        let rename_events = Rc::clone(&events);
        let sync_events = Rc::clone(&events);
        let published = publish_with(
            sealed(claimed),
            |claim, _from, _to| {
                rename_events.borrow_mut().push("rename".to_owned());
                claim.publish_entry()
            },
            |_claim, directory| {
                let name = match directory.file_name().and_then(std::ffi::OsStr::to_str) {
                    Some(name) => name,
                    None => "<root>",
                };
                sync_events.borrow_mut().push(format!("sync:{name}"));
                std::fs::File::open(directory)?.sync_all()
            },
            |_| super::IndexPublication::NotRequired,
        )
        .expect("durable publication");
        assert_eq!(
            events.borrow().as_slice(),
            ["rename", "sync:runs", "sync:.pending-runs"],
            "the destination entry and source removal are not durable until both rename parents sync"
        );
        assert_eq!(published.0, store.writable_run(&run_id));
        assert!(matches!(published.1, super::IndexPublication::NotRequired));
    }

    #[cfg(unix)]
    #[test]
    fn post_index_integrity_failure_never_deletes_the_indexed_run() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-cccdde").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let (published, indexes) = publish_with(
            sealed(claimed),
            |claim, _from, _to| claim.publish_entry(),
            |claim, directory| {
                let published = directory == claim.store.runs();
                claim.sync_rename_parent(published)
            },
            |claim| {
                super::point(claim, super::Index::Any, &run_id)
                    .expect("publish the derived pointer first");
                std::fs::write(claim.published.join("late-change"), b"changed")
                    .expect("adversarial post-index tree change");
                super::IndexPublication::Complete
            },
        )
        .expect("the durable canonical publication is not rewound");

        assert!(matches!(
            indexes,
            super::IndexPublication::RunChanged { .. }
        ));
        assert!(
            published.is_dir(),
            "a pointer must never be left naming a run removed during error unwinding"
        );
        assert_eq!(
            store
                .pointed_at(super::Index::Any)
                .expect("the strict pointer remains readable"),
            Some(rust_mutants::id::StoredRunId::from(&run_id))
        );
    }

    #[cfg(unix)]
    #[test]
    fn retention_over_a_replaced_namespace_without_the_indexed_run_is_a_typed_refusal() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-eeef04").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        publish(sealed(claimed), |_| super::IndexPublication::NotRequired)
            .expect("published fixture");
        let stored = rust_mutants::id::StoredRunId::from(&run_id);
        std::fs::write(
            store.index(super::Index::Any),
            serde_json::json!({
                "schema": crate::report::SCHEMA,
                "run_id": run_id.as_str(),
                "directory": store.named_exact(&stored),
            })
            .to_string(),
        )
        .expect("index fixture");
        let moved = store.root.join("moved-runs-namespace");
        std::fs::rename(store.runs(), &moved).expect("move the runs namespace aside");
        std::fs::create_dir_all(store.runs()).expect("replacement runs namespace");
        std::fs::write(store.runs().join(".gitignore"), "*\n").expect("ignore marker");

        let refused = store.retain(0);
        assert!(
            refused.is_err(),
            "a namespace the index does not resolve in is never silently collected from: \
             the protection census and the deletions share one capability, so the answer \
             is the refusal the index reader would get: {refused:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_index_reader_never_answers_from_a_runs_namespace_replaced_after_validation() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-eeef01").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let original = b"original held report\n";
        claimed
            .write_new(super::DOCUMENT_NAME, original)
            .expect("held document fixture");
        claimed.sync_tree().expect("durable fixture bytes");
        publish(
            SealedRunDirectory {
                directory: claimed,
                artifacts: Vec::new(),
                files: vec![super::PublicationFile {
                    name: super::DOCUMENT_NAME,
                    bytes: original.to_vec(),
                }],
            },
            |_| super::IndexPublication::NotRequired,
        )
        .expect("published fixture");
        let stored = rust_mutants::id::StoredRunId::from(&run_id);
        std::fs::write(
            store.index(super::Index::Any),
            serde_json::json!({
                "schema": crate::report::SCHEMA,
                "run_id": run_id.as_str(),
                "directory": store.named_exact(&stored),
            })
            .to_string(),
        )
        .expect("index fixture");

        let indexed = store
            .pointed_run(super::Index::Any)
            .expect("strict index")
            .expect("indexed held run");
        let moved = store.root.join("moved-runs-namespace");
        std::fs::rename(store.runs(), &moved).expect("move the runs namespace aside");
        std::fs::create_dir_all(store.runs()).expect("replacement runs namespace");
        std::fs::write(store.runs().join(".gitignore"), "*\n").expect("ignore marker");
        let attacker = store.runs().join(indexed.id.as_str());
        std::fs::create_dir_all(&attacker).expect("attacker run directory");
        std::fs::write(attacker.join(super::DOCUMENT_NAME), b"replacement bytes\n")
            .expect("attacker document");

        assert_eq!(
            indexed.document().expect("held document bytes"),
            "original held report\n",
            "the census and the opening share one held runs capability, so a namespace \
             replaced after the answer is held is never answered from: replacement bytes \
             are never an answer a reader may be handed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_runs_namespace_replaced_during_publication_refuses_rather_than_pointing_nowhere() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-eeef02").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let moved = store.root.join("moved-runs-namespace");
        let result = publish_with(
            sealed(claimed),
            |claim, _from, _to| claim.publish_entry(),
            |claim, directory| {
                let published = directory == claim.store.runs();
                if published {
                    std::fs::rename(claim.store.runs(), &moved).expect("move runs aside");
                    std::fs::create_dir_all(claim.store.runs()).expect("replacement runs");
                    std::fs::write(claim.store.runs().join(".gitignore"), "*\n")
                        .expect("ignore marker");
                }
                claim.sync_rename_parent(published)
            },
            |_| super::IndexPublication::Complete,
        );

        assert!(
            result.is_err(),
            "a publication into a runs namespace the held report root no longer names is \
             never committed: an index beside it would point readers at a replacement \
             nobody vouched for: {result:?}"
        );
        assert!(
            !store.index(super::Index::Any).exists(),
            "no durable pointer is written for a refused publication"
        );
        assert!(
            !store.runs().join(run_id.as_str()).exists(),
            "the replacement namespace stays untouched by the refusal"
        );
        assert!(
            !moved.join(run_id.as_str()).exists(),
            "and the refused publication cleans its own run out of the detached parent \
             rather than leaving it behind: {result:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_runs_namespace_replaced_while_indexes_were_written_is_reported_as_changed() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-eeef03").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let moved = store.root.join("moved-runs-namespace");
        let committed = publish_with(
            sealed(claimed),
            |claim, _from, _to| claim.publish_entry(),
            |claim, directory| {
                let published = directory == claim.store.runs();
                claim.sync_rename_parent(published)
            },
            |claim| {
                super::point(claim, super::Index::Any, &run_id)
                    .expect("publish the derived pointer first");
                std::fs::rename(claim.store.runs(), &moved).expect("move runs aside");
                std::fs::create_dir_all(claim.store.runs()).expect("replacement runs");
                std::fs::write(claim.store.runs().join(".gitignore"), "*\n")
                    .expect("ignore marker");
                super::IndexPublication::Complete
            },
        )
        .expect("the canonical run is never deleted to unwind an index");
        let (_, indexes) = committed;

        assert!(
            matches!(indexes, super::IndexPublication::RunChanged { .. }),
            "the publication already committed, so the swap is reported rather than \
             rewound: {indexes:?}"
        );
        assert!(
            moved.join(run_id.as_str()).is_dir(),
            "the committed run survives in the namespace it was published into"
        );
    }

    #[cfg(unix)]
    #[test]
    fn failure_syncing_the_source_parent_aborts_the_published_authority() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-dddddd").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("fresh unpublished namespace");
        let calls = Cell::new(0_u8);
        let failed = publish_with(
            sealed(claimed),
            |claim, _from, _to| claim.publish_entry(),
            |_claim, directory| {
                let next = calls
                    .get()
                    .checked_add(1)
                    .expect("the two-parent sync count is representable");
                calls.set(next);
                if next == 2 {
                    Err(std::io::Error::other("injected source-parent sync failure"))
                } else {
                    std::fs::File::open(directory)?.sync_all()
                }
            },
            |_| super::IndexPublication::NotRequired,
        );
        assert!(
            failed.is_err(),
            "an unsynced source removal is not published"
        );
        assert!(matches!(
            std::fs::symlink_metadata(store.writable_run(&run_id)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        ));
        store
            .claim_writable_run(&run_id)
            .expect("the aborted identity is claimable again")
            .abort()
            .expect("remove the second claim");
    }

    #[cfg(unix)]
    #[test]
    fn same_owner_legacy_namespaces_are_tightened_before_claiming() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let root = tempfile::tempdir().expect("temporary project");
        let report_root = root.path().join(crate::config::DEFAULT_REPORTS_DIRECTORY);
        let staging = report_root.join(super::STAGING_NAME);
        let runs = report_root.join(super::RUNS_NAME);
        std::fs::create_dir_all(&staging).expect("legacy staging namespace");
        std::fs::create_dir_all(&runs).expect("legacy runs namespace");
        for directory in [&staging, &runs] {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755))
                .expect("legacy directory mode");
        }

        let store = store(root.path());
        let run_id = RunId::try_from("20260101t000000z-aaaaac").expect("canonical run id");
        let claimed = store
            .claim_writable_run(&run_id)
            .expect("same-owner legacy namespaces are migrated through held descriptors");
        for directory in [&staging, &runs] {
            assert_eq!(
                std::fs::metadata(directory)
                    .expect("tightened namespace")
                    .mode()
                    & 0o7777,
                0o700
            );
        }
        claimed.abort().expect("remove the migrated claim");
    }

    #[cfg(unix)]
    #[test]
    fn a_preexisting_staging_name_is_not_adopted_as_a_fresh_claim() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let initialize = RunId::try_from("20260101t000000z-aaaaad").expect("canonical run id");
        store
            .claim_writable_run(&initialize)
            .expect("initialize private namespaces")
            .abort()
            .expect("remove the initializer");
        let run_id = RunId::try_from("20260101t000000z-aaaaae").expect("canonical run id");
        let occupied = store.root.join(super::STAGING_NAME).join(run_id.as_str());
        std::fs::create_dir_all(&occupied).expect("pre-existing hostile staging name");

        assert!(
            store.claim_writable_run(&run_id).is_err(),
            "the random private claim is atomically renamed with no replacement"
        );
        assert!(
            occupied.is_dir(),
            "the unowned directory is never cleaned up"
        );
        for entry in std::fs::read_dir(store.root.join(super::STAGING_NAME))
            .expect("private staging inventory")
        {
            let name = entry
                .expect("staging entry")
                .file_name()
                .into_string()
                .expect("the private fixture has exact UTF-8 names");
            assert!(
                !name.starts_with(".njutest-claim-"),
                "a refused atomic claim leaves no random temporary namespace"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_unknown_entry_makes_the_run_store_corrupt_instead_of_invisible() {
        let root = tempfile::tempdir().expect("temporary project");
        let store = store(root.path());
        let first = RunId::try_from("20260101t000000z-eeeeee").expect("canonical run id");
        store
            .claim_writable_run(&first)
            .expect("initialize the private store")
            .abort()
            .expect("remove the initializing claim");
        std::fs::write(store.runs().join("not-a-run"), b"foreign")
            .expect("adversarial foreign entry");
        let second = RunId::try_from("20260101t000000z-ffffff").expect("canonical run id");
        assert!(
            store.claim_writable_run(&second).is_err(),
            "an unrecognized entry cannot disappear from the case-fold collision census"
        );
    }
}
