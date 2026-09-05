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
    let dir = root.parent().unwrap_or(root).join("mjutest-devkit-temp");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}
