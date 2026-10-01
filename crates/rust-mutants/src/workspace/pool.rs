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

/// Hashes the complete copied tree and every outside directory at their relative placement.
pub(super) fn content(
    snapshot: &crate::snapshot::Snapshot,
    rules: &crate::snapshot::Options,
) -> io::Result<String> {
    let mut digest = Sha256::new();
    hashed(&mut digest, snapshot.workspace_digest().as_bytes())?;
    let placement = rules
        .layout
        .under(snapshot.dir().join(crate::snapshot::TREE_NAME));
    for (name, directory) in std::iter::once((Path::new("."), snapshot.root())).chain(
        placement
            .beside()
            .iter()
            .map(|placed| (placed.destination(), placed.destination())),
    ) {
        let name = match name.strip_prefix(placement.stage()) {
            Ok(relative) => relative,
            Err(_root_uses_the_dot_name) => name,
        };
        hashed(&mut digest, name.as_os_str().as_encoded_bytes())?;
        let layout = crate::snapshot::Layout::plan(directory, &[]).map_err(io::Error::other)?;
        let surveyed = crate::snapshot::survey(&crate::snapshot::Options::new(
            layout,
            std::path::PathBuf::new(),
        ))
        .map_err(io::Error::other)?;
        hashed(
            &mut digest,
            &serde_json::to_vec(&surveyed).map_err(io::Error::other)?,
        )?;
    }
    Ok(hex::encode(digest.finalize()))
}

/// Claims an exclusive source slot before the graph is loaded, or leaves a busy pool private.
pub(super) fn claim(
    vars: &Variables,
    (content, toolchain): (&str, &Toolchain),
    now: jiff::Timestamp,
) -> io::Result<Option<Owner>> {
    let Some(root) = vars.var("NJUTEST_FIXTURE_BUILD_CACHE") else {
        return Ok(None);
    };
    if !Path::new(root).is_absolute() {
        return Err(io::Error::other("a fixture build cache must be absolute"));
    }
    std::fs::create_dir_all(root)?;
    let root = crate::canonical::canonical(Path::new(root))?;
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
    for slot in 0..4 {
        let directory = root.join(&key).join(slot.to_string());
        std::fs::create_dir_all(&directory)?;
        match tempowner::claim_cache(&directory, now, "njutest-fixture-build-owner-v1") {
            Ok(owner) => return Ok(Some(owner)),
            Err(ClaimError::Owned { .. }) => {}
            Err(source @ (ClaimError::Lock { .. } | ClaimError::Marker { .. })) => {
                return Err(io::Error::other(source));
            }
        }
    }
    Ok(None)
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
