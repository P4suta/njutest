// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `cargo xtask sweep`: build output and temporary directories nobody has touched for a while go, and nothing else does.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "a machine that cannot be set up leaves nothing to sweep"
)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

/// A repository with worktrees and a temporary directory, all inside one scratch directory.
struct Machine {
    scratch: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        let machine = Self {
            scratch: tempfile::tempdir().expect("a scratch directory"),
        };
        std::fs::create_dir_all(machine.temp()).expect("a temporary directory");
        std::fs::create_dir_all(machine.main()).expect("a repository");
        git(&machine.main(), &["init", "-q", "-b", "main"]);
        std::fs::write(machine.main().join("README"), "one\n").expect("a file");
        git(&machine.main(), &["add", "README"]);
        git(
            &machine.main(),
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "one",
            ],
        );
        machine
    }

    fn root(&self) -> PathBuf {
        self.scratch.path().join("projects")
    }

    fn main(&self) -> PathBuf {
        self.root().join("main")
    }

    fn temp(&self) -> PathBuf {
        self.scratch.path().join("tmp")
    }

    fn worktree(&self, at: &Path) {
        git(
            &self.main(),
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                &at.display().to_string(),
            ],
        );
    }

    fn sweep(&self) -> Output {
        let mut command = isolated(env!("CARGO_BIN_EXE_xtask"));
        command
            .args(["sweep", "--budget-seconds", "60"])
            .current_dir(self.main())
            .env("TMPDIR", self.temp())
            .output()
            .expect("the sweep")
    }
}

fn isolated(program: &str) -> Command {
    let mut command = Command::new(program);
    for (name, _value) in std::env::vars_os() {
        if name.as_encoded_bytes().starts_with(b"GIT_") {
            command.env_remove(name);
        }
    }
    command.env("GIT_CONFIG_GLOBAL", "/dev/null");
    command
}

fn git(directory: &Path, arguments: &[&str]) {
    let status = isolated("git")
        .args(arguments)
        .current_dir(directory)
        .status()
        .expect("git");
    assert!(
        status.success(),
        "git {arguments:?} in {}",
        directory.display()
    );
}

/// A directory of build output with a file two levels down, as cargo leaves one.
fn built(at: &Path) {
    let deep = at.join("debug").join("deps");
    std::fs::create_dir_all(&deep).expect("build output");
    std::fs::write(deep.join("libone.rlib"), vec![0_u8; 4096]).expect("an artifact");
}

/// Marks everything at and below `at` as last written two days ago.
fn aged(at: &Path) {
    let then = SystemTime::now()
        .checked_sub(Duration::from_hours(48))
        .expect("two days ago");
    for entry in walkdir::WalkDir::new(at).contents_first(true) {
        let entry = entry.expect("an entry");
        std::fs::File::open(entry.path())
            .and_then(|file| file.set_modified(then))
            .expect("an old modification time");
    }
}

/// Whether something is at `path`, which is a different answer from a path that cannot be read.
fn present(path: &Path) -> bool {
    path.try_exists().expect("a path can be looked for")
}

