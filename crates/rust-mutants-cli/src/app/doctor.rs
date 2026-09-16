// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run would find in this environment, asked before it is spent finding out.
//!
//! Every check answers about the tree as it is rather than about the tree as it
//! would build: a doctor is for a workspace that may not compile, and one that
//! could not answer until it did would be no use at all. A check that cannot
//! look says so; nothing here reads silence as a pass.

use std::path::{Path, PathBuf};

use rust_mutants::runner::Cancel;

use std::io::Write;

use rust_mutants::{snapshot, workspace};

use super::{json_line, locating, write};
use crate::Environment;
use crate::report::doctor as doctor_report;

pub(super) fn doctor(
    asked: &Asked<'_>,
    environment: &Environment,
    stdout: &mut dyn Write,
    cancel: &Cancel,
) -> u8 {
    let document = doctor_document(asked, environment, cancel);
    let text = if asked.json {
        json_line(&document)
    } else {
        doctor_report::lines(&document)
    };
    write(stdout, &text);
    if document.ok { 0 } else { crate::EXIT_USAGE }
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
    let mut reports = root.join(crate::config::DEFAULT_REPORTS_DIRECTORY);
    if config_path.is_file() {
        match crate::config::Config::load(&root) {
            Ok(read) => {
                reports = root.join(&read.reports.directory);
                checks.push(noted("config", Well, &config_path.display().to_string()));
            }
            Err(error) => checks.push(doctor_report::Check::new(
                "config",
                Fail,
                &error.to_string(),
                error.code().remedy,
            )),
        }
    } else {
        checks.push(noted(
            "config",
            Well,
            &format!(
                "none; the defaults apply. `rust-mutants init` writes {}",
                crate::config::FILE_NAME
            ),
        ));
    }

    let temp = &environment.temp_directory;
    checks.push(doctor_report::Check::new(
        "temp",
        if temp.is_dir() { Well } else { Fail },
        &format!(
            "{} (snapshots as {}*, target directories as {}*, scratch as {}*)",
            temp.display(),
            snapshot::DIR_PREFIX,
            workspace::TARGET_DIR_PREFIX,
            workspace::SCRATCH_DIR_PREFIX
        ),
        (!temp.is_dir()).then_some("set TMPDIR to a directory a run may write in"),
    ));

    checks.push(git_check(environment));
    checks.push(targets_check(
        toolchain.as_ref().ok(),
        &root,
        asked.packages,
        cancel,
    ));
    checks.push(environment_check(environment));
    checks.push(cache_check(environment));
    checks.push(disk_check(&environment.temp_directory));
    checks.push(snapshots_check(&reports, environment));
    checks.push(llvm_tools_check(toolchain.as_ref().ok()));
    checks.push(guards_check(
        toolchain
            .as_ref()
            .ok()
            .map(rust_mutants::cargo::Toolchain::host),
    ));
    doctor_report::DoctorDocument::of(checks)
}

