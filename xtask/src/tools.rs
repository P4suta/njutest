// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The executables the gates invoke are the ones mise pins: selected through `mise which`, asked through Cargo's external-subcommand protocol, and refused where the path would answer with another one.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::environment::Environment;

/// Why the pinned-tool gate refused.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ToolsError {
    /// mise.toml could not be read.
    #[error("{path}: {source}")]
    Read {
        /// The unreadable file.
        path: String,
        /// The filesystem failure.
        source: std::io::Error,
    },
    /// mise.toml was not the shape this gate reads.
    #[error("{path}: {source}")]
    Parse {
        /// The malformed file.
        path: String,
        /// The TOML failure.
        source: toml::de::Error,
    },
    /// A program the gate asks could not be started.
    #[error("could not start {program}: {source}")]
    Start {
        /// The program.
        program: String,
        /// The operating system's answer.
        source: std::io::Error,
    },
    /// A program's answer was not text.
    #[error("{program} answered with bytes that are not UTF-8")]
    NotText {
        /// The program.
        program: String,
    },
    /// mise could not resolve a pinned tool's executable.
    #[error("mise does not resolve {tool}: {said}")]
    Unresolved {
        /// The pinned tool.
        tool: String,
        /// What mise said.
        said: String,
    },
    /// An earlier directory on the path answers the tool's name with another executable.
    #[error("{tool}: the path answers with {shadow} before mise's pinned {pinned}")]
    Shadowed {
        /// The pinned tool.
        tool: String,
        /// The executable the path finds first.
        shadow: String,
        /// The executable mise pins.
        pinned: String,
    },
    /// The pinned executable did not answer through Cargo's protocol.
    #[error("{tool}: `cargo {subcommand} --version` failed: {said}")]
    Protocol {
        /// The pinned tool.
        tool: String,
        /// The subcommand Cargo was asked to run.
        subcommand: String,
        /// What the invocation said.
        said: String,
    },
    /// The executable that answered through the protocol is not the pinned version.
    #[error("{tool}: answered {answered}, mise.toml pins {pinned}")]
    WrongVersion {
        /// The pinned tool.
        tool: String,
        /// The version the protocol invocation answered.
        answered: String,
        /// The version mise.toml pins.
        pinned: String,
    },
}

impl crate::error::Coded for ToolsError {
    fn code(&self) -> crate::error::XtCode {
        crate::error::XtCode::GateRefused
    }
}

/// The `[tools]` table of mise.toml, which is the whole shape this gate reads of it.
#[derive(Debug, serde::Deserialize)]
struct Tools {
    tools: BTreeMap<String, String>,
}

/// One pinned cargo plugin, as mise.toml names it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Plugin {
    /// The executable name, `cargo-nextest` for the pin `cargo:cargo-nextest`.
    executable: String,
    /// The subcommand Cargo runs it as.
    subcommand: String,
    /// The pinned version.
    pinned: String,
}

/// Validates every pinned cargo plugin for the gates that invoke it.
///
/// mise must resolve its executable, nothing earlier on the path may shadow it, and `cargo <subcommand> --version` through the external-subcommand protocol must answer with the pinned version.
///
/// # Errors
/// The first pin whose selection, protocol or version this cannot establish.
pub fn check(
    root: &Path,
    mise: &OsStr,
    cargo: &OsStr,
    environment: &Environment,
) -> Result<String, ToolsError> {
    let plugins = pins(root)?;
    for plugin in &plugins {
        let pinned = resolved(mise, &plugin.executable)?;
        first_on_path(&plugin.executable, environment, &pinned)?;
        answered(cargo, environment, plugin, &pinned)?;
    }
    let said = plugins
        .iter()
        .map(|plugin| format!("{} {}", plugin.executable, plugin.pinned))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "tools: {count} pinned cargo plugin(s) selected by mise, unshadowed on the path, and answering through cargo's protocol: {said}",
        count = plugins.len()
    ))
}

/// Every `cargo:` pin in mise.toml, as executable name, subcommand and pinned version.
fn pins(root: &Path) -> Result<Vec<Plugin>, ToolsError> {
    let path = root.join("mise.toml");
    let shown = path.display().to_string();
    let text = std::fs::read_to_string(&path).map_err(|source| ToolsError::Read {
        path: shown.clone(),
        source,
    })?;
    let table: Tools = toml::from_str(&text).map_err(|source| ToolsError::Parse {
        path: shown,
        source,
    })?;
    Ok(table
        .tools
        .iter()
        .filter_map(|(held, pinned)| {
            let tool = held.strip_prefix("cargo:")?;
            let subcommand = tool.strip_prefix("cargo-")?;
            Some(Plugin {
                executable: tool.to_owned(),
                subcommand: subcommand.to_owned(),
                pinned: pinned.clone(),
            })
        })
        .collect())
}

