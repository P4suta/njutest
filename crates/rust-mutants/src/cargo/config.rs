// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What `.cargo/config.toml` says about the flags a build compiles with.
//!
//! A coverage build has to add `-Cinstrument-coverage`, and it does that
//! through `CARGO_ENCODED_RUSTFLAGS`, which *replaces* `build.rustflags`
//! rather than adding to it. A project that configures its own flags would
//! have them dropped and would be measured as a program it is not. Reading
//! them here is what lets the build put them back instead of refusing.
//!
//! What is not put back is `target.<triple>` and `target.cfg(…)`: which of
//! them apply is cargo's decision about the target being built, and a guess
//! compiles something other than the project's own binaries. A tree that
//! configures those is told so by name.
//!
//! Cargo joins array values across files rather than letting the nearest one
//! win, with the higher-precedence items placed later, and the home directory
//! is the lowest precedence of all. So the order here is `$CARGO_HOME` first,
//! then the outermost ancestor, and the directory itself last.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The directory cargo reads configuration from, in every ancestor and in `CARGO_HOME`.
pub const DIRECTORY: &str = ".cargo";

/// The two file names cargo accepts in that directory, the newer one first.
pub const FILE_NAMES: [&str; 2] = ["config.toml", "config"];

/// What separates arguments inside `CARGO_ENCODED_RUSTFLAGS`.
pub const SEPARATOR: char = '\u{1f}';

/// What the configuration files say about compiler flags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Configured {
    /// `build.rustflags` joined across the files that name them, lowest precedence first.
    pub build: Vec<String>,
    /// Whether any `target.*` table configures flags, which are not merged because which of them apply is cargo's decision.
    pub target_specific: bool,
    /// Whether a configuration file was found that this could not read faithfully, so what it configures is unknown.
    pub unreadable: bool,
}

/// Reads every configuration file a build in `root` would compile under, in cargo's own precedence order.
#[must_use]
pub fn configured(root: &Path, cargo_home: Option<&Path>) -> Configured {
    let mut layers: Vec<Vec<String>> = Vec::new();
    let mut found = Configured::default();
    for directory in root.ancestors().chain(cargo_home) {
        let Some(text) = holding(directory) else {
            continue;
        };
        let one = read(&text);
        found.target_specific |= one.target_specific;
        found.unreadable |= one.unreadable;
        layers.push(one.build);
    }
    layers.reverse();
    found.build = layers.into_iter().flatten().collect();
    found
}

/// What one configuration file says about compiler flags.
///
/// A file nobody can parse, and a file that spells a flag this could not pass
/// on as itself, are both read as saying nothing: `unreadable` is what a run
/// states, rather than flags it never saw or a flag cut in two.
#[must_use]
pub fn read(text: &str) -> Configured {
    let Ok(document) = text.parse::<toml::Table>() else {
        return Configured {
            unreadable: true,
            ..Configured::default()
        };
    };
    let target_specific = matches!(document.get("target"), Some(toml::Value::Table(targets))
        if targets.values().any(|one| one.get("rustflags").is_some()));
    let build = document
        .get("build")
        .and_then(|build| build.get("rustflags"))
        .map(as_flags)
        .unwrap_or_default();
    if build.iter().any(|flag| flag.contains(SEPARATOR)) {
        return Configured {
            build: Vec::new(),
            target_specific,
            unreadable: true,
        };
    }
    Configured {
        build,
        target_specific,
        unreadable: false,
    }
}

/// Where cargo keeps its own configuration, as the environment spells it.
#[must_use]
pub fn home(env: &[(OsString, OsString)]) -> Option<PathBuf> {
    let of = |name: &str| {
        env.iter()
            .find(|(key, _)| key == OsStr::new(name))
            .map(|(_, value)| PathBuf::from(value))
            .filter(|path| !path.as_os_str().is_empty())
    };
    of("CARGO_HOME").or_else(|| {
        of("HOME")
            .or_else(|| of("USERPROFILE"))
            .map(|home| home.join(".cargo"))
    })
}

/// The value of `CARGO_ENCODED_RUSTFLAGS` for a command, or nothing when there is nothing to say and the project's own configuration keeps applying.
#[must_use]
pub fn encoded(
    env: &[(OsString, OsString)],
    configured: &Configured,
    extra: &[&str],
) -> Option<OsString> {
    let mut flags = inherited(env).unwrap_or_else(|| configured.build.clone());
    flags.extend(extra.iter().map(|flag| (*flag).to_owned()));
    if flags.is_empty() {
        return None;
    }
    Some(OsString::from(flags.join(&SEPARATOR.to_string())))
}

/// What the first of the two names that one directory holds says.
fn holding(directory: &Path) -> Option<String> {
    let base = directory.join(DIRECTORY);
    FILE_NAMES
        .iter()
        .find_map(|name| std::fs::read_to_string(base.join(name)).ok())
}

/// A `rustflags` value: a list of arguments, or one string cargo splits on whitespace.
fn as_flags(value: &toml::Value) -> Vec<String> {
    match value {
        toml::Value::String(text) => split_plain(text),
        toml::Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(ToOwned::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// What cargo takes from the environment instead of the configuration, in cargo's order.
fn inherited(env: &[(OsString, OsString)]) -> Option<Vec<String>> {
    let of = |name: &str| {
        env.iter()
            .find(|(key, _)| key == OsStr::new(name))
            .map(|(_, value)| value.to_string_lossy().into_owned())
    };
    if let Some(encoded) = of("CARGO_ENCODED_RUSTFLAGS") {
        return Some(
            encoded
                .split(SEPARATOR)
                .filter(|flag| !flag.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
        );
    }
    of("RUSTFLAGS").map(|plain| split_plain(&plain))
}

/// How cargo splits a plain `RUSTFLAGS`: on whitespace, with empty pieces dropped.
fn split_plain(value: &str) -> Vec<String> {
    value.split_whitespace().map(ToOwned::to_owned).collect()
}