/// Whether git is installed, which is what `--changed` asks.
fn git_check(environment: &Environment) -> doctor_report::Check {
    use doctor_report::Standing::{Ok as Well, Warn};
    let printed = std::process::Command::new("git")
        .arg("--version")
        .envs(environment.vars.clone())
        .output();
    match printed {
        Ok(printed) if printed.status.success() => doctor_report::Check::new(
            "git",
            Well,
            String::from_utf8_lossy(&printed.stdout).trim(),
            None,
        ),
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
    let tested = selected.len().saturating_sub(barren.len());
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
    let detail = format!("{} free under {}", rendered_bytes(free), temp.display());
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

/// How many bytes the filesystem holding `path` will still take.
#[cfg(unix)]
fn free_space(path: &Path) -> Option<u64> {
    let statistics = rustix::fs::statvfs(path).ok()?;
    let block = if statistics.f_frsize == 0 {
        statistics.f_bsize
    } else {
        statistics.f_frsize
    };
    Some(block.saturating_mul(statistics.f_bavail))
}

/// How many bytes the filesystem holding `path` will still take.
#[cfg(not(unix))]
const fn free_space(_path: &Path) -> Option<u64> {
    None
}

/// Bytes as a person reads them.
///
/// The value is truncated rather than rounded, in the unit and in the tenth:
/// the number is read where a person is deciding whether a sweep gave back
/// enough room, and one that overstated it would send them away satisfied.
#[must_use]
pub fn rendered_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut whole = bytes;
    let mut remainder: u64 = 0;
    let mut unit: usize = 0;
    while whole >= 1024 && unit.saturating_add(1) < UNITS.len() {
        remainder = whole.wrapping_rem(1024);
        whole = whole.wrapping_div(1024);
        unit = unit.saturating_add(1);
    }
    let name = UNITS.get(unit).copied().unwrap_or("B");
    if unit == 0 {
        return format!("{whole} {name}");
    }
    let tenths = remainder.saturating_mul(10).wrapping_div(1024);
    format!("{whole}.{tenths} {name}")
}

/// A check that carries no remedy because nothing is wrong with it.
fn noted(name: &str, standing: doctor_report::Standing, detail: &str) -> doctor_report::Check {
    doctor_report::Check::new(name, standing, detail, None)
}

/// Whether the root is the workspace, which is what a run measures.
///
/// The check reads the manifests rather than asking cargo: a doctor answers
/// about a tree that may not build, and `cargo metadata` on a tree that does
/// not resolve says nothing about where the workspace is.
fn workspace_check(root: &Path, manifest: &Path) -> doctor_report::Check {
    use doctor_report::Standing::{Fail, Ok as Well};
    if !manifest.is_file() {
        return doctor_report::Check::new(
            "workspace",
            Fail,
            &format!("{} is not there", manifest.display()),
            Some("run inside a cargo workspace, or pass --root at one"),
        );
    }
    let own = std::fs::read_to_string(manifest).unwrap_or_default();
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
        return doctor_report::Check::new("environment", Well, "no reserved variable is set", None);
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
    let (records, bytes) = store.size();
    let writable = std::fs::create_dir_all(store.root()).is_ok();
    if writable {
        doctor_report::Check::new(
            "cache",
            Well,
            &format!(
                "{} ({records} records, {bytes} bytes)",
                store.root().display()
            ),
            None,
        )
    } else {
        doctor_report::Check::new(
            "cache",
            Warn,
            &format!("{} cannot be written", store.root().display()),
            Some("set XDG_CACHE_HOME, or pass --cache-dir, or run with --no-cache"),
        )
    }
}

/// What earlier runs left in the temporary directory, and what a run kept on purpose.
///
/// The ledger of what was kept lives under the report directory the
/// configuration names, not under the default one: a project that moved its
/// reports would otherwise be told nothing was kept.
fn snapshots_check(reports: &Path, environment: &Environment) -> doctor_report::Check {
    use doctor_report::Standing::{Ok as Well, Warn};
    let ledger = crate::kept::Ledger::read(reports);
    let abandoned = std::fs::read_dir(&environment.temp_directory)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    name.starts_with(snapshot::DIR_PREFIX)
                        || name.starts_with(workspace::TARGET_DIR_PREFIX)
                        || name.starts_with(workspace::SCRATCH_DIR_PREFIX)
                })
                .count()
        })
        .unwrap_or_default();
    if abandoned == 0 && ledger.kept.is_empty() {
        return doctor_report::Check::new("snapshots", Well, "nothing is left over", None);
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
///
/// Coverage routing fails open — a measurement it cannot make routes every
/// mutation everywhere — so a missing component is a warning about how much a
/// run will cost, never a reason not to run.
/// Whether the guards of an instrumented tree can say which test reached them, on the host a run compiles for.
///
/// The record is per test because libtest gives each test a thread of its own
/// named after it, and it only does that where the platform has threads. On
/// one that does not, every touch is recorded under a name no test answers
/// for, so every mutation is put to every test of its target: sound, and none
/// of the saving. Saying so before a run is better than a person reading a
/// work ledger afterwards and wondering.
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
    let libdir = PathBuf::from(String::from_utf8_lossy(&printed.stdout).trim().to_owned());
    let profdata = libdir.parent().map(|parent| {
        parent.join("bin").join(if cfg!(windows) {
            "llvm-profdata.exe"
        } else {
            "llvm-profdata"
        })
    });
    profdata.filter(|path| path.is_file()).map_or_else(
        || {
            doctor_report::Check::new(
                "llvm-tools",
                Warn,
                "llvm-profdata is not in the toolchain's sysroot, so coverage routing falls back \
                 to every target",
                advise,
            )
        },
        |path| doctor_report::Check::new("llvm-tools", Well, &path.display().to_string(), None),
    )
}

/// What `doctor` was asked, and how it answers.
#[derive(Debug, Clone, Copy)]
pub(super) struct Asked<'a> {
    pub(super) root: Option<&'a Path>,
    pub(super) packages: &'a [String],
    pub(super) json: bool,
}