/// The executable mise selects for `tool`, refused where mise cannot name one.
fn resolved(mise: &OsStr, tool: &str) -> Result<PathBuf, ToolsError> {
    let output = Command::new(mise)
        .args(["which", tool])
        .output()
        .map_err(|source| ToolsError::Start {
            program: tool.to_owned(),
            source,
        })?;
    if !output.status.success() {
        return Err(ToolsError::Unresolved {
            tool: tool.to_owned(),
            said: String::from_utf8(output.stderr).map_err(|_undecodable| ToolsError::NotText {
                program: tool.to_owned(),
            })?,
        });
    }
    let said = String::from_utf8(output.stdout).map_err(|_undecodable| ToolsError::NotText {
        program: tool.to_owned(),
    })?;
    Ok(PathBuf::from(said.trim_end()))
}

/// The first directory on `environment`'s path holding an executable `tool`, refused where it is not the pinned one.
fn first_on_path(
    tool: &str,
    environment: &Environment,
    pinned: &Path,
) -> Result<PathBuf, ToolsError> {
    let path = environment.value("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(tool);
        if executable(&candidate) {
            return if candidate == pinned {
                Ok(candidate)
            } else {
                Err(ToolsError::Shadowed {
                    tool: tool.to_owned(),
                    shadow: candidate.display().to_string(),
                    pinned: pinned.display().to_string(),
                })
            };
        }
    }
    Err(ToolsError::Unresolved {
        tool: tool.to_owned(),
        said: "no directory on the path holds its executable".to_owned(),
    })
}

/// Whether `path` names a file this process could execute.
fn executable(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(held) => held.is_file() && executable_bit(path),
        Err(_absent) => false,
    }
}

/// Whether the file carries at least one execute bit, which is a Unix question.
#[cfg(unix)]
fn executable_bit(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    match std::fs::metadata(path) {
        Ok(held) => held.permissions().mode() & 0o111 != 0,
        Err(_absent) => false,
    }
}

/// Whether the file carries at least one execute bit, which Windows answers by extension.
#[cfg(not(unix))]
fn executable_bit(_path: &Path) -> bool {
    true
}

/// What `cargo <subcommand> --version` answers, refused unless it is the pinned version.
fn answered(
    cargo: &OsStr,
    environment: &Environment,
    plugin: &Plugin,
    pinned: &Path,
) -> Result<(), ToolsError> {
    let mut asking = Command::new(cargo);
    asking.args([plugin.subcommand.as_str(), "--version"]);
    asking.envs(environment.pairs());
    let output = asking.output().map_err(|source| ToolsError::Start {
        program: pinned.display().to_string(),
        source,
    })?;
    let said = String::from_utf8(output.stdout).map_err(|_undecodable| ToolsError::NotText {
        program: plugin.executable.clone(),
    })?;
    if !output.status.success() {
        let mut complained =
            String::from_utf8(output.stderr).map_err(|_undecodable| ToolsError::NotText {
                program: plugin.executable.clone(),
            })?;
        complained.push_str(&said);
        return Err(ToolsError::Protocol {
            tool: plugin.executable.clone(),
            subcommand: plugin.subcommand.clone(),
            said: complained.trim_end().to_owned(),
        });
    }
    if said.contains(&plugin.pinned) {
        return Ok(());
    }
    Err(ToolsError::WrongVersion {
        tool: plugin.executable.clone(),
        answered: first_version(&said),
        pinned: plugin.pinned.clone(),
    })
}

