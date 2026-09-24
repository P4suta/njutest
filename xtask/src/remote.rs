// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The whole suite of this commit, run on the other machines before it is pushed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread::JoinHandle;

use thiserror::Error;

/// Why a commit could not be put to the other machines, or what they said about it.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RemoteError {
    /// The machines file could not be read.
    #[error("{path}: {source}")]
    Read {
        /// The unreadable file.
        path: String,
        /// The filesystem failure.
        source: std::io::Error,
    },
    /// The machines file was not the shape this command reads.
    #[error("{path}: {source}")]
    Parse {
        /// The malformed file.
        path: String,
        /// The TOML failure.
        source: toml::de::Error,
    },
    /// The machines file named no machine.
    #[error("{path} names no machine")]
    NoMachine {
        /// The empty file.
        path: String,
    },
    /// A local program could not be started.
    #[error("could not start {program}: {source}")]
    Start {
        /// The program.
        program: String,
        /// The operating system's answer.
        source: std::io::Error,
    },
    /// A local git step refused.
    #[error("git {step} failed")]
    Git {
        /// Which step.
        step: &'static str,
    },
    /// Git answered with something that is not text.
    #[error("git {step} answered with bytes that are not UTF-8")]
    NotText {
        /// Which step.
        step: &'static str,
    },
    /// A log could not be kept.
    #[error("could not keep the log {path}: {source}")]
    Log {
        /// The log.
        path: String,
        /// The filesystem failure.
        source: std::io::Error,
    },
    /// The thread asking one machine ended without an answer.
    #[error("the thread asking {machine} ended without an answer")]
    Lost {
        /// Which machine.
        machine: String,
    },
    /// At least one machine refused the commit.
    #[error("{report}")]
    Refused {
        /// Every machine's answer.
        report: String,
    },
}

/// How a machine is spoken to once it is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Shell {
    /// A POSIX login shell.
    Posix,
    /// `PowerShell`.
    Powershell,
}

/// One machine the suite is put to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    /// What reports call it.
    pub name: String,
    /// What `ssh` and `scp` reach it by.
    pub host: String,
    /// How it is spoken to.
    pub shell: Shell,
    /// The clone a gate worktree is added from.
    pub repository: String,
    /// The worktree the commit is checked out in, kept between runs so its build stays warm.
    pub worktree: String,
    /// Where the build goes, where the machine wants it somewhere else.
    #[serde(default)]
    pub target_dir: Option<String>,
    /// Lines run before anything else, so the machine's session looks like the one CI runs in.
    #[serde(default)]
    pub prelude: Option<String>,
}

/// The machines and what each is asked to run.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fleet {
    /// The command each machine runs in the worktree.
    pub command: String,
    /// The machines.
    #[serde(rename = "machine")]
    pub members: Vec<Machine>,
}

/// Reads the machines file at `path`.
///
/// # Errors
/// Returns the read or parse failure, or [`RemoteError::NoMachine`] for a file that names none.
pub fn fleet(path: &Path) -> Result<Fleet, RemoteError> {
    let shown = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|source| RemoteError::Read {
        path: shown.clone(),
        source,
    })?;
    let parsed: Fleet = toml::from_str(&text).map_err(|source| RemoteError::Parse {
        path: shown.clone(),
        source,
    })?;
    if parsed.members.is_empty() {
        return Err(RemoteError::NoMachine { path: shown });
    }
    Ok(parsed)
}

/// The name the bundle has in each machine's home directory.
pub const BUNDLE: &str = "njutest-remote-check.bundle";

/// The machine's own prelude, ending in a newline where there is one.
fn prelude(machine: &Machine) -> String {
    machine
        .prelude
        .as_ref()
        .map_or_else(String::new, |lines| format!("{}\n", lines.trim_end()))
}

/// What `machine` is told to run to check out `sha` and put the suite to it.
#[must_use]
pub fn script(fleet: &Fleet, machine: &Machine, sha: &str) -> String {
    match machine.shell {
        Shell::Posix => {
            let target = machine.target_dir.as_ref().map_or_else(String::new, |dir| {
                format!("export CARGO_TARGET_DIR={dir}\n")
            });
            format!(
                "set -e\n{prelude}cd {repository}\n[ -d {worktree} ] || git worktree add -q --detach {worktree} HEAD\n\
                 cd {worktree}\ngit fetch -q ~/{BUNDLE} HEAD\n\
                 git checkout -q --detach {sha}\nmise trust -q . >/dev/null 2>&1 || true\n{target}{command}\n",
                repository = machine.repository,
                worktree = machine.worktree,
                command = fleet.command,
                prelude = prelude(machine),
            )
        }
        Shell::Powershell => {
            let target = machine.target_dir.as_ref().map_or_else(String::new, |dir| {
                format!("$env:CARGO_TARGET_DIR = '{dir}'\n")
            });
            format!(
                "$ErrorActionPreference = 'Stop'\n$PSNativeCommandUseErrorActionPreference = $true\n{prelude}\
                 Set-Location '{repository}'\nif (-not (Test-Path '{worktree}')) {{ git worktree add -q --detach '{worktree}' HEAD }}\n\
                 Set-Location '{worktree}'\ngit fetch -q (Join-Path $HOME '{BUNDLE}') HEAD\n\
                 git checkout -q --detach {sha}\n$PSNativeCommandUseErrorActionPreference = $false\nmise trust -q . *> $null\n\
                 $PSNativeCommandUseErrorActionPreference = $true\n{target}{command}\n",
                repository = machine.repository,
                worktree = machine.worktree,
                command = fleet.command,
                prelude = prelude(machine),
            )
        }
    }
}

