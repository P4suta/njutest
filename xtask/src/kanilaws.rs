// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Proving the production laws with Kani, or reading back the proof of exactly these inputs, and auditing whichever it is.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest as _, Sha256};

use crate::gates::GateFailure;
use crate::kaniaudit::Harness;

/// Everything a proof of the production laws rests on, by path from the workspace root: the prover reads the crate and its locked graph with the pinned toolchain, and the audit pins the prover.
pub const INPUTS: [&str; 6] = [
    "crates/rust-mutants/src",
    "crates/rust-mutants/Cargo.toml",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "xtask/src/kaniaudit.rs",
];

/// Every production harness, by the name Kani proves it under.
#[must_use]
pub fn harnesses() -> Vec<&'static str> {
    Harness::ALL.iter().map(|harness| harness.name()).collect()
}

/// The arguments that prove every production harness and write Kani's raw export to `export`.
#[must_use]
pub fn arguments(export: &Path) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = [
        "kani",
        "-p",
        "rust-mutants",
        "--lib",
        "--exact",
        "--no-assertion-reach-checks",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    for harness in harnesses() {
        arguments.push(OsString::from("--harness"));
        arguments.push(OsString::from(harness));
    }
    arguments.extend(["-Z", "unstable-options", "--export-json"].map(OsString::from));
    arguments.push(export.as_os_str().to_owned());
    arguments
}

/// The digest of every input a proof rests on, the harnesses asked, and the workspace it was proved in.
///
/// # Errors
/// An input that cannot be read, or a path under the workspace that is not UTF-8.
pub fn key(root: &Path) -> Result<String, GateFailure> {
    let mut hasher = Sha256::new();
    let mut part = |bytes: &[u8]| {
        hasher.update(bytes.len().to_be_bytes());
        hasher.update(bytes);
    };
    part(spelled(root)?.as_bytes());
    for harness in harnesses() {
        part(harness.as_bytes());
    }
    for input in INPUTS {
        let mut files = files_of(root, input)?;
        files.sort();
        for file in files {
            let relative = file
                .strip_prefix(root)
                .map_err(|error| GateFailure(format!("{}: {error}", file.display())))?;
            part(spelled(relative)?.as_bytes());
            let bytes = std::fs::read(&file)
                .map_err(|error| GateFailure(format!("{}: {error}", file.display())))?;
            part(&bytes);
        }
    }
    Ok(hex::encode(hasher.finalize()))
}

/// `path` as UTF-8, or the refusal to key a proof by a name that is not.
fn spelled(path: &Path) -> Result<&str, GateFailure> {
    path.to_str()
        .ok_or_else(|| GateFailure(format!("{}: a path that is not UTF-8", path.display())))
}

/// Every file of the repository at `root` that is `input` or lies under it.
fn files_of(root: &Path, input: &str) -> Result<Vec<PathBuf>, GateFailure> {
    let under = format!("{}/", input.trim_end_matches('/'));
    Ok(crate::repository::files(root)?
        .into_iter()
        .filter(|relative| relative == input || relative.starts_with(&under))
        .map(|relative| root.join(relative))
        .collect())
}

/// Proves the production laws in `root` with `cargo`, or reads back from `cache` the proof of exactly these inputs, and audits the export either way.
///
/// # Errors
/// A proof that fails, an export the audit refuses, or a cache that cannot be written.
pub fn laws(root: &Path, cargo: &OsStr, cache: &Path) -> Result<String, GateFailure> {
    let key = key(root)?;
    let kept = cache.join(format!("{key}.json"));
    match crate::kaniaudit::audit(&kept, root) {
        Ok(()) => {
            return Ok(format!(
                "kani-laws: {} production harnesses answered by the proof of these exact \
                 inputs ({}), audited again",
                harnesses().len(),
                key.get(..12).unwrap_or(&key)
            ));
        }
        Err(crate::kaniaudit::AuditError::Read { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(_kept_for_another_workspace_or_prover) => {}
    }
    std::fs::create_dir_all(cache)
        .map_err(|error| GateFailure(format!("{}: {error}", cache.display())))?;
    let fresh = tempfile::Builder::new()
        .prefix("kani-laws-")
        .suffix(".json")
        .tempfile_in(cache)
        .map_err(|error| GateFailure(format!("{}: {error}", cache.display())))?;
    let status = Command::new(cargo)
        .args(arguments(fresh.path()))
        .env_remove("RUSTFLAGS")
        .current_dir(root)
        .status()
        .map_err(|error| GateFailure(format!("cargo kani could not start: {error}")))?;
    if !status.success() {
        return Err(GateFailure(format!("cargo kani failed: {status}")));
    }
    crate::kaniaudit::audit(fresh.path(), root).map_err(|error| GateFailure(error.to_string()))?;
    fresh
        .persist(&kept)
        .map_err(|error| GateFailure(format!("{}: {error}", kept.display())))?;
    Ok(format!(
        "kani-laws: {} production harnesses proved and audited, and the proof kept for these \
         exact inputs ({})",
        harnesses().len(),
        key.get(..12).unwrap_or(&key)
    ))
}
