// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Mise selects gate executables while Cargo retains its external-subcommand protocol.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::environment::Environment;
use crate::work::{Bound, Ended, Stops, WorkError};

mod commandwork;
mod hostcost;

/// The actual cleanup recipient selected before any nested command can start.
#[derive(Debug)]
#[cfg_attr(not(unix), derive(Clone, Copy))]
pub(crate) enum Recipient {
    /// The existing command runner settles every producer itself.
    Direct,
    /// The acknowledged original native session retains naturally completed nested groups.
    #[cfg(unix)]
    Original {
        /// The verified original session and retained launching parent.
        parent: njutest_process::ParentSession,
    },
}

/// The actual destination of both child streams.
#[derive(Debug)]
pub(crate) enum Output {
    Inherited,
    Log(std::fs::File),
    Capture {
        stdout: std::fs::File,
        stderr: std::fs::File,
    },
}

/// The retained inputs to one existing owned command execution.
pub(crate) struct Request<'a, 'b> {
    pub(crate) command: &'a mut Command,
    pub(crate) bound: Option<&'a mut Bound<'b>>,
    pub(crate) stops: &'a Stops,
    pub(crate) environment: &'a Environment,
    pub(crate) output: Output,
}

impl std::fmt::Debug for Request<'_, '_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Request")
            .field("command", &self.command)
            .field("stops", &self.stops)
            .field("output", &self.output)
            .finish_non_exhaustive()
    }
}

pub(crate) fn run<F>(request: Request<'_, '_>, started: F) -> Result<Ended, WorkError>
where
    F: FnOnce(u32) -> std::io::Result<()>,
{
    commandwork::run(request, started)
}

/// Verifies the inherited original recipient before slot metadata or work can start.
pub(crate) fn session_recipient(
    environment: &Environment,
    stops: &Stops,
) -> std::io::Result<Recipient> {
    commandwork::recipient(environment, stops)
}

/// Runs with the actual native recipient already admitted by its original slot caller.
pub(crate) fn run_with_recipient<F>(
    request: Request<'_, '_>,
    started: F,
    recipient: Recipient,
) -> Result<Ended, WorkError>
where
    F: FnOnce(u32) -> std::io::Result<()>,
{
    commandwork::run_with_recipient(request, started, recipient)
}

pub(crate) fn capture(
    command: &mut Command,
    environment: &Environment,
) -> std::io::Result<std::process::Output> {
    commandwork::capture(command, environment)
}

pub(crate) fn log(path: &Path) -> std::io::Result<Output> {
    std::fs::File::create(path).map(Output::Log)
}

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
    #[error("{tool}: mise's relative selection {pinned} can be shadowed by {shadow}")]
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
/// Mise resolves each executable before Cargo asks it for its pinned version.
///
/// # Errors
/// The first pin whose selection, protocol or version this cannot establish.
pub fn check(
    root: &Path,
    mise: &OsStr,
    _cargo: &OsStr,
    environment: &Environment,
) -> Result<String, ToolsError> {
    let plugins = pins(root)?;
    for plugin in &plugins {
        let pinned = resolved(root, mise, &plugin.executable, environment)?;
        let cargo = resolved(root, mise, "cargo", environment)?;
        answered(
            Protocol {
                cargo: cargo.as_os_str(),
                environment,
                pinned: &pinned,
                toolchain: None,
            },
            plugin,
        )?;
    }
    let said = plugins
        .iter()
        .map(|plugin| format!("{} {}", plugin.executable, plugin.pinned))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "tools: {count} pinned cargo plugin(s) selected by mise and answering through Cargo's protocol: {said}",
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
    let table: BTreeMap<String, toml::Value> =
        toml::from_str(&text).map_err(|source| ToolsError::Parse {
            path: shown,
            source,
        })?;
    let tools = table
        .get("tools")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| ToolsError::Unresolved {
            tool: "mise tools".to_owned(),
            said: "mise.toml has no tools table".to_owned(),
        })?;
    tools
        .iter()
        .filter_map(|(held, value)| {
            let executable = held.strip_prefix("cargo:")?;
            let subcommand = executable.strip_prefix("cargo-")?;
            Some(
                value
                    .as_str()
                    .map(|pinned| Plugin {
                        executable: executable.to_owned(),
                        subcommand: subcommand.to_owned(),
                        pinned: pinned.to_owned(),
                    })
                    .ok_or_else(|| ToolsError::Unresolved {
                        tool: executable.to_owned(),
                        said: "the plugin has no exact textual pin".to_owned(),
                    }),
            )
        })
        .collect()
}

