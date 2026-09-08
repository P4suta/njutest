// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The public entry point: a read-only source tree, copied.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cargo::{Driver, LocateOptions, Metadata, MetadataOptions, Toolchain};
use crate::error::{self, ErrorCode};
use crate::glob::Pattern;
use crate::runner::Cancel;
use crate::session::{PrepareOptions, Session, prepare};
use crate::snapshot::{self, DIR_PREFIX, Options as SnapshotOptions, Snapshot};
use crate::tempowner::{self, SweepResult};
use crate::trace::{OpenRecord, Recorder, SnapshotRecord, SweepRecord};

/// Where the engine puts the target directory of a run, under the temporary root: one per source root, so successive runs share cargo's incremental state.
pub const TARGET_DIR_PREFIX: &str = "rust-mutants-target-";

/// The schema a target directory's owner marker names, so a reader can tell a build cache from a run's scratch tree.
pub const TARGET_OWNER_SCHEMA: &str = "rust-mutants-target-owner-v1";

/// Configures [`Workspace::open`].
#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    /// The cargo to use: a path, or a bare name to find on `search_path`.
    pub cargo: Option<PathBuf>,
    /// The `PATH` a bare cargo name is searched on. The composition root reads the process environment; the engine never does.
    pub search_path: Option<OsString>,
    /// The complete environment every command and test process runs with.
    pub env: Vec<(OsString, OsString)>,
    /// The absolute directory snapshots and target directories are created in. An argument for the same reason `env` is; the composition root is where the operating system's temporary directory is named.
    pub temp_directory: PathBuf,
    /// The configured report directory as a source-root-relative path, so the snapshot excludes it.
    pub report_directory: Option<String>,
    /// Patterns removing paths from the snapshot entirely.
    pub exclude: Vec<Pattern>,
    /// Preserve the snapshot and the target directory instead of removing them, for a person who has to look at what a run produced.
    pub keep_temp: bool,
    /// Pass `--offline` to every cargo command.
    pub offline: bool,
    /// Pass `--locked` to every cargo command.
    pub locked: bool,
    /// Directories outside the root the workspace may read code from, each copied beside the tree.
    ///
    /// A run measures a copy, so a path dependency outside the root is not in
    /// it. Naming one here says the run may copy it too, which makes the
    /// measurement about a tree that is not the one on disk: it is a decision
    /// for a person to make rather than one a run takes silently.
    pub allow_outside: Vec<PathBuf>,
    /// Where the run records what it did. [`Recorder::disabled`] by default.
    pub trace: Recorder,
}

/// Claims the build cache for the life of this workspace, so a sweep elsewhere leaves it alone while cargo is writing into it. A cache that cannot be claimed is one another run is already using, which is not this run's business and not a reason to fail: cargo takes its own lock.
fn claim_target(dir: &Path, now: jiff::Timestamp) -> Option<tempowner::Owner> {
    std::fs::create_dir_all(dir).ok()?;
    tempowner::claim_cache(dir, now, TARGET_OWNER_SCHEMA).ok()
}

/// A read-only source tree and the disposable copy of it this run works in.
#[derive(Debug)]
pub struct Workspace {
    pub(crate) snapshot: Snapshot,
    pub(crate) toolchain: Toolchain,
    pub(crate) metadata: Metadata,
    pub(crate) target_dir: PathBuf,
    /// The claim on that directory: held for the life of the workspace so a concurrent sweep leaves it alone, and released without removing anything.
    pub(crate) target_owner: Option<tempowner::Owner>,
    pub(crate) base_env: Vec<(OsString, OsString)>,
    pub(crate) swept: SweepResult,
    pub(crate) keep_temp: bool,
    pub(crate) offline: bool,
    pub(crate) locked: bool,
    pub(crate) trace: Recorder,
}

