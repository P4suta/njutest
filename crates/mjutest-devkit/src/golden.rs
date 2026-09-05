// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Byte-exact golden-file comparison.
//!
//! A golden file records what a contract produced the last time somebody
//! looked at it. The comparison is bytes, never a normalized text: a trailing
//! newline, a CRLF, and a non-UTF-8 byte are all differences. Without
//! `UPDATE_GOLDEN=1` the comparison is read-only, and a missing file is a
//! failure rather than a silent first recording, so a golden test can never
//! pass by accident on a fresh checkout.

use std::fs;
use std::path::{Path, PathBuf};

/// Why a golden comparison did not pass.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GoldenError {
    /// The golden file does not exist and no update was requested.
    #[error("golden file {path} is missing; run with UPDATE_GOLDEN=1 to record it")]
    Missing {
        /// The golden file that was looked for.
        path: PathBuf,
    },
    /// The recorded bytes differ from the golden bytes.
    #[error(
        "golden file {path} differs from the recorded output (UPDATE_GOLDEN=1 rewrites it):\n{diff}"
    )]
    Mismatch {
        /// The golden file that was compared.
        path: PathBuf,
        /// A unified diff, golden first, or a byte-offset note for binary data.
        diff: String,
    },
    /// Reading or writing the golden file failed.
    #[error("golden file {path}: {source}")]
    Io {
        /// The golden file that was read or written.
        path: PathBuf,
        /// The underlying failure.
        #[source]
        source: std::io::Error,
    },
}

/// Compares `got` against the bytes of the golden file at `path`.
///
/// With `update` false the comparison is read-only: a missing file is
/// [`GoldenError::Missing`], never a silent first recording. With `update`
/// true the file is (re)written with `got`, parents included, and the
/// comparison passes.
///
/// # Errors
///
/// Returns the mismatch, the missing file, or the I/O failure.
pub fn compare_golden(path: &Path, got: &[u8], update: bool) -> Result<(), GoldenError> {
    let io = |source| GoldenError::Io {
        path: path.to_path_buf(),
        source,
    };
    if update {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        return fs::write(path, got).map_err(io);
    }
    let want = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Err(GoldenError::Missing {
                path: path.to_path_buf(),
            });
        }
        Err(source) => return Err(io(source)),
    };
    if want == got {
        return Ok(());
    }
    Err(GoldenError::Mismatch {
        path: path.to_path_buf(),
        diff: render_diff(&want, got),
    })
}

fn render_diff(want: &[u8], got: &[u8]) -> String {
    if let (Ok(want_text), Ok(got_text)) = (std::str::from_utf8(want), std::str::from_utf8(got)) {
        let diff = similar::TextDiff::from_lines(want_text, got_text);
        return diff
            .unified_diff()
            .context_radius(3)
            .header("golden", "recorded")
            .to_string();
    }
    let offset = want
        .iter()
        .zip(got)
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| want.len().min(got.len()));
    let mut note = format!(
        "binary data differs at offset {offset} (golden {} bytes, recorded {} bytes)",
        want.len(),
        got.len()
    );
    if let (Some(a), Some(b)) = (want.get(offset), got.get(offset)) {
        note = format!("{note}: golden 0x{a:02x}, recorded 0x{b:02x}");
    }
    note
}

/// Reports whether `UPDATE_GOLDEN=1` asked for golden files to be rewritten.
///
/// This is the one environment read of the devkit: tests are the composition
/// root of their own process, and `cargo test` accepts no flag of its own.
#[must_use]
pub fn update_requested() -> bool {
    std::env::var_os("UPDATE_GOLDEN").is_some_and(|value| value == "1")
}

/// [`compare_golden`] driven by [`update_requested`].
///
/// # Errors
///
/// See [`compare_golden`].
pub fn golden(path: &Path, got: &[u8]) -> Result<(), GoldenError> {
    compare_golden(path, got, update_requested())
}
