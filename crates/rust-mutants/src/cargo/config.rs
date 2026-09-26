// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What `.cargo/config.toml` says about the flags a build compiles with.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The directory cargo reads configuration from, in every ancestor and in `CARGO_HOME`.
pub const DIRECTORY: &str = ".cargo";

/// The two file names cargo accepts in that directory, the newer one first.
pub const FILE_NAMES: [&str; 2] = ["config.toml", "config"];

/// The variable a composed set of flags is put in, which is the encoded form so a value with a space cannot become two flags.
pub const ENCODED_RUSTFLAGS: &str = "CARGO_ENCODED_RUSTFLAGS";

/// The plain form, which cargo ignores when the encoded one is set.
pub const RUSTFLAGS: &str = "RUSTFLAGS";

/// What separates arguments inside `CARGO_ENCODED_RUSTFLAGS`.
pub const SEPARATOR: char = '\u{1f}';

/// A compiler-flag environment value which cannot be preserved exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// Cargo's textual flag protocol was supplied through a non-UTF-8 value.
    #[error("{variable} is not valid UTF-8, so its compiler flags cannot be preserved exactly")]
    NonUtf8Environment {
        /// The environment variable whose bytes were refused.
        variable: &'static str,
    },
}

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
        let text = match holding(directory) {
            Held::Absent => continue,
            Held::Text(text) => text,
            Held::Unreadable => {
                found.unreadable = true;
                continue;
            }
        };
        let one = read(&text);
        found.target_specific |= one.target_specific;
        found.unreadable |= one.unreadable;
        layers.push(one.build);
    }
    layers.reverse();
    found.build = layers
        .into_iter()
        .flat_map(IntoIterator::into_iter)
        .collect();
    found
}

/// The digest of every configuration file a build in `root` would read, each with where it is, in cargo's precedence order: a file changing, appearing, or vanishing anywhere cargo looks changes it.
#[must_use]
pub fn fingerprint(root: &Path, cargo_home: Option<&Path>) -> crate::id::HexDigest {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hasher.update(b"rust-mutants-cargo-configuration-v1\0");
    for directory in root.ancestors().chain(cargo_home) {
        for name in FILE_NAMES {
            let path = directory.join(DIRECTORY).join(name);
            let read = match std::fs::read(&path) {
                Ok(bytes) => crate::id::HexDigest::of(&bytes).into_inner(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => format!("unreadable: {}", error.kind()),
            };
            hasher.update(path.as_os_str().as_encoded_bytes());
            hasher.update(b"\0");
            hasher.update(read.as_bytes());
            hasher.update(b"\0");
        }
    }
    crate::id::HexDigest::finish(hasher)
}

/// What one configuration file says about compiler flags.
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
    let build = match document
        .get("build")
        .and_then(|build| build.get("rustflags"))
    {
        Some(value) => match as_flags(value) {
            Some(flags) => flags,
            None => {
                return Configured {
                    target_specific,
                    unreadable: true,
                    ..Configured::default()
                };
            }
        },
        None => Vec::new(),
    };
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
pub fn home(env: &crate::vars::Variables) -> Option<PathBuf> {
    let of = |name: &str| {
        env.var(name)
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
    };
    if let Some(cargo_home) = of("CARGO_HOME") {
        return Some(cargo_home);
    }
    let home = match of("HOME") {
        Some(home) => home,
        None => of("USERPROFILE")?,
    };
    Some(home.join(".cargo"))
}

/// The value of `CARGO_ENCODED_RUSTFLAGS` for a command, or nothing when there is nothing to say and the project's own configuration keeps applying.
///
/// # Errors
/// Refuses a non-UTF-8 flag variable instead of changing its bytes with a lossy conversion.
pub fn encoded(
    env: &crate::vars::Variables,
    configured: &Configured,
    extra: &[&str],
) -> Result<Option<OsString>, ConfigError> {
    let mut flags = match inherited(env)? {
        Some(inherited) => inherited,
        None => configured.build.clone(),
    };
    flags.extend(extra.iter().map(|flag| (*flag).to_owned()));
    if flags.is_empty() {
        return Ok(None);
    }
    Ok(Some(OsString::from(flags.join(&SEPARATOR.to_string()))))
}

/// What the first of the two names that one directory holds says.
enum Held {
    Absent,
    Text(String),
    Unreadable,
}

fn holding(directory: &Path) -> Held {
    let base = directory.join(DIRECTORY);
    for name in FILE_NAMES {
        match std::fs::read_to_string(base.join(name)) {
            Ok(text) => return Held::Text(text),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(_unreadable) => return Held::Unreadable,
        }
    }
    Held::Absent
}

/// A `rustflags` value: a list of arguments, or one string cargo splits on whitespace.
fn as_flags(value: &toml::Value) -> Option<Vec<String>> {
    match value {
        toml::Value::String(text) => Some(split_plain(text)),
        toml::Value::Array(items) => items
            .iter()
            .map(|item| item.as_str().map(ToOwned::to_owned))
            .collect(),
        _ => None,
    }
}

/// What cargo takes from the environment instead of the configuration, in cargo's order.
fn inherited(env: &crate::vars::Variables) -> Result<Option<Vec<String>>, ConfigError> {
    let of = |name: &'static str| -> Result<Option<String>, ConfigError> {
        match env.var(name) {
            Some(value) => value
                .to_str()
                .map(str::to_owned)
                .map(Some)
                .ok_or(ConfigError::NonUtf8Environment { variable: name }),
            None => Ok(None),
        }
    };
    if let Some(encoded) = of("CARGO_ENCODED_RUSTFLAGS")? {
        return Ok(Some(
            encoded
                .split(SEPARATOR)
                .filter(|flag| !flag.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
        ));
    }
    Ok(of("RUSTFLAGS")?.map(|plain| split_plain(&plain)))
}

/// How cargo splits a plain `RUSTFLAGS`: on whitespace, with empty pieces dropped.
fn split_plain(value: &str) -> Vec<String> {
    value.split_whitespace().map(ToOwned::to_owned).collect()
}