/// Why the workspace layer could not do what it was asked.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
    /// The tree does not compile before anything was instrumented.
    #[error("{}: the workspace does not compile before anything is instrumented: {first}", error::SESSION_PRISTINE_BROKEN.code)]
    PristineBroken {
        /// The first error the compiler reported, rendered.
        first: String,
    },
    /// The workspace reads code from outside itself, which the copy does not hold.
    #[error(
        "{}: {name} is read from {}, which is outside {} and so is not in the copy a run \
         measures. {} declares it. Allow it with --allow-outside, or vendor it inside the tree",
        error::WORKSPACE_REACHES_OUTSIDE.code,
        path.display(),
        root.display(),
        manifest.display()
    )]
    ReachesOutside {
        /// The dependency or patched crate.
        name: String,
        /// The manifest that declares it.
        manifest: PathBuf,
        /// Where it reads it from.
        path: PathBuf,
        /// The tree the run was given.
        root: PathBuf,
    },
    /// The root names a member of a workspace rather than the workspace.
    #[error(
        "{}: {} is a member of the workspace at {}, and a member on its own is not a tree a \
         run can build. Pass --root {}",
        error::ROOT_IS_NOT_THE_WORKSPACE.code,
        root.display(),
        workspace_root.display(),
        workspace_root.display()
    )]
    RootIsNotTheWorkspace {
        /// The tree the run was given.
        root: PathBuf,
        /// The workspace it belongs to.
        workspace_root: PathBuf,
    },
    /// The instrumented baseline does not pass its own tests.
    #[error(
        "{}: {} fails with nothing active, so no outcome under a mutation would be about the \
         mutation. Fix the test, leave the target out with {}, or pass --no-verify and read \
         every result as being about the instrumentation as much as about the mutation:\n{output}",
        error::SESSION_VERIFY_FAILED.code,
        targets.join(", "),
        targets
            .iter()
            .map(|target| format!("--skip-target {target}"))
            .collect::<Vec<String>>()
            .join(" ")
    )]
    VerifyFailed {
        /// Every target that failed, in identity order.
        ///
        /// A run does not stop at the first: somebody reading this is about to
        /// fix what it names, and a refusal that names one of five sends them
        /// round the loop five times.
        targets: Vec<String>,
        /// The tail of what the first of them said.
        output: String,
    },
    /// No prefix of the catalog matches, or several do.
    #[error("{}: {message}", error::SESSION_UNKNOWN_MUTANT.code)]
    UnknownMutant {
        /// What was wrong with the prefix.
        message: String,
    },
    /// The named target is not one this session built.
    #[error(
        "{}: no test target is named {name:?}; this session built {}",
        error::SESSION_UNKNOWN_TARGET.code,
        if available.is_empty() { String::from("none") } else { available.join(", ") }
    )]
    UnknownTarget {
        /// The name that was asked for.
        name: String,
        /// The targets there are, which is what a reader has to choose between.
        available: Vec<String>,
    },
    /// The workspace has no test target at all, so nothing can be measured.
    #[error(
        "{}: the workspace builds no test target, so no mutant can be measured; {} {} selected",
        error::SESSION_NO_TARGETS.code,
        if packages.is_empty() { String::from("every package was") } else { packages.join(", ") },
        if packages.len() == 1 { "was" } else { "were" }
    )]
    NoTargets {
        /// The packages the run was about, which is where a reader looks for a test to write.
        packages: Vec<String>,
    },
    /// The instrumented tree could not be written.
    #[error("{}: cannot write {path} into the snapshot: {source}", error::SESSION_WRITE_FAILED.code)]
    WriteFailed {
        /// The workspace-relative path.
        path: String,
        /// The failure.
        #[source]
        source: std::io::Error,
    },
}

impl SessionError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::PristineBroken { .. } => error::SESSION_PRISTINE_BROKEN,
            Self::VerifyFailed { .. } => error::SESSION_VERIFY_FAILED,
            Self::UnknownMutant { .. } => error::SESSION_UNKNOWN_MUTANT,
            Self::UnknownTarget { .. } => error::SESSION_UNKNOWN_TARGET,
            Self::NoTargets { .. } => error::SESSION_NO_TARGETS,
            Self::WriteFailed { .. } => error::SESSION_WRITE_FAILED,
            Self::ReachesOutside { .. } => error::WORKSPACE_REACHES_OUTSIDE,
            Self::RootIsNotTheWorkspace { .. } => error::ROOT_IS_NOT_THE_WORKSPACE,
        }
    }
}

