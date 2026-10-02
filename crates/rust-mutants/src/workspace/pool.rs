// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Exclusive source slots and build directories addressed by content, compiler and graph inputs.

use std::ffi::OsStr;
use std::io;
use std::path::Path;

use sha2::{Digest as _, Sha256};

use crate::cargo::Toolchain;
use crate::tempowner::{self, ClaimError, Owner};
use crate::vars::{Spelling, Variables};

/// A retained immutable graph with its single preparation lease held through the editable copy.
pub(super) struct Graph {
    frozen: crate::snapshot::FrozenGraph,
    _lease: std::fs::File,
    key: String,
    root: Root,
}

/// A retained source placement that preserves Cargo's standalone workspace discovery.
enum Root {
    Retained(std::path::PathBuf),
    Isolated(std::path::PathBuf),
}

impl Root {
    fn of(source: &Path, options: &super::OpenOptions, retained: &Path) -> io::Result<Self> {
        let manifest: toml::Table =
            toml::from_str(&std::fs::read_to_string(source.join("Cargo.toml"))?)
                .map_err(io::Error::other)?;
        if manifest.get("workspace").is_none() {
            for ancestor in retained.ancestors() {
                match std::fs::metadata(ancestor.join("Cargo.toml")) {
                    Ok(metadata) if metadata.is_file() => {
                        for parent in options.temp_directory.ancestors() {
                            match std::fs::metadata(parent.join("Cargo.toml")) {
                                Ok(metadata) if metadata.is_file() => {
                                    return Err(io::Error::other(
                                        "a standalone source owner would inherit another workspace",
                                    ));
                                }
                                Ok(_) => {}
                                Err(source) if source.kind() == io::ErrorKind::NotFound => {}
                                Err(source) => return Err(source),
                            }
                        }
                        return Ok(Self::Isolated(
                            options.temp_directory.join("standalone-source-graphs"),
                        ));
                    }
                    Ok(_) => {}
                    Err(source) if source.kind() == io::ErrorKind::NotFound => {}
                    Err(source) => return Err(source),
                }
            }
        }
        Ok(Self::Retained(retained.to_path_buf()))
    }

    fn path(&self) -> &Path {
        match self {
            Self::Retained(path) | Self::Isolated(path) => path,
        }
    }
}