fn listed(machine: &Machine) -> String {
    let output = isolated("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(machine.main())
        .output()
        .expect("git worktree list");
    String::from_utf8(output.stdout).expect("utf-8")
}

#[test]
fn build_output_and_temporaries_nobody_touched_go_and_nothing_else_does() {
    let machine = Machine::new();
    let idle = machine.root().join("idle");
    let busy = machine.root().join("busy");
    machine.worktree(&idle);
    machine.worktree(&busy);
    built(&idle.join("target"));
    aged(&idle.join("target"));
    built(&busy.join("target"));

    let gate = machine.temp().join("njutest-pre-push-abc");
    machine.worktree(&gate.join("tree"));
    built(&gate.join("target"));
    aged(&gate);

    let left = machine.temp().join("njutest-commands-old");
    std::fs::create_dir_all(left.join("fixture-baseline")).expect("a leftover");
    aged(&left);
    let running = machine.temp().join("njutest-commands-new");
    std::fs::create_dir_all(&running).expect("a test that is running");
    let foreign = machine.temp().join("somebody-else");
    std::fs::create_dir_all(&foreign).expect("another program's directory");
    aged(&foreign);

    let swept = machine.sweep();
    let said = format!(
        "{}{}",
        String::from_utf8(swept.stdout.clone()).expect("the sweep writes text"),
        String::from_utf8(swept.stderr.clone()).expect("the sweep writes text")
    );
    assert!(swept.status.success(), "{said}");
    assert!(
        !present(&idle.join("target")),
        "build output nobody has written for two days is regenerable and goes: {said}"
    );
    assert!(
        present(&idle.join("README")),
        "and the worktree it sat in stays, source and all"
    );
    assert!(
        present(&busy.join("target")),
        "build output written just now belongs to somebody working: {said}"
    );
    assert!(
        !present(&gate) && !listed(&machine).contains("njutest-pre-push-abc"),
        "a gate tree nobody has used for two days goes, and git forgets the worktree it was: \
         {said}\n{}",
        listed(&machine)
    );
    assert!(
        !present(&left),
        "a test's temporary directory two days old is a leak: {said}"
    );
    assert!(
        present(&running),
        "one made just now is a test still running"
    );
    assert!(
        present(&foreign),
        "and a directory another program made is not this repository's to take"
    );
    let trash: Vec<PathBuf> = [machine.root(), machine.temp()]
        .iter()
        .map(|at| at.join(".njutest-trash"))
        .filter(|trash| std::fs::read_dir(trash).is_ok_and(|mut entries| entries.next().is_some()))
        .collect();
    assert!(
        trash.is_empty(),
        "what was taken is gone within the budget, not moved aside and kept: {trash:?}"
    );
}

/// A process that sits with its working directory at `at` until it is dropped.
struct Sitting(std::process::Child);

impl Sitting {
    fn at(at: &Path) -> Self {
        Self(
            Command::new("sleep")
                .arg("600")
                .current_dir(at)
                .spawn()
                .expect("a process to sit there"),
        )
    }
}

impl Drop for Sitting {
    fn drop(&mut self) {
        self.0.kill().expect("the sitting process stops");
        self.0.wait().expect("and is reaped");
    }
}

#[test]
fn build_output_a_process_is_using_stays_however_long_nothing_wrote_it() {
    let machine = Machine::new();
    let running = machine.root().join("running");
    machine.worktree(&running);
    built(&running.join("target"));
    aged(&running.join("target"));
    let sitting = Sitting::at(&running.join("target").join("debug"));
    let held = machine.temp().join("njutest-commands-held");
    std::fs::create_dir_all(held.join("fixture-baseline")).expect("a test's directory");
    aged(&held);
    let holding = Sitting::at(&held.join("fixture-baseline"));
    let across = machine.root().join("across");
    machine.worktree(&across);
    built(&across.join("target"));
    aged(&across.join("target"));
    let beside = Sitting::at(&across);

    let swept = machine.sweep();
    let said = format!(
        "{}{}",
        String::from_utf8(swept.stdout.clone()).expect("the sweep writes text"),
        String::from_utf8(swept.stderr.clone()).expect("the sweep writes text")
    );
    drop(sitting);
    drop(holding);
    drop(beside);
    assert!(swept.status.success(), "{said}");
    assert!(
        present(&running.join("target")),
        "a test binary running out of build output writes nothing there, and taking it from under \
         the run fails every test it starts next: {said}"
    );
    assert!(
        present(&held),
        "a test's directory a process still works in is not a leak however old: {said}"
    );
    assert!(
        present(&across.join("target")),
        "a session sitting in a worktree is about to build or run there: {said}"
    );
}

#[test]
fn a_directory_that_cannot_be_taken_leaves_the_rest_to_be_taken() {
    use std::os::unix::fs::PermissionsExt;
    let machine = Machine::new();
    let stuck = machine.temp().join("njutest-commands-stuck");
    std::fs::create_dir_all(&stuck).expect("a leftover");
    aged(&stuck);
    let later = machine.temp().join("njutest-fixture-later");
    std::fs::create_dir_all(&later).expect("a leftover");
    aged(&later);
    let idle = machine.root().join("idle");
    machine.worktree(&idle);
    built(&idle.join("target"));
    aged(&idle.join("target"));
    let trash = machine.temp().join(".njutest-trash");
    std::fs::create_dir_all(&trash).expect("the trash");
    std::fs::set_permissions(&trash, std::fs::Permissions::from_mode(0o500)).expect("sealed");
    let swept = machine.sweep();
    std::fs::set_permissions(&trash, std::fs::Permissions::from_mode(0o700)).expect("unsealed");
    let said = format!(
        "{}{}",
        String::from_utf8(swept.stdout.clone()).expect("the sweep writes text"),
        String::from_utf8(swept.stderr.clone()).expect("the sweep writes text")
    );
    assert!(
        !present(&idle.join("target")),
        "what the sweep could take it took, though another directory would not go: {said}"
    );
    assert!(
        said.contains("njutest-commands-stuck"),
        "and it names what it could not take rather than stopping at it: {said}"
    );
    assert!(
        !swept.status.success(),
        "a sweep that left something it meant to take says so in its status: {said}"
    );
}

#[test]
fn a_directory_the_product_makes_for_somebody_s_own_run_is_not_this_repository_s() {
    let machine = Machine::new();
    let theirs = machine.temp().join("rust-mutants-target-abc");
    std::fs::create_dir_all(&theirs).expect("a user's run directory");
    aged(&theirs);
    let provider = machine.temp().join("njutest-provider-output-abc");
    std::fs::create_dir_all(&provider).expect("a user's run directory");
    aged(&provider);
    let swept = machine.sweep();
    assert!(swept.status.success());
    assert!(
        present(&theirs) && present(&provider),
        "njutest run on another project makes directories with these names too, and only the \
         names this repository's tests and gates make are its to take"
    );
}

#[test]
fn every_name_the_sweep_takes_is_one_only_this_repository_s_own_tests_and_gates_make() {
    let root = xtask::gates::workspace_root();
    let mut shipped = Vec::new();
    for crate_dir in std::fs::read_dir(root.join("crates")).expect("the crates") {
        let crate_dir = crate_dir.expect("a crate").path();
        if crate_dir.ends_with("njutest-devkit") {
            continue;
        }
        for entry in walkdir::WalkDir::new(crate_dir.join("src")) {
            let entry = entry.expect("a source entry");
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "rs")
            {
                let text = std::fs::read_to_string(entry.path()).expect("a source file");
                for prefix in xtask::sweep::OURS {
                    if text.contains(&format!("\"{prefix}")) {
                        shipped.push(format!("{} names {prefix}", entry.path().display()));
                    }
                }
            }
        }
    }
    assert!(
        shipped.is_empty(),
        "a name the product makes is one it makes for somebody's own run on this machine too, so \
         the sweep would take that run's directory: {shipped:?}"
    );
}