impl Workspace {
    /// Refuses a tree a copy of which would not build: one that is a member of a workspace, and one that reads code from outside itself.
    ///
    /// Both are asked of the tree on disk, before it is copied. Asked of the
    /// copy they would be asked of a tree that already cannot resolve, and
    /// cargo's answer would be about a manifest that is missing rather than
    /// about what a run could have done instead.
    fn reachable(
        root: &Path,
        toolchain: &Toolchain,
        options: &OpenOptions,
        cancel: &Cancel,
    ) -> Result<(), crate::EngineError> {
        let metadata = Metadata::load_no_deps(
            &Driver {
                toolchain,
                dir: root,
                cancel,
                trace: &options.trace,
            },
            MetadataOptions {
                locked: options.locked,
                offline: options.offline,
            },
        )?;
        let workspace_root = metadata
            .workspace_root
            .canonicalize()
            .unwrap_or_else(|_error| metadata.workspace_root.clone());
        if workspace_root != root {
            return Err(SessionError::RootIsNotTheWorkspace {
                root: root.to_path_buf(),
                workspace_root,
            }
            .into());
        }
        let allowed: Vec<PathBuf> = options
            .allow_outside
            .iter()
            .map(|path| path.canonicalize().unwrap_or_else(|_error| path.clone()))
            .collect();
        let patches = crate::cargo::manifest::patches(root);
        for outside in crate::cargo::reaching_outside(&metadata, root, &patches) {
            if allowed.iter().any(|allow| outside.path.starts_with(allow)) {
                continue;
            }
            return Err(SessionError::ReachesOutside {
                name: outside.name,
                manifest: outside.manifest,
                path: outside.path,
                root: root.to_path_buf(),
            }
            .into());
        }
        Ok(())
    }

    /// Sweeps the temporary area, copies `root` into a snapshot, and locates the toolchain inside the copy.
    ///
    /// # Errors
    /// The snapshot's refusals, and whatever stopped the toolchain from
    /// being located or `cargo metadata` from being read.
    pub fn open(
        root: &Path,
        options: OpenOptions,
        cancel: &Cancel,
    ) -> Result<Self, crate::EngineError> {
        let phase = options.trace.phase("open");
        let root = root
            .canonicalize()
            .unwrap_or_else(|_error| root.to_path_buf());
        let parent = options.temp_directory.clone();
        let now = jiff::Timestamp::now();
        let swept =
            tempowner::sweep(&parent, &[DIR_PREFIX, TARGET_DIR_PREFIX], now).unwrap_or_default();

        let toolchain = Toolchain::locate(
            &LocateOptions {
                cargo: options.cargo.clone(),
                search_path: options.search_path.clone(),
                env: Some(options.env.clone()),
            },
            &root,
            cancel,
        )?;
        Self::reachable(&root, &toolchain, &options, cancel)?;

        let snapshot = Self::copy(&root, &parent, &options, now)?;
        options.trace.open(OpenRecord {
            root: root.display().to_string(),
            snapshot_dir: snapshot.dir().display().to_string(),
            stable_dir: snapshot.stable_dir(),
            sweep: Some(SweepRecord {
                parent: parent.display().to_string(),
                removed: u64::try_from(swept.removed.len()).unwrap_or(u64::MAX),
                removed_bytes: swept.removed_bytes,
                live: u64::try_from(swept.live).unwrap_or(u64::MAX),
                kept: u64::try_from(swept.kept).unwrap_or(u64::MAX),
                failures: u64::try_from(swept.failures.len()).unwrap_or(u64::MAX),
            }),
        });
        let base_env = options.env.clone();
        let metadata = Metadata::load(
            &Driver {
                toolchain: &toolchain,
                dir: snapshot.root(),
                cancel,
                trace: &options.trace,
            },
            MetadataOptions {
                locked: options.locked,
                offline: options.offline,
            },
        )?;
        let target_dir = parent.join(format!(
            "{TARGET_DIR_PREFIX}{}",
            snapshot::stable_name(&root)
                .strip_prefix(DIR_PREFIX)
                .unwrap_or_default()
        ));
        let target_owner = claim_target(&target_dir, now);
        phase.end();
        Ok(Self {
            snapshot,
            toolchain,
            metadata,
            target_dir,
            target_owner,
            base_env,
            swept,
            keep_temp: options.keep_temp,
            offline: options.offline,
            locked: options.locked,
            trace: options.trace,
        })
    }

