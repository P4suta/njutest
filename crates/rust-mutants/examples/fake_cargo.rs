// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A cargo, a rustc, a coverage tool, or a test binary that says what a script told it to say.

#![expect(
    clippy::print_stderr,
    reason = "printing on the standard streams is what this program is: it stands in for a tool \
              whose whole answer is what it prints and what it exits with"
)]

use std::ffi::OsString;
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use njutest_devkit::fake_cargo::{Invocation, SCRIPT_ENV, Script, UNMATCHED_EXIT};
use rust_mutants::telling::LosslessBytes;

/// The command this process was started as: the name it answers to, and what came after it.
struct Started {
    program: String,
    args: Vec<String>,
}

#[derive(Debug)]
enum StartedError {
    NonUtf8Argument(OsString),
    ProgramStem,
}

impl fmt::Display for StartedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonUtf8Argument(argument) => write!(
                formatter,
                "a command-line argument is not valid UTF-8: {}",
                LosslessBytes::new(argument.as_encoded_bytes())
            ),
            Self::ProgramStem => formatter.write_str("the program name has no exact UTF-8 stem"),
        }
    }
}

impl std::error::Error for StartedError {}

#[derive(Debug, thiserror::Error)]
enum AnswerLogError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: line {line} is not an answer index: {source}")]
    InvalidIndex {
        path: String,
        line: usize,
        #[source]
        source: std::num::ParseIntError,
    },
    #[error("the answer log has more lines than usize can represent")]
    LineCountOverflow,
}

#[derive(Debug, thiserror::Error)]
enum ExpandError {
    #[error("the script directory {directory:?} is not valid UTF-8")]
    ScriptDirectoryNotUtf8 { directory: PathBuf },
    #[error("cannot read cwd: {source}")]
    CurrentDirectory {
        #[source]
        source: std::io::Error,
    },
    #[error("the current directory {directory:?} is not valid UTF-8")]
    CurrentDirectoryNotUtf8 { directory: PathBuf },
    #[error("the system clock precedes the Unix epoch: {source}")]
    Clock {
        #[source]
        source: std::time::SystemTimeError,
    },
}

fn main() -> ExitCode {
    let started = match started() {
        Ok(started) => started,
        Err(why) => {
            eprintln!("fake-cargo: {why}");
            return ExitCode::from(UNMATCHED_EXIT);
        }
    };
    let (program, args) = (&started.program, &started.args);
    let Ok(script_path) = std::env::var(SCRIPT_ENV) else {
        return refuse(program, args, format!("{SCRIPT_ENV} is not set"));
    };
    let Ok(text) = std::fs::read_to_string(&script_path) else {
        return refuse(program, args, format!("{script_path} cannot be read"));
    };
    let Ok(script) = njutest_devkit::strictjson::decode_str::<Script>(&text) else {
        return refuse(program, args, format!("{script_path} is not the script"));
    };
    let answered = match answered_before(&script_path) {
        Ok(answered) => answered,
        Err(why) => return refuse(program, args, why),
    };
    let Some((index, entry)) = script
        .invocations
        .iter()
        .enumerate()
        .find(|(index, entry)| matches(entry, &started, *index, &answered))
    else {
        return refuse(program, args, "no entry of the script matches");
    };
    if let Err(error) = record(&script_path, index) {
        return refuse(program, args, format!("cannot record the answer: {error}"));
    }
    answer(entry, &script_path, &started)
}

fn started() -> Result<Started, StartedError> {
    let command_line: Result<Vec<String>, StartedError> = std::env::args_os()
        .map(|argument| {
            argument
                .into_string()
                .map_err(StartedError::NonUtf8Argument)
        })
        .collect();
    let mut command_line = command_line?;
    let program = match command_line.first() {
        Some(first) => Path::new(first)
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or(StartedError::ProgramStem)?
            .to_owned(),
        None => String::new(),
    };
    let args = command_line.drain(1..).collect();
    Ok(Started { program, args })
}

/// Whether this command is the one `entry` answers: the same program, those arguments in front, that environment, and a turn still left.
fn matches(entry: &Invocation, started: &Started, index: usize, answered: &[usize]) -> bool {
    let prefix_matches = started.args.len() >= entry.args_prefix.len()
        && entry
            .args_prefix
            .iter()
            .zip(&started.args)
            .all(|(want, got)| want == got);
    let environment_matches = entry
        .env_has
        .iter()
        .all(|name| std::env::var_os(name).is_some())
        && !entry
            .env_lacks
            .iter()
            .any(|name| std::env::var_os(name).is_some())
        && entry
            .env_is
            .iter()
            .all(|(name, value)| match std::env::var(name) {
                Ok(actual) => &actual == value,
                Err(_) => false,
            });
    let turns_left = entry.times.is_none_or(|times| {
        let used = answered.iter().filter(|seen| **seen == index).count();
        u32::try_from(used).is_ok_and(|used| used < times)
    });
    entry.program == started.program && prefix_matches && environment_matches && turns_left
}

/// Does what the entry says, in the order a tool would: the files first, then the wait, then what it printed, then what it exits with.
fn answer(entry: &Invocation, script_path: &str, started: &Started) -> ExitCode {
    let live = match live_marker(script_path) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("fake-cargo: cannot create the live marker: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(exit) = write_entry_files(entry, script_path, started) {
        return exit;
    }
    if entry.delay_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(entry.delay_ms));
    }
    let stdout = match stdout_for(entry, script_path, started) {
        Ok(stdout) => stdout,
        Err(exit) => return exit,
    };
    if !stdout.is_empty() {
        let mut out = std::io::stdout();
        if let Err(error) = out.write_all(&stdout).and_then(|()| out.flush()) {
            eprintln!("fake-cargo: cannot write stdout: {error}");
            return ExitCode::FAILURE;
        }
    }
    let stderr = match expand(&entry.stderr, script_path, started) {
        Ok(stderr) => stderr,
        Err(why) => {
            eprintln!("fake-cargo: cannot expand stderr: {why}");
            return ExitCode::FAILURE;
        }
    };
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }
    if let Err(error) = std::fs::remove_file(&live) {
        eprintln!(
            "fake-cargo: cannot remove {}: {error}",
            LosslessBytes::new(live.as_os_str().as_encoded_bytes())
        );
        return ExitCode::FAILURE;
    }
    ExitCode::from(entry.exit)
}

