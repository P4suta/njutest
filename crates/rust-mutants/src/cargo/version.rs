// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `-vV` banner of cargo and rustc.

use super::{CargoError, CargoErrorKind};

/// What `cargo -vV` or `rustc -vV` said about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    /// The first line, e.g. `rustc 1.98.1 (48a229cea 2026-09-01)`.
    pub summary: String,
    /// The `release:` line, e.g. `1.98.1` or `1.100.0-nightly`.
    pub release: String,
    /// The `commit-hash:` line, absent when it says `unknown`.
    pub commit_hash: Option<String>,
    /// The `commit-date:` line, absent when it says `unknown`.
    pub commit_date: Option<String>,
    /// The `host:` line: the target triple the tool runs on.
    pub host: String,
    /// The `LLVM version:` line of rustc.
    pub llvm_version: Option<String>,
}

impl VersionInfo {
    /// Whether the release is a nightly.
    #[must_use]
    pub fn is_nightly(&self) -> bool {
        self.release.contains("nightly")
    }
}

/// Parses a `-vV` banner.
///
/// # Errors
/// [`CargoErrorKind::VersionUnreadable`] when the `release:` or `host:` line
/// is missing: without them the toolchain cannot be named or keyed.
pub fn parse_version(output: &str) -> Result<VersionInfo, CargoError> {
    let mut lines = output.lines();
    let summary = lines.next().unwrap_or_default().trim().to_owned();
    let mut release = None;
    let mut commit_hash = None;
    let mut commit_date = None;
    let mut host = None;
    let mut llvm_version = None;
    for line in lines {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let known =
            |value: &str| (value != "unknown" && !value.is_empty()).then(|| value.to_owned());
        match key.trim() {
            "release" => release = Some(value.to_owned()),
            "commit-hash" => commit_hash = known(value),
            "commit-date" => commit_date = known(value),
            "host" => host = Some(value.to_owned()),
            "LLVM version" => llvm_version = known(value),
            _ => {}
        }
    }
    let unreadable = |what: &str| {
        CargoError::new(
            CargoErrorKind::VersionUnreadable,
            format!("the version banner has no `{what}:` line: {summary:?}"),
        )
    };
    let release = release
        .filter(|r| !r.is_empty())
        .ok_or_else(|| unreadable("release"))?;
    let host = host
        .filter(|h| !h.is_empty())
        .ok_or_else(|| unreadable("host"))?;
    Ok(VersionInfo {
        summary,
        release,
        commit_hash,
        commit_date,
        host,
        llvm_version,
    })
}
