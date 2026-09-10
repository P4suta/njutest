// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where things are: the workspace root, the fixture projects, the cargo that built the test binary.

use std::fs;
use std::path::{Path, PathBuf};

/// The root of this workspace, resolved from this crate's manifest directory at compile time, so it does not depend on the working directory of the test process.
#[must_use]
pub fn workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map_or_else(|| manifest_dir.to_path_buf(), Path::to_path_buf)
}

/// The directory holding the independent fixture projects.
#[must_use]
pub fn fixtures_dir() -> PathBuf {
    workspace_root().join("fixtures")
}

/// The `cargo` a test drives a fixture with.
#[must_use]
pub fn cargo_binary() -> PathBuf {
    std::env::var_os("CARGO").map_or_else(|| PathBuf::from("cargo"), PathBuf::from)
}

/// A directory beside `root` for what a run puts in the temporary directory.
///
/// Snapshots, build caches, and worker scratch all land there. Pointing
/// `TMPDIR` at it is how a test bounds its own mess: what the run leaves
/// behind goes away with the tree the test owns, rather than accumulating in
/// the machine's shared temporary directory.
///
/// # Errors
/// The directory could not be created, which means the test has no place to work.
pub fn temp_beside(root: &Path) -> std::io::Result<PathBuf> {
    beside(root, "mjutest-devkit-temp")
}

/// A directory beside `root` for the caches a run keeps between runs.
///
/// It has to be outside the tree: a cache inside the tree under verification
/// would change the tree's own digest every time a run wrote to it, and no
/// second run of the same work would ever look like one.
///
/// # Errors
/// The directory could not be created, which means the test has no place to work.
pub fn cache_beside(root: &Path) -> std::io::Result<PathBuf> {
    beside(root, "mjutest-devkit-cache")
}

fn beside(root: &Path, name: &str) -> std::io::Result<PathBuf> {
    let dir = root.parent().unwrap_or(root).join(name);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The parent's environment with the variables a run composes for itself taken out.
///
/// A run refuses to inherit `RUST_MUTANTS_ACTIVE`, `RUST_MUTANTS_CATALOG` and
/// `RUST_MUTANTS_TOUCH`, because an inherited one would decide what somebody
/// else's run measured. That refusal is right, and it means a test that starts
/// a run inherits its way into it whenever the test is itself running under a
/// measurement — which is every test of this workspace when the workspace is
/// measuring itself.
///
/// So a test composes the environment it gives a run rather than passing its
/// own along. `LLVM_PROFILE_FILE` goes for the same reason from the other end:
/// a measurement sets it, and a child that inherited it writes over the
/// measurement that started it.
#[must_use]
pub fn environment_for_a_run() -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let composed: [&str; 4] = [
        "RUST_MUTANTS_ACTIVE",
        "RUST_MUTANTS_CATALOG",
        "RUST_MUTANTS_TOUCH",
        "LLVM_PROFILE_FILE",
    ];
    std::env::vars_os()
        .filter(|(name, _value)| {
            !composed
                .iter()
                .any(|reserved| name.as_os_str() == std::ffi::OsStr::new(reserved))
        })
        .collect()
}

/// A command for `program`, with the variables a run composes for itself taken out of what it inherits.
///
/// Every test that starts a run needs this, for the reason
/// [`environment_for_a_run`] gives: under a measurement of this workspace the
/// parent's environment names an activation, and a run that inherited one
/// refuses — correctly, and so a test that inherits its way into that refusal
/// cannot be measured at all.
#[must_use]
pub fn command(program: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    for composed in [
        "RUST_MUTANTS_ACTIVE",
        "RUST_MUTANTS_CATALOG",
        "RUST_MUTANTS_TOUCH",
        "LLVM_PROFILE_FILE",
    ] {
        let _configured = command.env_remove(composed);
    }
    command
}