impl Graph {
    pub(super) fn open(
        rules: &crate::snapshot::Options,
        options: &super::OpenOptions,
        now: jiff::Timestamp,
    ) -> Result<Self, crate::EngineError> {
        let original = semantic(rules).map_err(unavailable)?;
        let key = hex::encode(Sha256::digest(
            serde_json::to_vec(&original)
                .map_err(io::Error::other)
                .map_err(unavailable)?,
        ));
        let root = options
            .env
            .var("NJUTEST_FIXTURE_BUILD_CACHE")
            .ok_or_else(|| {
                unavailable(io::Error::other(
                    "the source graph has no retained cache owner",
                ))
            })?;
        if !Path::new(root).is_absolute() {
            return Err(unavailable(io::Error::other(
                "a source graph cache must be absolute",
            )));
        }
        let root =
            Root::of(rules.layout.source_root(), options, Path::new(root)).map_err(unavailable)?;
        let parent = root.path().join("source-graphs-v1").join(&key);
        std::fs::create_dir_all(&parent).map_err(unavailable)?;
        let lease = graph_lease(&parent, &options.trace).map_err(unavailable)?;
        let record = parent.join("graph.json");
        let frozen = match std::fs::read(&record) {
            Ok(bytes) => crate::strictjson::decode_slice::<crate::snapshot::FrozenGraph>(&bytes)
                .map_err(io::Error::other)
                .map_err(unavailable)?,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                let mut freezing = rules.clone();
                freezing.dest_parent.clone_from(&parent);
                let copied =
                    super::Workspace::copy(rules.layout.source_root(), &freezing, options, now)?;
                if semantic(rules).map_err(unavailable)? != original {
                    return Err(unavailable(io::Error::other(
                        "the source graph changed while it was copied",
                    )));
                }
                let frozen =
                    crate::snapshot::FrozenGraph::take(copied, rules).map_err(unavailable)?;
                frozen.verify(&parent).map_err(unavailable)?;
                crate::replace::file(
                    &record,
                    &serde_json::to_vec(&frozen)
                        .map_err(io::Error::other)
                        .map_err(unavailable)?,
                )
                .map_err(|error| unavailable(error.source))?;
                frozen
            }
            Err(source) => return Err(unavailable(source)),
        };
        frozen.verify(&parent).map_err(unavailable)?;
        let retained = semantic(
            &frozen
                .rules(std::path::PathBuf::new())
                .map_err(unavailable)?,
        )
        .map_err(unavailable)?;
        if retained.files != original.files {
            return Err(unavailable(io::Error::other(
                "the retained graph does not match its complete source inputs",
            )));
        }
        Ok(Self {
            frozen,
            _lease: lease,
            key,
            root,
        })
    }

    pub(super) fn key(&self) -> &str {
        &self.key
    }

    pub(super) fn retained_root(&self) -> &Path {
        self.root.path()
    }

    pub(super) fn copy(
        &self,
        (original, parent): (&Path, std::path::PathBuf),
        options: &super::OpenOptions,
        now: jiff::Timestamp,
    ) -> Result<crate::snapshot::Snapshot, crate::EngineError> {
        let rules = self.frozen.rules(parent).map_err(unavailable)?;
        let mut snapshot = super::Workspace::copy(original, &rules, options, now)?;
        let placement = rules
            .layout
            .under(snapshot.dir().join(crate::snapshot::TREE_NAME));
        let copied_rules = crate::snapshot::Options::new(
            crate::snapshot::Layout::plan(
                placement.root(),
                &placement
                    .beside()
                    .iter()
                    .map(|placed| placed.destination().to_path_buf())
                    .collect::<Vec<_>>(),
            )?,
            std::path::PathBuf::new(),
        );
        if semantic(&copied_rules).map_err(unavailable)?.files
            != semantic(&rules).map_err(unavailable)?.files
        {
            return Err(unavailable(io::Error::other(
                "an editable graph differs from its complete immutable input",
            )));
        }
        self.frozen
            .bind_copy(&mut snapshot, original)
            .map_err(unavailable)?;
        Ok(snapshot)
    }
}

pub(super) fn unavailable(source: io::Error) -> crate::EngineError {
    super::SessionError::WriteFailed {
        path: "owned immutable source graph".to_owned(),
        source,
    }
    .into()
}

fn graph_lease(parent: &Path, trace: &crate::trace::Recorder) -> io::Result<std::fs::File> {
    let lease = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(parent.join("preparation.lock"))?;
    let started = std::time::Instant::now();
    let result = lease.lock();
    trace.note("host-wait", &serde_json::json!({
        "owner": parent.display().to_string(), "cause": "immutable source graph publication",
        "elapsed_ns": u64::try_from(started.elapsed().as_nanos()).map_err(io::Error::other)?,
        "machine": {"os": std::env::consts::OS, "cpus": std::thread::available_parallelism()?.get()}
    }).to_string());
    result?;
    Ok(lease)
}

/// Names every outside file by its relative placement, preserving distinct filesystem roots.
fn semantic(rules: &crate::snapshot::Options) -> io::Result<crate::snapshot::Survey> {
    let mut surveyed = crate::snapshot::survey(rules).map_err(io::Error::other)?;
    let placement = rules.layout.under(std::path::PathBuf::from("graph"));
    let named = |name: String| {
        for outside in placement.beside() {
            match Path::new(&name).strip_prefix(outside.source()) {
                Ok(relative) => return outside.destination().join(relative).display().to_string(),
                Err(_not_an_outside_path) => {}
            }
        }
        name
    };
    surveyed.files = surveyed
        .files
        .into_iter()
        .map(|(path, state)| (named(path), state))
        .collect();
    surveyed.passed_over = surveyed
        .passed_over
        .into_iter()
        .map(|(path, state)| (named(path), state))
        .collect();
    surveyed.rules = hex::encode(Sha256::digest(
        serde_json::to_vec(&(
            rules
                .exclude
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            &rules.report_dir,
            &rules.build_dir,
            placement
                .beside()
                .iter()
                .map(crate::snapshot::Placed::destination)
                .collect::<Vec<_>>(),
        ))
        .map_err(io::Error::other)?,
    ));
    Ok(surveyed)
}

