// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A cargo, a rustc, a coverage tool, or a test binary that says what a script told it to say.

#![expect(
    clippy::print_stderr,
    reason = "printing on the standard streams is what this program is: it stands in for a tool \
              whose whole answer is what it prints and what it exits with"
)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use njutest_devkit::fake_cargo::{Invocation, SCRIPT_ENV, Script, UNMATCHED_EXIT};

/// The command this process was started as: the name it answers to, and what came after it.
struct Started {
    program: String,
    args: Vec<String>,
}

fn main() -> ExitCode {
    let command_line: Vec<String> = std::env::args().collect();
    let program = command_line.first().map_or_else(String::new, |first| {
        Path::new(first)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let started = Started {
        program,
        args: command_line.into_iter().skip(1).collect(),
    };
    let (program, args) = (&started.program, &started.args);
    let Ok(script_path) = std::env::var(SCRIPT_ENV) else {
        return refuse(program, args, &format!("{SCRIPT_ENV} is not set"));
    };
    let Ok(text) = std::fs::read_to_string(&script_path) else {
        return refuse(program, args, &format!("{script_path} cannot be read"));
    };
    let Ok(script) = serde_json::from_str::<Script>(&text) else {
        return refuse(program, args, &format!("{script_path} is not the script"));
    };
    let answered = answered_before(&script_path);
    let Some((index, entry)) = script
        .invocations
        .iter()
        .enumerate()
        .find(|(index, entry)| matches(entry, &started, *index, &answered))
    else {
        return refuse(program, args, "no entry of the script matches");
    };
    record(&script_path, index);
    answer(entry, &script_path)
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
            .all(|(name, value)| std::env::var(name).ok().as_ref() == Some(value));
    let turns_left = entry.times.is_none_or(|times| {
        let used = answered.iter().filter(|seen| **seen == index).count();
        u32::try_from(used).is_ok_and(|used| used < times)
    });
    entry.program == started.program && prefix_matches && environment_matches && turns_left
}

/// Does what the entry says, in the order a tool would: the files first, then the wait, then what it printed, then what it exits with.
fn answer(entry: &Invocation, script_path: &str) -> ExitCode {
    let live = live_marker(script_path);
    for write in &entry.writes {
        let path = PathBuf::from(expand(&write.path, script_path));
        if let Some(parent) = path.parent() {
            let _made = std::fs::create_dir_all(parent);
        }
        let _written = std::fs::write(&path, expand(&write.contents, script_path));
    }
    if entry.delay_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(entry.delay_ms));
    }
    let stdout = entry.stdout_file.as_ref().map_or_else(
        || expand(&entry.stdout, script_path).into_bytes(),
        |file| std::fs::read(expand(&file.to_string_lossy(), script_path)).unwrap_or_default(),
    );
    if !stdout.is_empty() {
        let mut out = std::io::stdout();
        let _printed = out.write_all(&stdout);
        let _flushed = out.flush();
    }
    let stderr = expand(&entry.stderr, script_path);
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }
    if let Some(path) = live {
        let _removed = std::fs::remove_file(path);
    }
    ExitCode::from(entry.exit)
}

fn refuse(program: &str, args: &[String], why: &str) -> ExitCode {
    eprintln!("fake-cargo: {why}: {program} {}", args.join(" "));
    ExitCode::from(UNMATCHED_EXIT)
}

/// A file under `live/` for as long as this process runs, so a test can watch how many ran at once.
fn live_marker(script_path: &str) -> Option<PathBuf> {
    let dir = Path::new(script_path)
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("live");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(std::process::id().to_string());
    std::fs::write(&path, b"").ok()?;
    Some(path)
}

fn answered_before(script_path: &str) -> Vec<usize> {
    std::fs::read_to_string(format!("{script_path}.answered"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// Appends this answer to the log beside the script, which is both what a test reads back and how `times` is counted across processes.
fn record(script_path: &str, index: usize) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{script_path}.answered"))
    {
        let _written = writeln!(file, "{index}");
    }
}

fn expand(text: &str, script_path: &str) -> String {
    let script_dir = Path::new(script_path)
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_string_lossy()
        .into_owned();
    let cwd = std::env::current_dir()
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_default();
    let target_dir = std::env::args()
        .skip_while(|arg| arg != "--target-dir")
        .nth(1)
        .unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis().to_string())
        .unwrap_or_default();
    text.replace("{{script_dir}}", &script_dir)
        .replace("{{cwd}}", &cwd)
        .replace("{{target_dir}}", &target_dir)
        .replace("{{pid}}", &std::process::id().to_string())
        .replace("{{now_ms}}", &now)
}