pub(crate) fn proof_paths(
    root: &Path,
    environment: &Environment,
) -> Result<Vec<PathBuf>, ToolsError> {
    let mut paths = vec![resolved(root, "mise".as_ref(), "cargo", environment)?];
    let text =
        std::fs::read_to_string(root.join("mise.toml")).map_err(|source| ToolsError::Read {
            path: root.join("mise.toml").display().to_string(),
            source,
        })?;
    let table: toml::Value = toml::from_str(&text).map_err(|source| ToolsError::Parse {
        path: root.join("mise.toml").display().to_string(),
        source,
    })?;
    let tools = table
        .get("tools")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| ToolsError::Unresolved {
            tool: "mise tools".to_owned(),
            said: "mise.toml has no complete tools table".to_owned(),
        })?;
    for tool in tools.keys() {
        let executable = match tool.as_str() {
            "rust" => "rustc",
            named => named.strip_prefix("cargo:").unwrap_or(named),
        };
        paths.push(resolved(root, "mise".as_ref(), executable, environment)?);
    }
    Ok(paths)
}

pub(crate) fn cargo(root: &Path, environment: &Environment) -> Result<PathBuf, ToolsError> {
    resolved(root, "mise".as_ref(), "cargo", environment)
}

/// The executable mise selects for `tool`, refused where mise cannot name one.
fn resolved(
    root: &Path,
    mise: &OsStr,
    tool: &str,
    environment: &Environment,
) -> Result<PathBuf, ToolsError> {
    let mut request = Command::new(mise);
    request
        .args(["which", tool])
        .current_dir(root)
        .envs(environment.pairs());
    let output = capture(&mut request, environment).map_err(|source| ToolsError::Start {
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
fn selected_path(
    tool: &str,
    environment: &Environment,
    pinned: &Path,
) -> Result<OsString, ToolsError> {
    if !pinned.is_absolute() {
        return Err(ToolsError::Shadowed {
            tool: tool.to_owned(),
            shadow: "the caller's working directory".to_owned(),
            pinned: pinned.display().to_string(),
        });
    }
    if !executable(pinned) {
        return Err(ToolsError::Unresolved {
            tool: tool.to_owned(),
            said: format!("{} is not executable", pinned.display()),
        });
    }
    let parent = pinned.parent().ok_or_else(|| ToolsError::Unresolved {
        tool: tool.to_owned(),
        said: "mise returned a path without a directory".to_owned(),
    })?;
    let mut parts = vec![parent.to_path_buf()];
    if let Some(path) = environment.value("PATH") {
        parts.extend(std::env::split_paths(path));
    }
    let home = environment
        .value("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            environment
                .value("HOME")
                .map(|home| Path::new(home).join(".cargo"))
        })
        .or_else(|| {
            environment
                .value("USERPROFILE")
                .map(|home| Path::new(home).join(".cargo"))
        })
        .ok_or_else(|| ToolsError::Unresolved {
            tool: tool.to_owned(),
            said: "Cargo home cannot be established for its subcommand search".to_owned(),
        })?;
    let bin = home.join("bin");
    if !parts.contains(&bin) {
        parts.push(bin);
    }
    std::env::join_paths(parts).map_err(|source| ToolsError::Unresolved {
        tool: tool.to_owned(),
        said: source.to_string(),
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
#[derive(Debug, Clone, Copy)]
struct Protocol<'a> {
    cargo: &'a OsStr,
    environment: &'a Environment,
    pinned: &'a Path,
    toolchain: Option<&'a str>,
}

fn answered(request: Protocol<'_>, plugin: &Plugin) -> Result<(), ToolsError> {
    let said = protocol(request, &plugin.executable, &plugin.subcommand)?;
    if first_version(&said) == plugin.pinned {
        return Ok(());
    }
    Err(ToolsError::WrongVersion {
        tool: plugin.executable.clone(),
        answered: first_version(&said),
        pinned: plugin.pinned.clone(),
    })
}

fn protocol(
    request: Protocol<'_>,
    executable: &str,
    subcommand: &str,
) -> Result<String, ToolsError> {
    let Protocol {
        cargo,
        environment,
        pinned,
        toolchain,
    } = request;
    let mut asking = Command::new(cargo);
    if let Some(toolchain) = toolchain {
        asking.arg(format!("+{toolchain}"));
    }
    asking.args([subcommand, "--version"]);
    asking.envs(environment.pairs());
    asking.env("PATH", selected_path(executable, environment, pinned)?);
    asking.env("CARGO", cargo);
    let output = capture(&mut asking, environment).map_err(|source| ToolsError::Start {
        program: pinned.display().to_string(),
        source,
    })?;
    let said = String::from_utf8(output.stdout).map_err(|_undecodable| ToolsError::NotText {
        program: executable.to_owned(),
    })?;
    if !output.status.success() {
        let mut complained =
            String::from_utf8(output.stderr).map_err(|_undecodable| ToolsError::NotText {
                program: executable.to_owned(),
            })?;
        complained.push_str(&said);
        return Err(ToolsError::Protocol {
            tool: executable.to_owned(),
            subcommand: subcommand.to_owned(),
            said: complained.trim_end().to_owned(),
        });
    }
    if said.trim().is_empty() {
        return Err(ToolsError::Protocol {
            tool: executable.to_owned(),
            subcommand: subcommand.to_owned(),
            said: "the selected executable returned an empty version answer".to_owned(),
        });
    }
    Ok(said)
}

/// Selects and validates an actual pinned Cargo plugin command for a gate or task.
///
/// # Errors
/// The command is not pinned, mise cannot resolve it, or the protocol/version control fails.
pub fn command(
    root: &Path,
    mise: &OsStr,
    args: &[OsString],
    environment: &Environment,
) -> Result<Command, ToolsError> {
    let (toolchain, arguments) = requested_toolchain(root, args)?;
    let Some(subcommand) = arguments.first().and_then(|name| name.to_str()) else {
        return Err(ToolsError::Unresolved {
            tool: "cargo plugin".to_owned(),
            said: "a UTF-8 subcommand is required".to_owned(),
        });
    };
    let cargo = resolved(root, mise, "cargo", environment)?;
    let (executable, pinned) = if subcommand == "miri" {
        let channel = toolchain.as_deref().ok_or_else(|| ToolsError::Unresolved {
            tool: "cargo-miri".to_owned(),
            said: "Miri requires the explicitly pinned nightly toolchain".to_owned(),
        })?;
        let pinned = component(
            Site {
                root,
                mise,
                environment,
            },
            channel,
            "cargo-miri",
        )?;
        protocol(
            Protocol {
                cargo: cargo.as_os_str(),
                environment,
                pinned: &pinned,
                toolchain: Some(channel),
            },
            "cargo-miri",
            subcommand,
        )?;
        ("cargo-miri".to_owned(), pinned)
    } else {
        let plugin = pins(root)?
            .into_iter()
            .find(|plugin| plugin.subcommand == subcommand)
            .ok_or_else(|| ToolsError::Unresolved {
                tool: subcommand.to_owned(),
                said: "this command has no mise pin".to_owned(),
            })?;
        let pinned = resolved(root, mise, &plugin.executable, environment)?;
        answered(
            Protocol {
                cargo: cargo.as_os_str(),
                environment,
                pinned: &pinned,
                toolchain: toolchain.as_deref(),
            },
            &plugin,
        )?;
        (plugin.executable, pinned)
    };
    let mut command = Command::new(&cargo);
    command
        .args(args)
        .current_dir(root)
        .envs(environment.pairs())
        .env("PATH", selected_path(&executable, environment, &pinned)?)
        .env("CARGO", &cargo);
    Ok(command)
}

fn requested_toolchain<'a>(
    root: &Path,
    args: &'a [OsString],
) -> Result<(Option<String>, &'a [OsString]), ToolsError> {
    let Some(requested) = args
        .first()
        .and_then(|value| value.to_str())
        .and_then(|value| value.strip_prefix('+'))
    else {
        return Ok((None, args));
    };
    let path = root.join("rust-toolchain.toml");
    let text = std::fs::read_to_string(&path).map_err(|source| ToolsError::Read {
        path: path.display().to_string(),
        source,
    })?;
    let table: toml::Value = toml::from_str(&text).map_err(|source| ToolsError::Parse {
        path: path.display().to_string(),
        source,
    })?;
    let channel = table
        .get("njutest")
        .and_then(|held| held.get("nightly"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| ToolsError::Unresolved {
            tool: "nightly".to_owned(),
            said: "rust-toolchain.toml has no exact nightly pin".to_owned(),
        })?;
    if requested != channel {
        return Err(ToolsError::Unresolved {
            tool: requested.to_owned(),
            said: format!("rust-toolchain.toml requires {channel}"),
        });
    }
    let arguments = args.get(1..).ok_or_else(|| ToolsError::Unresolved {
        tool: requested.to_owned(),
        said: "the requested toolchain has no subcommand".to_owned(),
    })?;
    Ok((Some(channel.to_owned()), arguments))
}

#[derive(Debug, Clone, Copy)]
struct Site<'a> {
    root: &'a Path,
    mise: &'a OsStr,
    environment: &'a Environment,
}

fn component(site: Site<'_>, channel: &str, executable: &str) -> Result<PathBuf, ToolsError> {
    let Site {
        root,
        mise,
        environment,
    } = site;
    let rustup = resolved(root, mise, "rustup", environment)?;
    let mut request = Command::new(rustup);
    request
        .args(["which", "--toolchain", channel, executable])
        .current_dir(root)
        .envs(environment.pairs());
    let output = capture(&mut request, environment).map_err(|source| ToolsError::Start {
        program: executable.to_owned(),
        source,
    })?;
    if !output.status.success() {
        return Err(ToolsError::Unresolved {
            tool: executable.to_owned(),
            said: String::from_utf8(output.stderr).map_err(|_undecodable| ToolsError::NotText {
                program: executable.to_owned(),
            })?,
        });
    }
    let path = String::from_utf8(output.stdout).map_err(|_undecodable| ToolsError::NotText {
        program: executable.to_owned(),
    })?;
    Ok(PathBuf::from(path.trim_end()))
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
    use std::path::{Path, PathBuf};

    fn scripted(root: &Path, name: &str, body: &str) -> PathBuf {
        let script = format!("#!/usr/bin/env sh\nset -eu\n{body}\n");
        script_program(root, name, &script)
    }

    #[cfg(unix)]
    fn script_program(root: &Path, name: &str, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let path = root.join(name);
        std::fs::write(&path, script).expect("the scripted program");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("the executable script");
        path
    }

    #[cfg(windows)]
    fn script_program(root: &Path, name: &str, script: &str) -> PathBuf {
        let driver = script_driver(root);
        let program = root.join(name).with_extension("exe");
        std::fs::copy(driver, &program).expect("the native script bridge");
        std::fs::write(program.with_extension("script"), script).expect("the fixture script");
        program
    }

    #[cfg(windows)]
    fn script_driver(root: &Path) -> PathBuf {
        let executable = root.join("fixture-script-driver.exe");
        if executable
            .try_exists()
            .expect("the driver can be inspected")
        {
            return executable;
        }
        let shell = njutest_devkit::paths::posix_sh();
        let shell = serde_json::to_string(shell.to_str().expect("the Git shell path"))
            .expect("a Rust string literal");
        let source = root.join("fixture-script-driver.rs");
        let body = format!(
            r#"fn main() {{
    let script = std::env::current_exe().expect("this bridge").with_extension("script");
    let status = std::process::Command::new({shell})
        .arg(script).args(std::env::args_os().skip(1)).status().expect("the fixture shell");
    std::process::exit(status.code().expect("the shell status"));
}}
"#
        );
        std::fs::write(&source, body).expect("the real native bridge source");
        let cargo = njutest_devkit::paths::cargo_binary();
        let compiler = cargo
            .parent()
            .expect("the pinned Cargo directory")
            .join("rustc.exe");
        let mut command = std::process::Command::new(compiler);
        command
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .current_dir(root);
        let built = njutest_devkit::cost::probe(
            command,
            njutest_devkit::cost::ProbeRole::RustcBuild,
            "native Cargo protocol fixture",
        )
        .expect("the observed native bridge compilation");
        assert!(
            built.status.success(),
            "the native bridge compiles: {built:?}"
        );
        executable
    }

    fn search_path(directories: &[&Path]) -> String {
        let host = std::env::var_os("PATH").expect("the host PATH");
        std::env::join_paths(
            directories
                .iter()
                .map(|path| path.to_path_buf())
                .chain(std::env::split_paths(&host)),
        )
        .expect("the fixture search path")
        .into_string()
        .expect("the test PATH is UTF-8")
    }

    fn environment(pairs: &[(&str, &str)]) -> Environment {
        Environment::of(
            std::env::vars_os()
                .filter(|(name, _value)| !pairs.iter().any(|(replaced, _new)| name == replaced))
                .chain(
                    pairs
                        .iter()
                        .map(|(name, value)| (OsString::from(*name), OsString::from(*value))),
                ),
        )
    }

    fn repository(root: &Path, table: &str) {
        std::fs::write(root.join("mise.toml"), table).expect("the pins");
    }

    fn selector(root: &Path, plugin: &Path) -> PathBuf {
        let cargo = njutest_devkit::paths::cargo_binary();
        scripted(
            root,
            "mise",
            &format!(
                "case \"$*\" in 'which cargo') echo '{}';; 'which cargo-'*) echo '{}';; *) exit 99;; esac",
                cargo.display(),
                plugin.display(),
            ),
        )
    }

    #[test]
    fn the_pinned_plugin_answering_through_the_protocol_passes() {
        let root = tempfile::tempdir().expect("a scratch repository");
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).expect("the plugin directory");
        repository(
            root.path(),
            "[tools]\n\"cargo:cargo-nextest\" = \"9.9.9\"\n",
        );
        let plugin = scripted(
            &bin,
            "cargo-nextest",
            "test \"$1\" = nextest; test \"$2\" = --version; test -n \"$CARGO\"; printf '%s\n' \"$*\" > \"$CALLS\"; echo 'cargo-nextest 9.9.9'",
        );
        let mise = selector(root.path(), &plugin);
        let calls = root.path().join("calls");
        let environment = environment(&[("CALLS", &calls.display().to_string())]);
        let cargo = njutest_devkit::paths::cargo_binary();
        let result = check(
            root.path(),
            mise.as_os_str(),
            cargo.as_os_str(),
            &environment,
        );
        assert!(
            result.is_ok(),
            "the actual Cargo protocol passes: {result:?}"
        );
        assert_eq!(
            std::fs::read_to_string(calls).expect("the actual plugin arguments"),
            "nextest --version\n"
        );
    }

    #[test]
    fn an_earlier_executable_does_not_replace_the_selected_pin() {
        let root = tempfile::tempdir().expect("a scratch repository");
        let poisoned = root.path().join("poisoned");
        let pinned = root.path().join("pinned");
        for directory in [&poisoned, &pinned] {
            std::fs::create_dir_all(directory).expect("a plugin directory");
        }
        repository(root.path(), "[tools]\n\"cargo:cargo-deny\" = \"1.2.3\"\n");
        scripted(&poisoned, "cargo-deny", "echo 'cargo-deny 0.0.0-poisoned'");
        let plugin = scripted(
            &pinned,
            "cargo-deny",
            "test \"$1\" = deny; echo 'cargo-deny 1.2.3'",
        );
        let mise = selector(root.path(), &plugin);
        let path = search_path(&[&poisoned, &pinned]);
        let environment = environment(&[("PATH", &path)]);
        let cargo = njutest_devkit::paths::cargo_binary();
        let result = check(
            root.path(),
            mise.as_os_str(),
            cargo.as_os_str(),
            &environment,
        );
        assert!(
            result.is_ok(),
            "the earlier plugin cannot replace the pin: {result:?}"
        );
    }

    #[test]
    fn a_wrong_version_answering_through_the_protocol_is_refused() {
        let root = tempfile::tempdir().expect("a scratch repository");
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).expect("the plugin directory");
        repository(
            root.path(),
            "[tools]\n\"cargo:cargo-nextest\" = \"0.9.140\"\n",
        );
        let plugin = scripted(&bin, "cargo-nextest", "echo 'cargo-nextest 0.9.146'");
        let mise = selector(root.path(), &plugin);
        let environment = environment(&[]);
        let cargo = njutest_devkit::paths::cargo_binary();
        let result = check(
            root.path(),
            mise.as_os_str(),
            cargo.as_os_str(),
            &environment,
        );
        assert!(
            matches!(result, Err(super::ToolsError::WrongVersion {
            ref answered, ref pinned, ..
        }) if answered == "0.9.146" && pinned == "0.9.140"),
            "the actual protocol answer is checked against its pin: {result:?}"
        );
    }

    #[test]
    fn an_unresolved_pin_is_refused_with_what_mise_said() {
        let root = tempfile::tempdir().expect("a scratch repository");
        repository(
            root.path(),
            "[tools]\n\"cargo:cargo-mutants\" = \"27.1.0\"\n",
        );
        let mise = scripted(
            root.path(),
            "mise",
            "echo 'cargo-mutants is not active here' >&2; exit 1",
        );
        let result = check(
            root.path(),
            mise.as_os_str(),
            "cargo".as_ref(),
            &environment(&[]),
        );
        assert!(
            matches!(result, Err(super::ToolsError::Unresolved { ref tool, .. })
            if tool == "cargo-mutants"),
            "the missing pin is named: {result:?}"
        );
    }

    #[test]
    fn the_real_cargo_protocol_selects_the_pin_over_path_and_cargo_home() {
        let root = tempfile::tempdir().expect("a scratch repository");
        let poisoned = root.path().join("poisoned");
        let pinned = root.path().join("pinned");
        let home = root.path().join("cargo-home");
        for directory in [&poisoned, &pinned, &home.join("bin")] {
            std::fs::create_dir_all(directory).expect("a plugin directory");
        }
        repository(
            root.path(),
            "[tools]\n\"cargo:cargo-nextest\" = \"9.9.9\"\n",
        );
        let plugin = scripted(
            &pinned,
            "cargo-nextest",
            "test \"$1\" = nextest; test -n \"$CARGO\"; echo 'cargo-nextest 9.9.9'",
        );
        scripted(
            &poisoned,
            "cargo-nextest",
            "echo 'cargo-nextest 0.0.0-path'",
        );
        scripted(
            &home.join("bin"),
            "cargo-nextest",
            "echo 'cargo-nextest 0.0.0-home'",
        );
        let wrong_cargo = scripted(&poisoned, "cargo", "echo 'cargo-nextest 0.0.0-cargo'");
        let mise = selector(root.path(), &plugin);
        let path = search_path(&[&poisoned, &pinned]);
        let environment =
            environment(&[("PATH", &path), ("CARGO_HOME", &home.display().to_string())]);
        let result = check(
            root.path(),
            mise.as_os_str(),
            wrong_cargo.as_os_str(),
            &environment,
        );
        assert!(
            result.is_ok(),
            "mise selects the actual Cargo and plugin despite all shadows: {result:?}"
        );
    }
}
