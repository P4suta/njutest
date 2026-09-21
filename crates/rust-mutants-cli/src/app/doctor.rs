// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run would find in this environment, asked before it is spent finding out.

use std::path::{Path, PathBuf};

use rust_mutants::runner::Cancel;

use std::io::Write;

use rust_mutants::{snapshot, workspace};

use super::{json_line, locating, write};
use crate::Environment;
use crate::filesystem::{EntryKind, entry_kind};
use crate::report::doctor as doctor_report;

pub(super) fn doctor(
    asked: &Asked<'_>,
    environment: &Environment,
    stdout: &mut dyn Write,
    cancel: &Cancel,
) -> Result<u8, crate::error::CliError> {
    let document = doctor_document(asked, environment, cancel);
    let text = if asked.json {
        json_line(&document)?
    } else {
        doctor_report::lines(&document)
    };
    write(stdout, &text)?;
    Ok(if document.ok { 0 } else { crate::EXIT_USAGE })
}

/// What a run would find in this environment, as the document both the lines and a bundle are made of.
pub(super) fn doctor_document(
    asked: &Asked<'_>,
    environment: &Environment,
    cancel: &Cancel,
) -> doctor_report::DoctorDocument {
    use doctor_report::Standing::{Fail, Ok as Well};
    let root = environment.rooted(asked.root);
    let mut checks: Vec<doctor_report::Check> = Vec::new();

    let toolchain = rust_mutants::cargo::Toolchain::locate(&locating(environment), &root, cancel);
    match &toolchain {
        Ok(found) => {
            checks.push(noted("cargo", Well, &found.cargo_version().summary));
            checks.push(noted("rustc", Well, &found.rustc_version().summary));
            checks.push(noted("host", Well, found.host()));
        }
        Err(error) => checks.push(doctor_report::Check::new(
            "toolchain",
            Fail,
            &error.to_string(),
            Some("install a toolchain with rustup, or put cargo on PATH"),
        )),
    }

    let manifest = root.join("Cargo.toml");
    checks.push(workspace_check(&root, &manifest));

    let config_path = root.join(crate::config::FILE_NAME);
    let mut reports = super::stored::Store::read(&root).root();
    checks.push(config_check(&root, &config_path, &mut reports));

    let temp = &environment.temp_directory;
    checks.push(temp_check(temp));

    checks.extend(machine(environment));
    let available_toolchain = match toolchain.as_ref() {
        Ok(toolchain) => Some(toolchain),
        Err(_) => None,
    };
    checks.push(targets_check(
        available_toolchain,
        &root,
        asked.packages,
        cancel,
    ));
    checks.push(snapshots_check(&reports, environment));
    checks.push(llvm_tools_check(available_toolchain));
    checks.push(guards_check(
        available_toolchain.map(rust_mutants::cargo::Toolchain::host),
    ));
    doctor_report::DoctorDocument::of(checks)
}

fn config_check(root: &Path, path: &Path, reports: &mut PathBuf) -> doctor_report::Check {
    use doctor_report::Standing::{Fail, Ok as Well};
    match entry_kind(path) {
        Ok(EntryKind::File) => match crate::config::Config::load(root) {
            Ok(read) => {
                *reports = super::stored::Store::of(root, &read.reports.directory).root();
                noted("config", Well, &path.display().to_string())
            }
            Err(error) => {
                doctor_report::Check::new("config", Fail, &error.to_string(), error.code().remedy)
            }
        },
        Ok(EntryKind::Missing) => noted(
            "config",
            Well,
            &format!(
                "no {} under {}; the defaults apply, and `rust-mutants init` writes one",
                crate::config::FILE_NAME,
                root.display()
            ),
        ),
        Ok(EntryKind::Directory | EntryKind::Other) => doctor_report::Check::new(
            "config",
            Fail,
            &format!("{} is not a regular file", path.display()),
            Some("replace it with a regular configuration file, or remove it to use defaults"),
        ),
        Err(error) => doctor_report::Check::new(
            "config",
            Fail,
            &format!("cannot inspect {}: {error}", path.display()),
            Some("make the configuration path readable, or remove it to use defaults"),
        ),
    }
}

