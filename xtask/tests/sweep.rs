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