/// The first version-shaped word of `said`, so a refusal names what answered rather than a whole banner.
fn first_version(said: &str) -> String {
    said.split_whitespace()
        .find(|word| {
            word.chars()
                .next()
                .is_some_and(|first| first.is_ascii_digit())
        })
        .unwrap_or("no version")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::environment::Environment;
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};

    fn scripted(root: &Path, name: &str, body: &str) -> PathBuf {
        let path = root.join(name);
        std::fs::write(
            &path,
            format!("#!/usr/bin/env bash\nset -euo pipefail\n{body}\n"),
        )
        .expect("a scripted program");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("an executable script");
        path
    }

    fn environment(pairs: &[(&str, &str)]) -> Environment {
        Environment::of(
            pairs
                .iter()
                .map(|(name, value)| (OsString::from(*name), OsString::from(*value))),
        )
    }

    fn repository(root: &Path, table: &str) {
        std::fs::write(root.join("mise.toml"), table).expect("the pins");
    }

    #[test]
    fn the_pinned_plugin_answering_through_the_protocol_passes() {
        let root = tempfile::tempdir().expect("a scratch repository");
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).expect("a bin directory");
        repository(
            root.path(),
            "[tools]\n\"cargo:cargo-nextest\" = \"9.9.9\"\n",
        );
        let plugin = scripted(
            &bin,
            "cargo-nextest",
            "case \"$*\" in '--version') echo 'cargo-nextest 9.9.9 (stable)'; exit 0;; *) exit 9;; esac",
        );
        let mise = scripted(root.path(), "mise", &format!("echo '{}'", plugin.display()));
        let cargo = scripted(
            root.path(),
            "cargo",
            "printf '%s\\n' \"cargo $*\" >> \"$CALLS\"; exec \"$(dirname \"$0\")/bin/cargo-nextest\" \"${@:2}\"",
        );
        std::fs::create_dir_all(root.path().join("target")).expect("a place for the calls");
        let calls = root.path().join("target/calls");
        std::fs::write(&calls, "").expect("an empty record");
        let path = format!("{}:{}:/bin:/usr/bin", bin.display(), root.path().display());
        let environment = environment(&[
            ("PATH", path.as_str()),
            ("CALLS", &calls.display().to_string()),
        ]);
        let said = check(
            root.path(),
            mise.as_os_str(),
            cargo.as_os_str(),
            &environment,
        );
        assert!(
            said.is_ok(),
            "the pinned plugin answering through the protocol passes: {said:?}"
        );
        let made = std::fs::read_to_string(&calls).expect("the recorded invocations");
        assert_eq!(
            made, "cargo nextest --version\n",
            "the gate asks the plugin through cargo's external-subcommand protocol, never directly"
        );
    }

    #[test]
    fn an_earlier_executable_shadowing_the_pin_is_refused() {
        let root = tempfile::tempdir().expect("a scratch repository");
        let poisoned = root.path().join("poisoned");
        let pinned_dir = root.path().join("pinned");
        std::fs::create_dir_all(&poisoned).expect("a poisoned directory");
        std::fs::create_dir_all(&pinned_dir).expect("a pinned directory");
        repository(root.path(), "[tools]\n\"cargo:cargo-deny\" = \"1.2.3\"\n");
        scripted(
            &poisoned,
            "cargo-deny",
            "echo 'cargo-deny 0.0.0-poisoned'; exit 0",
        );
        let pinned = scripted(&pinned_dir, "cargo-deny", "echo 'cargo-deny 1.2.3'; exit 0");
        let mise = scripted(root.path(), "mise", &format!("echo '{}'", pinned.display()));
        let path = format!(
            "{}:{}:/bin:/usr/bin",
            poisoned.display(),
            pinned_dir.display()
        );
        let environment = environment(&[("PATH", path.as_str())]);
        let said = check(
            root.path(),
            mise.as_os_str(),
            std::ffi::OsStr::new("cargo"),
            &environment,
        );
        assert!(
            matches!(said, Err(super::ToolsError::Shadowed { ref shadow, .. }) if shadow.contains("poisoned/cargo-deny")),
            "the earlier executable is refused by name: {said:?}"
        );
    }

    #[test]
    fn a_wrong_version_answering_through_the_protocol_is_refused() {
        let root = tempfile::tempdir().expect("a scratch repository");
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).expect("a bin directory");
        repository(
            root.path(),
            "[tools]\n\"cargo:cargo-nextest\" = \"0.9.140\"\n",
        );
        let plugin = scripted(
            &bin,
            "cargo-nextest",
            "echo 'cargo-nextest 0.9.146'; exit 0",
        );
        let mise = scripted(root.path(), "mise", &format!("echo '{}'", plugin.display()));
        let cargo = scripted(
            root.path(),
            "cargo",
            "exec \"$(dirname \"$0\")/bin/cargo-nextest\" \"${@:2}\"",
        );
        let path = format!("{}:{}:/bin:/usr/bin", bin.display(), root.path().display());
        let environment = environment(&[("PATH", path.as_str())]);
        let said = check(
            root.path(),
            mise.as_os_str(),
            cargo.as_os_str(),
            &environment,
        );
        assert!(
            matches!(
                said,
                Err(super::ToolsError::WrongVersion {
                    ref answered,
                    ref pinned,
                    ..
                }) if answered == "0.9.146" && pinned == "0.9.140"
            ),
            "the version that answered is refused against the pin: {said:?}"
        );
    }

    #[test]
    fn an_unresolved_pin_is_refused_with_what_mise_said() {
        let root = tempfile::tempdir().expect("a scratch repository");
        std::fs::write(
            root.path().join("mise.toml"),
            "[tools]\n\"cargo:cargo-mutants\" = \"27.1.0\"\n",
        )
        .expect("the pins");
        let mise = scripted(
            root.path(),
            "mise",
            "echo 'mise ERROR cargo-mutants is not active here' >&2; exit 1",
        );
        let environment = environment(&[]);
        let said = check(
            root.path(),
            mise.as_os_str(),
            std::ffi::OsStr::new("cargo"),
            &environment,
        );
        assert!(
            matches!(said, Err(super::ToolsError::Unresolved { ref tool, .. }) if tool == "cargo-mutants"),
            "a pin mise cannot resolve is refused naming the tool: {said:?}"
        );
    }
}