fn temp_check(temp: &Path) -> doctor_report::Check {
    use doctor_report::Standing::{Fail, Ok as Well};
    let (standing, problem) = match std::fs::metadata(temp) {
        Ok(metadata) if metadata.is_dir() => (Well, None),
        Ok(_not_a_directory) => (Fail, Some("it is not a directory".to_owned())),
        Err(error) => (Fail, Some(format!("its metadata cannot be read: {error}"))),
    };
    let suffix = problem.map_or_else(String::new, |problem| format!("; {problem}"));
    doctor_report::Check::new(
        "temp",
        standing,
        &format!(
            "{} (snapshots as {}*, target directories as {}*, scratch as {}*){suffix}",
            temp.display(),
            snapshot::DIR_PREFIX,
            workspace::TARGET_DIR_PREFIX,
            workspace::SCRATCH_DIR_PREFIX
        ),
        (standing == Fail).then_some("set TMPDIR to a directory a run may write in"),
    )
}

/// What this machine offers a run, as against what the workspace does.
fn machine(environment: &Environment) -> Vec<doctor_report::Check> {
    vec![
        git_check(environment),
        environment_check(environment),
        cache_check(environment),
        disk_check(&environment.temp_directory),
        exec_check(&environment.temp_directory, &environment.program),
    ]
}

/// Whether git is installed, which is what `--changed` asks.
fn git_check(environment: &Environment) -> doctor_report::Check {
    use doctor_report::Standing::{Ok as Well, Warn};
    let printed = std::process::Command::new("git")
        .arg("--version")
        .envs(environment.vars.clone())
        .output();
    match printed {
        Ok(printed) if printed.status.success() => {
            let version = match std::str::from_utf8(&printed.stdout) {
                Ok(text) => text.trim().to_owned(),
                Err(_not_utf8) => {
                    rust_mutants::telling::LosslessBytes::new(&printed.stdout).to_string()
                }
            };
            doctor_report::Check::new("git", Well, &version, None)
        }
        _ => doctor_report::Check::new(
            "git",
            Warn,
            "git is not there, so --changed has nothing to ask",
            Some("install git, or select with --file and --package instead"),
        ),
    }
}

/// Whether anything would run: a package without a test target answers nothing about its mutants.
fn targets_check(
    toolchain: Option<&rust_mutants::cargo::Toolchain>,
    root: &Path,
    packages: &[String],
    cancel: &Cancel,
) -> doctor_report::Check {
    use doctor_report::Standing::{Fail, Ok as Well, Warn};
    let Some(toolchain) = toolchain else {
        return doctor_report::Check::new(
            "targets",
            Warn,
            "there is no cargo to ask what the targets are",
            None,
        );
    };
    let trace = rust_mutants::trace::Recorder::disabled();
    let driver = rust_mutants::cargo::Driver {
        toolchain,
        dir: root,
        cancel,
        trace: &trace,
    };
    let metadata = rust_mutants::cargo::Metadata::load_no_deps(
        &driver,
        rust_mutants::cargo::MetadataOptions::default(),
    );
    let metadata = match metadata {
        Ok(metadata) => metadata,
        Err(error) => {
            return doctor_report::Check::new(
                "targets",
                Warn,
                &error.to_string(),
                Some("fix what cargo metadata says before asking about mutants"),
            );
        }
    };
    let selected: Vec<&rust_mutants::cargo::Package> = metadata
        .members()
        .filter(|package| packages.is_empty() || packages.contains(&package.name))
        .collect();
    let barren: Vec<&str> = selected
        .iter()
        .filter(|package| !package.targets.iter().any(tests_something))
        .map(|package| package.name.as_str())
        .collect();
    let tested = match selected.len().checked_sub(barren.len()) {
        Some(tested) => tested,
        None => {
            return doctor_report::Check::new(
                "targets",
                Fail,
                "the package classification was internally inconsistent",
                Some("report this invariant failure"),
            );
        }
    };
    if selected.is_empty() || tested == 0 {
        return doctor_report::Check::new(
            "targets",
            Fail,
            &format!(
                "{} packages selected, none with a test target",
                selected.len()
            ),
            Some("write a test, or select a package that has one with --package"),
        );
    }
    if barren.is_empty() {
        return doctor_report::Check::new(
            "targets",
            Well,
            &format!("{tested} packages, each with a test target"),
            None,
        );
    }
    doctor_report::Check::new(
        "targets",
        Warn,
        &format!("{} has no test target", barren.join(", ")),
        Some("a package without a test target answers nothing; narrow with --package"),
    )
}