/// What `machine` is asked so the bundle carries only the commits its clone lacks: every commit its references name, one per line.
#[must_use]
pub fn known(machine: &Machine) -> String {
    match machine.shell {
        Shell::Posix => format!(
            "git -C {} for-each-ref --format='%(objectname)'\n",
            machine.repository
        ),
        Shell::Powershell => format!(
            "git -C '{}' for-each-ref --format='%(objectname)'\n",
            machine.repository
        ),
    }
}

/// The remote command line that runs `script` on a machine spoken to by `shell`, with nothing in it a shell could reinterpret.
#[must_use]
pub fn invocation(shell: Shell, script: &str) -> String {
    match shell {
        Shell::Posix => format!("echo {} | base64 -d | bash -l", base64(script.as_bytes())),
        Shell::Powershell => {
            let wide: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
            format!(
                "pwsh -NoProfile -NonInteractive -EncodedCommand {}",
                base64(&wide)
            )
        }
    }
}

/// The base64 digit for the low six bits of `value`.
fn digit(value: u32) -> char {
    const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let at = match usize::try_from(value & 0x3f) {
        Ok(at) => at,
        Err(_six_bits_always_fit) => return '=',
    };
    match DIGITS.get(at) {
        Some(byte) => char::from(*byte),
        None => '=',
    }
}

/// Standard base64 with padding.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3).saturating_mul(4));
    for chunk in bytes.chunks(3) {
        let (joined, kept) = match *chunk {
            [first, second, third] => (
                (u32::from(first) << 16) | (u32::from(second) << 8) | u32::from(third),
                4,
            ),
            [first, second] => ((u32::from(first) << 16) | (u32::from(second) << 8), 3),
            [first] => (u32::from(first) << 16, 2),
            _ => continue,
        };
        for (index, shift) in [18_u32, 12, 6, 0].into_iter().enumerate() {
            encoded.push(if index < kept {
                digit(joined >> shift)
            } else {
                '='
            });
        }
    }
    encoded
}

/// The lines of a suite's output that say what failed, each once, in the order they came.
#[must_use]
pub fn failures(log: &str) -> Vec<String> {
    let mut said: Vec<String> = Vec::new();
    for line in log.lines() {
        let trimmed = line.trim();
        let failed = trimmed.starts_with("FAIL [")
            || trimmed.starts_with("TIMEOUT [")
            || trimmed.starts_with("error[")
            || trimmed.starts_with("error:")
            || trimmed.contains("panicked at");
        if failed && !said.iter().any(|seen| seen == trimmed) {
            said.push(trimmed.to_owned());
        }
    }
    said
}

/// What one machine said about the commit.
#[derive(Debug)]
pub struct Answer {
    /// Which machine.
    pub machine: String,
    /// Whether every step and the suite passed.
    pub passed: bool,
    /// The failing lines, when it did not.
    pub failures: Vec<String>,
    /// Where its whole output is kept.
    pub log: PathBuf,
}

/// One commit put to one machine.
#[derive(Debug, Clone)]
struct Asked {
    fleet: Fleet,
    machine: Machine,
    sha: String,
    root: PathBuf,
    logs: PathBuf,
}

fn run(program: &str, arguments: &[&str], directory: &Path) -> Result<Output, RemoteError> {
    Command::new(program)
        .args(arguments)
        .current_dir(directory)
        .output()
        .map_err(|source| RemoteError::Start {
            program: program.to_owned(),
            source,
        })
}

fn git(directory: &Path, step: &'static str, arguments: &[&str]) -> Result<String, RemoteError> {
    let output = run("git", arguments, directory)?;
    if !output.status.success() {
        return Err(RemoteError::Git { step });
    }
    match String::from_utf8(output.stdout) {
        Ok(text) => Ok(text.trim().to_owned()),
        Err(_not_text) => Err(RemoteError::NotText { step }),
    }
}

/// The failing lines of `said`, or its last lines where it is not text or names no failure.
fn read_back(said: &[u8]) -> Vec<String> {
    let Ok(text) = std::str::from_utf8(said) else {
        return vec!["the output is not UTF-8; read the whole log".to_owned()];
    };
    let failing = failures(text);
    if !failing.is_empty() {
        return failing;
    }
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(5);
    lines
        .get(from..)
        .unwrap_or_default()
        .iter()
        .map(|line| (*line).to_owned())
        .collect()
}