/// Claims the first free editable source lease without a bounded private fallback.
pub(super) fn claim(
    root: &Path,
    vars: &Variables,
    (content, toolchain): (&str, &Toolchain),
    now: jiff::Timestamp,
) -> io::Result<Option<Owner>> {
    if !root.is_absolute() {
        return Err(io::Error::other("a fixture build cache must be absolute"));
    }
    std::fs::create_dir_all(root)?;
    let root = crate::canonical::canonical(root)?;
    let mut digest = Sha256::new();
    for part in [
        "njutest-fixture-build-v1",
        content,
        &format!(
            "{:?}/{:?}",
            toolchain.cargo_version(),
            toolchain.rustc_version()
        ),
    ] {
        hashed(&mut digest, part.as_bytes())?;
    }
    hashed(
        &mut digest,
        toolchain.cargo().as_os_str().as_encoded_bytes(),
    )?;
    hashed(
        &mut digest,
        toolchain.rustc().as_os_str().as_encoded_bytes(),
    )?;
    let spelling = vars.spelling();
    for (name, value) in vars
        .canonical()
        .into_iter()
        .filter(|(name, _value)| !diagnostic(spelling, name))
    {
        hashed(&mut digest, name.as_encoded_bytes())?;
        hashed(&mut digest, value.as_encoded_bytes())?;
    }
    let key = hex::encode(digest.finalize());
    let mut slot = 0_u64;
    loop {
        let directory = root.join(&key).join(slot.to_string());
        std::fs::create_dir_all(&directory)?;
        match tempowner::claim_cache(&directory, now, "njutest-fixture-build-owner-v1") {
            Ok(owner) => return Ok(Some(owner)),
            Err(ClaimError::Owned { .. }) => {
                slot = slot
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("source lease identities exhausted"))?;
            }
            Err(source @ (ClaimError::Lock { .. } | ClaimError::Marker { .. })) => {
                return Err(io::Error::other(source));
            }
        }
    }
}

/// Graphs with arbitrary compiler programs get separate targets for every full input environment.
pub(super) fn target(
    slot: &Path,
    snapshot: &crate::snapshot::Snapshot,
    metadata: &crate::cargo::Metadata,
    vars: &Variables,
) -> io::Result<std::path::PathBuf> {
    let opaque = metadata
        .packages
        .iter()
        .flat_map(|package| &package.targets)
        .any(|target| {
            target
                .kind
                .iter()
                .any(|kind| kind == "custom-build" || kind == "proc-macro")
        });
    let parent = if opaque {
        let mut digest = Sha256::new();
        for (name, value) in vars.canonical() {
            hashed(&mut digest, name.as_encoded_bytes())?;
            hashed(&mut digest, value.as_encoded_bytes())?;
        }
        slot.join(format!("inputs-{}", hex::encode(digest.finalize())))
    } else {
        slot.to_path_buf()
    };
    Ok(super::target_of(&parent, snapshot.root()))
}

/// Names excluded from the source-slot identity, restored to opaque graphs' target identity.
fn diagnostic(spelling: Spelling, name: &OsStr) -> bool {
    spelling.begins(name, "NEXTEST_")
        || [
            "NJUTEST_TEST_COST_DIR",
            "NJUTEST_FIXTURE_BUILD_CACHE",
            "NJUTEST_TEST_CLOCK",
            "TMPDIR",
            "TMP",
            "TEMP",
            "XDG_CACHE_HOME",
        ]
        .iter()
        .any(|ignored| spelling.same(name, OsStr::new(ignored)))
}

/// Adds one complete, length-prefixed input to the build identity.
fn hashed(digest: &mut Sha256, bytes: &[u8]) -> io::Result<()> {
    let size = u64::try_from(bytes.len()).map_err(io::Error::other)?;
    digest.update(size.to_be_bytes());
    digest.update(bytes);
    Ok(())
}