/// Whether a target is one a run would execute.
fn tests_something(target: &rust_mutants::cargo::Target) -> bool {
    target.test
        && target
            .kind
            .iter()
            .any(|kind| kind == "test" || kind == "lib" || kind == "bin")
}

/// Whether the temporary directory has room for the snapshots and target directories a run makes.
/// Whether there is room, said in whole gigabytes.
///
/// The figure is rounded because it is read twice: a check that reported the
/// exact free bytes disagreed with itself between two invocations a moment
/// apart, which is the same thing that made the execution check unreadable
/// before it was made to rest on a ratio. Whole gigabytes is the granularity a
/// person decides at, and it does not move while they are looking.
fn disk_check(temp: &Path) -> doctor_report::Check {
    use doctor_report::Standing::{Fail, Ok as Well, Warn};
    const GIB: u64 = 1024 * 1024 * 1024;
    let Some(free) = free_space(temp) else {
        return doctor_report::Check::new(
            "disk",
            Warn,
            &format!("how much room {} has could not be read", temp.display()),
            None,
        );
    };
    let detail = format!("{} GiB free under {}", free / GIB, temp.display());
    let standing = if free < GIB / 4 {
        Fail
    } else if free < GIB {
        Warn
    } else {
        Well
    };
    doctor_report::Check::new(
        "disk",
        standing,
        &detail,
        (standing != Well).then_some("free some room, or point TMPDIR at a filesystem that has it"),
    )
}

/// What it costs to run a file that has just been written, which a run does for every target it builds.
fn exec_check(temp: &Path, program: &Path) -> doctor_report::Check {
    use doctor_report::Standing::Ok as Well;
    let (first, second) = match rust_mutants::execcost::exec_twice(temp, program) {
        Ok(measured) => measured,
        Err(why) => {
            return doctor_report::Check::new("exec", Well, &format!("not measured: {why}"), None);
        }
    };

    let (first, second) = (first.as_secs_f64(), second.as_secs_f64());
    let detail = format!(
        "a newly written file took {first:.2}s to run the first time and {second:.2}s the second"
    );
    let standing = standing_of(first, second);
    doctor_report::Check::new(
        "exec",
        standing,
        &detail,
        (standing != Well).then_some(
            "this machine is evaluating new executables; a run started now measures that \
             and not your tests, so wait until the first number is under a second. A \
             process waiting on it looks hung rather than slow, and a sample of one shows \
             a single frame in the dynamic loader",
        ),
    )
}

/// What the pair says, which is not what either number says alone.
const fn standing_of(first: f64, second: f64) -> doctor_report::Standing {
    use doctor_report::Standing::{Fail, Ok as Well, Warn};
    const SLOW: f64 = 5.0;
    const LOPSIDED: f64 = 10.0;
    if first < SLOW {
        return Well;
    }
    if first >= second * LOPSIDED {
        Fail
    } else {
        Warn
    }
}