/// Writes every file one scripted invocation creates before producing output.
fn write_entry_files(
    entry: &Invocation,
    script_path: &str,
    started: &Started,
) -> Result<(), ExitCode> {
    for write in &entry.writes {
        let path = match expand(&write.path, script_path, started) {
            Ok(path) => PathBuf::from(path),
            Err(why) => {
                eprintln!("fake-cargo: cannot expand a written path: {why}");
                return Err(ExitCode::FAILURE);
            }
        };
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            eprintln!(
                "fake-cargo: cannot create {}: {error}",
                LosslessBytes::new(parent.as_os_str().as_encoded_bytes())
            );
            return Err(ExitCode::FAILURE);
        }
        let contents = match expand(&write.contents, script_path, started) {
            Ok(contents) => contents,
            Err(why) => {
                eprintln!("fake-cargo: cannot expand written contents: {why}");
                return Err(ExitCode::FAILURE);
            }
        };
        if let Err(error) = std::fs::write(&path, contents) {
            eprintln!(
                "fake-cargo: cannot write {}: {error}",
                LosslessBytes::new(path.as_os_str().as_encoded_bytes())
            );
            return Err(ExitCode::FAILURE);
        }
    }
    Ok(())
}

/// Reads or expands the bytes one scripted invocation writes to stdout.
fn stdout_for(
    entry: &Invocation,
    script_path: &str,
    started: &Started,
) -> Result<Vec<u8>, ExitCode> {
    match &entry.stdout_file {
        None => expand(&entry.stdout, script_path, started)
            .map(String::into_bytes)
            .map_err(|why| {
                eprintln!("fake-cargo: cannot expand stdout: {why}");
                ExitCode::FAILURE
            }),
        Some(file) => {
            let Some(file) = file.to_str() else {
                eprintln!(
                    "fake-cargo: stdout file is not valid UTF-8: {}",
                    LosslessBytes::new(file.as_os_str().as_encoded_bytes())
                );
                return Err(ExitCode::FAILURE);
            };
            let path = match expand(file, script_path, started) {
                Ok(path) => path,
                Err(why) => {
                    eprintln!("fake-cargo: cannot expand stdout file: {why}");
                    return Err(ExitCode::FAILURE);
                }
            };
            match std::fs::read(&path) {
                Ok(bytes) => Ok(bytes),
                Err(error) => {
                    eprintln!("fake-cargo: cannot read {path}: {error}");
                    Err(ExitCode::FAILURE)
                }
            }
        }
    }
}

fn refuse(program: &str, args: &[String], why: impl fmt::Display) -> ExitCode {
    eprintln!("fake-cargo: {why}: {program} {}", args.join(" "));
    ExitCode::from(UNMATCHED_EXIT)
}

/// A file under `live/` for as long as this process runs, so a test can watch how many ran at once.
fn live_marker(script_path: &str) -> std::io::Result<PathBuf> {
    let dir = Path::new(script_path)
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("live");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(std::process::id().to_string());
    std::fs::write(&path, b"")?;
    Ok(path)
}

fn answered_before(script_path: &str) -> Result<Vec<usize>, AnswerLogError> {
    let path = format!("{script_path}.answered");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(AnswerLogError::Read { path, source }),
    };
    text.lines()
        .enumerate()
        .map(|(line_number, line)| {
            let line_number = line_number
                .checked_add(1)
                .ok_or(AnswerLogError::LineCountOverflow)?;
            line.trim()
                .parse::<usize>()
                .map_err(|source| AnswerLogError::InvalidIndex {
                    path: path.clone(),
                    line: line_number,
                    source,
                })
        })
        .collect()
}

/// Appends this answer to the log beside the script, which is both what a test reads back and how `times` is counted across processes.
fn record(script_path: &str, index: usize) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{script_path}.answered"))?;
    writeln!(file, "{index}")
}

fn expand(text: &str, script_path: &str, started: &Started) -> Result<String, ExpandError> {
    let script_dir = Path::new(script_path)
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let script_dir = script_dir
        .to_str()
        .ok_or_else(|| ExpandError::ScriptDirectoryNotUtf8 {
            directory: script_dir.to_path_buf(),
        })?;
    let cwd = std::env::current_dir().map_err(|source| ExpandError::CurrentDirectory { source })?;
    let cwd = cwd
        .to_str()
        .ok_or_else(|| ExpandError::CurrentDirectoryNotUtf8 {
            directory: cwd.clone(),
        })?;
    let target_dir = match started
        .args
        .windows(2)
        .find_map(|arguments| match arguments {
            [flag, value] if flag == "--target-dir" => Some(value.as_str()),
            _ => None,
        }) {
        Some(target_dir) => target_dir,
        None => "",
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis().to_string())
        .map_err(|source| ExpandError::Clock { source })?;
    Ok(text
        .replace("{{script_dir}}", script_dir)
        .replace("{{cwd}}", cwd)
        .replace("{{target_dir}}", target_dir)
        .replace("{{pid}}", &std::process::id().to_string())
        .replace("{{now_ms}}", &now))
}