/// The commits among `answer` that this clone also has, so a bundle may assume them.
fn shared(root: &Path, answer: &[u8]) -> Vec<String> {
    let Ok(text) = std::str::from_utf8(answer) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| line.len() == 40 && line.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .filter(|sha| {
            run(
                "git",
                &["cat-file", "-e", &format!("{sha}^{{commit}}")],
                root,
            )
            .is_ok_and(|output| output.status.success())
        })
        .map(ToOwned::to_owned)
        .collect()
}

fn ask(asked: &Asked) -> Result<Answer, RemoteError> {
    let log = asked.logs.join(format!("{}.log", asked.machine.name));
    let queried = run(
        "ssh",
        &[
            &asked.machine.host,
            &invocation(asked.machine.shell, &known(&asked.machine)),
        ],
        &asked.root,
    )?;
    let assumed = shared(&asked.root, &queried.stdout);
    let bundle = asked.logs.join(format!("{}.bundle", asked.machine.name));
    let bundle_path = bundle
        .as_os_str()
        .to_str()
        .map_or_else(String::new, ToOwned::to_owned);
    let mut arguments = vec!["bundle", "create", "-q", bundle_path.as_str(), "HEAD"];
    if !assumed.is_empty() {
        arguments.push("--not");
        arguments.extend(assumed.iter().map(String::as_str));
    }
    git(&asked.root, "bundle", &arguments)?;
    let destination = format!("{}:{BUNDLE}", asked.machine.host);
    let copied = run("scp", &["-q", &bundle_path, &destination], &asked.logs)?;
    let mut said = copied.stdout;
    said.extend_from_slice(&copied.stderr);
    let passed = if copied.status.success() {
        let line = invocation(
            asked.machine.shell,
            &script(&asked.fleet, &asked.machine, &asked.sha),
        );
        let ran = run("ssh", &[&asked.machine.host, &line], &asked.logs)?;
        said.extend_from_slice(&ran.stdout);
        said.extend_from_slice(&ran.stderr);
        ran.status.success()
    } else {
        false
    };
    std::fs::write(&log, &said).map_err(|source| RemoteError::Log {
        path: log.display().to_string(),
        source,
    })?;
    Ok(Answer {
        machine: asked.machine.name.clone(),
        passed,
        failures: if passed { Vec::new() } else { read_back(&said) },
        log,
    })
}

/// The thread asking one machine, joined before its owner is gone.
struct Asking {
    machine: String,
    handle: Option<JoinHandle<Result<Answer, RemoteError>>>,
}

impl Asking {
    fn start(asked: Asked) -> Self {
        let machine = asked.machine.name.clone();
        Self {
            machine,
            handle: Some(std::thread::spawn(move || ask(&asked))),
        }
    }

    fn finish(mut self) -> Result<Answer, RemoteError> {
        let lost = || RemoteError::Lost {
            machine: self.machine.clone(),
        };
        match self.handle.take() {
            Some(handle) => match handle.join() {
                Ok(answer) => answer,
                Err(_panicked) => Err(lost()),
            },
            None => Err(lost()),
        }
    }
}

impl Drop for Asking {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            std::process::abort();
        }
    }
}

/// Puts the commit checked out at `root` to every machine in the file at `machines`, all at once, and says what each found.
///
/// # Errors
/// Returns why the commit could not be put, or [`RemoteError::Refused`] naming every machine that refused it and what failed there.
pub fn check(root: &Path, machines: &Path) -> Result<String, RemoteError> {
    let fleet = fleet(machines)?;
    let sha = git(root, "rev-parse", &["rev-parse", "HEAD"])?;
    let short: String = sha.chars().take(8).collect();
    let logs = root.join("target").join("remote-check").join(&short);
    std::fs::create_dir_all(&logs).map_err(|source| RemoteError::Log {
        path: logs.display().to_string(),
        source,
    })?;
    let asking: Vec<Asking> = fleet
        .members
        .iter()
        .map(|machine| {
            Asking::start(Asked {
                fleet: fleet.clone(),
                machine: machine.clone(),
                sha: sha.clone(),
                root: root.to_path_buf(),
                logs: logs.clone(),
            })
        })
        .collect();
    let mut report: Vec<String> = Vec::new();
    let mut refused = false;
    for one in asking {
        let answer = one.finish()?;
        if answer.passed {
            report.push(format!("remote-check {short}: {} passed", answer.machine));
        } else {
            refused = true;
            report.push(format!(
                "remote-check {short}: {} FAILED (whole output in {})",
                answer.machine,
                answer.log.display()
            ));
            report.extend(
                answer
                    .failures
                    .iter()
                    .map(|line| format!("  {}: {line}", answer.machine)),
            );
        }
    }
    let report = report.join("\n");
    if refused {
        return Err(RemoteError::Refused { report });
    }
    Ok(report)
}