/// How many bytes the filesystem holding `path` will still take.
#[cfg(unix)]
fn free_space(path: &Path) -> Option<u64> {
    let statistics = match rustix::fs::statvfs(path) {
        Ok(statistics) => statistics,
        Err(_) => return None,
    };
    let block = if statistics.f_frsize == 0 {
        statistics.f_bsize
    } else {
        statistics.f_frsize
    };
    block.checked_mul(statistics.f_bavail)
}

/// How many bytes the filesystem holding `path` will still take.
#[cfg(not(unix))]
const fn free_space(_path: &Path) -> Option<u64> {
    None
}

/// Bytes as a person reads them.
///
/// # Errors
/// Returns when an intermediate unit or fractional value cannot be represented exactly.
#[cfg(feature = "testkit")]
pub fn rendered_bytes(bytes: u64) -> Result<String, crate::error::CliError> {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut whole = bytes;
    let mut remainder: u64 = 0;
    let mut unit: usize = 0;
    while whole >= 1024 {
        let Some(next) = unit.checked_add(1) else {
            return Err(crate::error::CliError::ProjectionOverflow {
                projection: "doctor",
                field: "the byte-size unit index",
            });
        };
        if next >= UNITS.len() {
            break;
        }
        remainder = whole % 1024;
        whole /= 1024;
        unit = next;
    }
    let name = UNITS.get(unit).copied().unwrap_or("B");
    if unit == 0 {
        return Ok(format!("{whole} {name}"));
    }
    let tenths = remainder
        .checked_mul(10)
        .ok_or(crate::error::CliError::ProjectionOverflow {
            projection: "doctor",
            field: "the rendered byte-size fraction",
        })?
        / 1024;
    Ok(format!("{whole}.{tenths} {name}"))
}

/// A check that carries no remedy because nothing is wrong with it.
fn noted(name: &str, standing: doctor_report::Standing, detail: &str) -> doctor_report::Check {
    doctor_report::Check::new(name, standing, detail, None)
}

/// Whether the root is the workspace, which is what a run measures.
fn workspace_check(root: &Path, manifest: &Path) -> doctor_report::Check {
    use doctor_report::Standing::{Fail, Ok as Well};
    match entry_kind(manifest) {
        Ok(EntryKind::File) => {}
        Ok(EntryKind::Missing) => {
            return doctor_report::Check::new(
                "workspace",
                Fail,
                &format!("{} is not there", manifest.display()),
                Some("run inside a cargo workspace, or pass --root at one"),
            );
        }
        Ok(EntryKind::Directory | EntryKind::Other) => {
            return doctor_report::Check::new(
                "workspace",
                Fail,
                &format!("{} is not a regular file", manifest.display()),
                Some("pass --root at a Cargo workspace"),
            );
        }
        Err(error) => {
            return doctor_report::Check::new(
                "workspace",
                Fail,
                &format!("cannot inspect {}: {error}", manifest.display()),
                Some("make the workspace manifest readable"),
            );
        }
    }
    let own = match std::fs::read_to_string(manifest) {
        Ok(own) => own,
        Err(error) => {
            return doctor_report::Check::new(
                "workspace",
                Fail,
                &format!("cannot read {}: {error}", manifest.display()),
                Some("make the workspace manifest readable"),
            );
        }
    };
    if own
        .lines()
        .any(|line| line.trim_start().starts_with("[workspace"))
    {
        return doctor_report::Check::new("workspace", Well, &manifest.display().to_string(), None);
    }
    let above = root.ancestors().skip(1).find(|directory| {
        std::fs::read_to_string(directory.join("Cargo.toml")).is_ok_and(|text| {
            text.lines()
                .any(|line| line.trim_start().starts_with("[workspace"))
        })
    });
    above.map_or_else(
        || doctor_report::Check::new("workspace", Well, &manifest.display().to_string(), None),
        |found| {
            doctor_report::Check::new(
                "workspace",
                Fail,
                &format!("{} is a member of {}", root.display(), found.display()),
                Some("run with --root at the workspace root, and --package to narrow it"),
            )
        },
    )
}

