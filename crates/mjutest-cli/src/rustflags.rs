// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a build is compiled with.

use std::ffi::{OsStr, OsString};
use std::path::Path;

/// The flag the coverage build adds.
pub const COVERAGE_FLAG: &str = "-Cinstrument-coverage";

/// What separates arguments inside `CARGO_ENCODED_RUSTFLAGS`.
pub const SEPARATOR: char = '\u{1f}';

/// The limitation a run states when the project configures flags for a target and the coverage build could not merge them.
pub const TARGET_RUSTFLAGS_LIMITATION: &str = "target-rustflags-not-merged";

/// The limitation a run states when a cargo configuration file could not be read at all.
pub const UNREADABLE_CONFIG_LIMITATION: &str = "cargo-configuration-unreadable";

/// What `.cargo/config.toml` says about compiler flags.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Configured {
    /// `build.rustflags`, closest configuration file first.
    pub build: Vec<String>,
    /// Whether any `target.*` table configures flags, which this does not merge because which of them apply is cargo's decision.
    pub target_specific: bool,
    /// Whether a configuration file was found and could not be read.
    pub unreadable: bool,
}

/// Reads `build.rustflags` from the cargo configuration at `root` and every ancestor, closest first, which is the order cargo joins them in.
#[must_use]
pub fn configured(root: &Path) -> Configured {
    let mut found = Configured::default();
    for directory in root.ancestors() {
        for name in ["config.toml", "config"] {
            let path = directory.join(".cargo").join(name);
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            match toml::from_str::<toml::Value>(&text) {
                Ok(value) => absorb(&value, &mut found),
                Err(_error) => found.unreadable = true,
            }
            break;
        }
    }
    found
}

/// Takes what one configuration file says about flags.
fn absorb(value: &toml::Value, found: &mut Configured) {
    if let Some(flags) = value
        .get("build")
        .and_then(|build| build.get("rustflags"))
        .and_then(as_flags)
    {
        found.build.extend(flags);
    }
    if let Some(targets) = value.get("target").and_then(toml::Value::as_table) {
        found.target_specific = found.target_specific
            || targets
                .values()
                .any(|entry| entry.get("rustflags").is_some());
    }
}

/// A `rustflags` value: an array of arguments, or one string cargo splits on whitespace.
fn as_flags(value: &toml::Value) -> Option<Vec<String>> {
    match value {
        toml::Value::Array(items) => Some(
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect(),
        ),
        toml::Value::String(text) => Some(split_plain(text)),
        _ => None,
    }
}

/// The value of `CARGO_ENCODED_RUSTFLAGS` for a command, or `None` when there is nothing to say — leaving the variable unset, so the project's own configuration keeps applying.
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

/// What cargo itself would take from the environment, in cargo's order. `None` means the environment says nothing, and the configuration is what cargo would use.
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

/// How cargo splits a plain `RUSTFLAGS`: on whitespace, with empty pieces dropped rather than passed as empty arguments.
fn split_plain(value: &str) -> Vec<String> {
    value.split_whitespace().map(ToOwned::to_owned).collect()
}