    /// Copies the tree and records what that produced.
    fn copy(
        root: &Path,
        parent: &Path,
        options: &OpenOptions,
        now: jiff::Timestamp,
    ) -> Result<Snapshot, crate::EngineError> {
        let started = std::time::Instant::now();
        let snapshot = snapshot::create(
            root,
            &SnapshotOptions {
                exclude: options.exclude.clone(),
                beside: options.allow_outside.clone(),
                report_dir: options.report_directory.clone(),
                dest_parent: parent.to_path_buf(),
            },
            now,
        )?;
        options.trace.snapshot(SnapshotRecord {
            source_root: root.display().to_string(),
            dir: snapshot.dir().display().to_string(),
            files: u64::try_from(snapshot.manifest().len()).unwrap_or(u64::MAX),
            bytes: snapshot.manifest().iter().map(|entry| entry.size).sum(),
            workspace_digest: Some(snapshot.workspace_digest().to_owned()),
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            error: None,
        });
        Ok(snapshot)
    }

    /// The absolute source root that was copied.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.snapshot.source_root()
    }

    /// The root of the copy everything happens in.
    #[must_use]
    pub fn snapshot_root(&self) -> &Path {
        self.snapshot.root()
    }

    /// The directory the snapshot owns.
    #[must_use]
    pub fn snapshot_dir(&self) -> &Path {
        self.snapshot.dir()
    }

    /// Every entry the snapshot did not copy because it is not a regular file.
    #[must_use]
    pub fn passed_over(&self) -> &[snapshot::PassedOver] {
        self.snapshot.passed_over()
    }

    /// The frozen digest of the copied tree.
    #[must_use]
    pub fn workspace_digest(&self) -> &str {
        self.snapshot.workspace_digest()
    }

    /// What the sweep on the way in collected.
    #[must_use]
    pub const fn swept(&self) -> &SweepResult {
        &self.swept
    }

    /// The located toolchain.
    #[must_use]
    pub const fn toolchain(&self) -> &Toolchain {
        &self.toolchain
    }

    /// What `cargo metadata` said about the copy.
    #[must_use]
    pub const fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// The target directory builds go into: outside the snapshot, at a name stable for this source root, so successive runs share cargo's incremental state.
    #[must_use]
    pub fn target_dir(&self) -> &Path {
        &self.target_dir
    }

    /// Discovers, instruments, validates, and builds; see [`prepare()`].
    ///
    /// # Errors
    /// Every failure of the phases it runs.
    pub fn prepare(
        self,
        options: &PrepareOptions,
        cancel: &Cancel,
    ) -> Result<Session, crate::EngineError> {
        prepare(self, options, cancel)
    }

    /// Removes the snapshot, or preserves it when the workspace was opened with `keep_temp`, and reports what was preserved.
    ///
    /// # Errors
    /// A snapshot directory that could not be removed.
    pub fn close(mut self) -> Result<Vec<PathBuf>, crate::EngineError> {
        if let Some(mut owner) = self.target_owner.take() {
            drop(owner.release());
        }
        if self.keep_temp {
            let dir = self.snapshot.dir().to_path_buf();
            self.snapshot.keep()?;
            return Ok(vec![dir, self.target_dir]);
        }
        self.snapshot.cleanup()?;
        Ok(Vec::new())
    }

    /// A driver for a cargo command in the snapshot.
    pub(crate) fn driver<'a>(&'a self, cancel: &'a Cancel) -> Driver<'a> {
        Driver {
            toolchain: &self.toolchain,
            dir: self.snapshot.root(),
            cancel,
            trace: &self.trace,
        }
    }

    /// How long a command may take, when the caller bounded it.
    pub(crate) const fn timeout(build_timeout: Option<Duration>) -> Option<Duration> {
        build_timeout
    }
}