/// Whether a reserved variable is already set, which would make every answer a run gives an answer about something else.
fn environment_check(environment: &Environment) -> doctor_report::Check {
    use doctor_report::Standing::{Fail, Ok as Well};
    let set = super::reserved_names(environment);
    if set.is_empty() {
        return doctor_report::Check::new(
            "environment",
            Well,
            &format!(
                "none of {} is set",
                rust_mutants::execute::RESERVED_ENV.join(", ")
            ),
            None,
        );
    }
    doctor_report::Check::new(
        "environment",
        Fail,
        &format!("{} is set", set.join(", ")),
        Some("unset it: a run composes the activation itself"),
    )
}

/// Where what earlier runs established is kept, and how much of it there is.
fn cache_check(environment: &Environment) -> doctor_report::Check {
    use doctor_report::Standing::{Ok as Well, Warn};
    let store = crate::outcomes::Store::new(&environment.cache_directory);
    let writable = std::fs::create_dir_all(store.root()).is_ok();
    let size = store.size();
    match (writable, size) {
        (true, Ok((records, bytes))) => doctor_report::Check::new(
            "cache",
            Well,
            &format!(
                "{} ({records} records, {bytes} bytes)",
                store.root().display()
            ),
            None,
        ),
        (false, _) => doctor_report::Check::new(
            "cache",
            Warn,
            &format!("{} cannot be written", store.root().display()),
            Some("set XDG_CACHE_HOME, or pass --cache-dir, or run with --no-cache"),
        ),
        (true, Err(error)) => doctor_report::Check::new(
            "cache",
            Warn,
            &format!(
                "{} cannot be read completely: {error}",
                store.root().display()
            ),
            Some("check the cache directory is readable, or pass --cache-dir at another one"),
        ),
    }
}

/// What earlier runs left in the temporary directory, and what a run kept on purpose.
fn snapshots_check(reports: &Path, environment: &Environment) -> doctor_report::Check {
    use doctor_report::Standing::{Ok as Well, Warn};
    let ledger = match crate::kept::Ledger::read(reports) {
        Ok(ledger) => ledger,
        Err(error) => {
            return doctor_report::Check::new(
                "snapshots",
                Warn,
                &format!(
                    "{} cannot be read exactly: {error}",
                    reports.join(crate::kept::FILE_NAME).display()
                ),
                Some("repair or remove the malformed kept-directory ledger"),
            );
        }
    };
    let entries = match std::fs::read_dir(&environment.temp_directory) {
        Ok(entries) => entries,
        Err(error) => {
            return doctor_report::Check::new(
                "snapshots",
                Warn,
                &format!(
                    "{} cannot be read completely: {error}",
                    environment.temp_directory.display()
                ),
                Some("check the temporary directory is readable by this user"),
            );
        }
    };
    let entries = match entries.collect::<std::io::Result<Vec<_>>>() {
        Ok(entries) => entries,
        Err(error) => {
            return doctor_report::Check::new(
                "snapshots",
                Warn,
                &format!(
                    "{} cannot be enumerated completely: {error}",
                    environment.temp_directory.display()
                ),
                Some("check the temporary directory is readable by this user"),
            );
        }
    };
    let abandoned = entries
        .into_iter()
        .filter(|entry| {
            let name = entry.file_name();
            let bytes = name.as_encoded_bytes();
            bytes.starts_with(snapshot::DIR_PREFIX.as_bytes())
                || bytes.starts_with(workspace::TARGET_DIR_PREFIX.as_bytes())
                || bytes.starts_with(workspace::SCRATCH_DIR_PREFIX.as_bytes())
        })
        .count();
    if abandoned == 0 && ledger.kept.is_empty() {
        return doctor_report::Check::new(
            "snapshots",
            Well,
            &format!(
                "nothing is left over under {}",
                environment.temp_directory.display()
            ),
            None,
        );
    }
    doctor_report::Check::new(
        "snapshots",
        Warn,
        &format!(
            "{abandoned} directories under {}, {} kept on purpose",
            environment.temp_directory.display(),
            ledger.kept.len()
        ),
        Some(
            "`cache --gc` removes what is abandoned, `--gc --all` the build caches, `--gc --kept` what was kept",
        ),
    )
}

/// Whether the LLVM tools the coverage layer needs are installed.
fn guards_check(host: Option<&str>) -> doctor_report::Check {
    use doctor_report::Standing::{Ok as Well, Warn};
    let threadless = ["wasm", "emscripten", "zkvm"];
    let Some(host) = host else {
        return doctor_report::Check::new(
            "guards",
            Warn,
            "there is no toolchain to say what this host is",
            Some("install a toolchain with rustup, or put cargo on PATH"),
        );
    };
    if threadless.iter().any(|family| host.contains(family)) {
        return doctor_report::Check::new(
            "guards",
            Warn,
            &format!(
                "{host} runs its tests without a thread each, so a touch cannot be attributed to \
                 a test and every mutation goes to every test of its target"
            ),
            Some("route by the coverage build instead: --coverage"),
        );
    }
    doctor_report::Check::new(
        "guards",
        Well,
        &format!(
            "{host} gives each test a thread named after it, so a mutation goes to the tests \
             that reached it"
        ),
        None,
    )
}

fn llvm_tools_check(toolchain: Option<&rust_mutants::cargo::Toolchain>) -> doctor_report::Check {
    use doctor_report::Standing::{Ok as Well, Warn};
    let advise = Some("rustup component add llvm-tools");
    let Some(toolchain) = toolchain else {
        return doctor_report::Check::new(
            "llvm-tools",
            Warn,
            "there is no toolchain to look in",
            advise,
        );
    };
    let printed = std::process::Command::new(toolchain.rustc())
        .arg("--print")
        .arg("target-libdir")
        .output();
    let Ok(printed) = printed else {
        return doctor_report::Check::new(
            "llvm-tools",
            Warn,
            "rustc could not say where its libraries are",
            advise,
        );
    };
    let libdir = match std::str::from_utf8(&printed.stdout) {
        Ok(text) => PathBuf::from(text.trim()),
        Err(_not_utf8) => {
            return doctor_report::Check::new(
                "llvm-tools",
                Warn,
                &format!(
                    "rustc printed a non-UTF-8 target library directory ({})",
                    rust_mutants::telling::LosslessBytes::new(&printed.stdout)
                ),
                advise,
            );
        }
    };
    let profdata = libdir.parent().map(|parent| {
        parent.join("bin").join(if cfg!(windows) {
            "llvm-profdata.exe"
        } else {
            "llvm-profdata"
        })
    });
    match profdata {
        Some(path) if matches!(entry_kind(&path), Ok(EntryKind::File)) => {
            doctor_report::Check::new("llvm-tools", Well, &path.display().to_string(), None)
        }
        Some(path) => doctor_report::Check::new(
            "llvm-tools",
            Warn,
            &format!(
                "{} is not a readable regular file, so coverage routing falls back to every \
                 target",
                path.display()
            ),
            advise,
        ),
        None => doctor_report::Check::new(
            "llvm-tools",
            Warn,
            "llvm-profdata is not in the toolchain's sysroot, so coverage routing falls back to \
             every target",
            advise,
        ),
    }
}

/// What `doctor` was asked, and how it answers.
#[derive(Debug, Clone, Copy)]
pub(super) struct Asked<'a> {
    pub(super) root: Option<&'a Path>,
    pub(super) packages: &'a [String],
    pub(super) json: bool,
}
