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
        std::fs::write(
            machine.main().join(".gitignore"),
            "/target/\n/.njutest-trash/\n",
        )
        .expect("what the repository ignores");
        git(&machine.main(), &["add", "README", ".gitignore"]);
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
        let origin = machine.scratch.path().join("origin.git");
        git(
            machine.scratch.path(),
            &["init", "-q", "--bare", "-b", "main", "origin.git"],
        );
        git(
            &machine.main(),
            &["remote", "add", "origin", &origin.display().to_string()],
        );
        git(&machine.main(), &["push", "-q", "origin", "main"]);
        git(&machine.main(), &["fetch", "-q", "origin"]);
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

    fn open_work(&self, at: &Path) {
        git(
            &self.main(),
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "open",
                &at.display().to_string(),
            ],
        );
        std::fs::write(at.join("README"), "two\n").expect("a change");
        git(at, &["add", "README"]);
        git(
            at,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "two",
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
    command.env("GIT_CONFIG_NOSYSTEM", "1");
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

/// Claims `directory` as the engine's owners do, holding its lock for as long as the value lives.
fn owned(directory: &Path) -> std::fs::File {
    std::fs::create_dir_all(directory).expect("a directory to own");
    let lock = std::fs::File::create(directory.join("owner.lock")).expect("the owner lock");
    rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive)
        .expect("the lock is ours");
    lock
}

#[test]
fn what_landed_and_what_nobody_owns_goes_and_nothing_else_does() {
    let machine = Machine::new();
    built(&machine.main().join("target"));
    let landed = machine.root().join("landed");
    machine.worktree(&landed);
    built(&landed.join("target"));
    let open = machine.root().join("open");
    machine.open_work(&open);
    built(&open.join("target"));
    let dirty = machine.root().join("dirty");
    machine.worktree(&dirty);
    built(&dirty.join("target"));
    std::fs::write(dirty.join("notes"), "unsaved\n").expect("a change nobody committed");

    let gate = machine.temp().join("njutest-pre-push-abc");
    machine.worktree(&gate.join("tree"));
    built(&gate.join("target"));

    let abandoned = machine.temp().join("njutest-commands-abandoned");
    let released = owned(&abandoned);
    drop(released);
    let running = machine.temp().join("njutest-commands-running");
    let holding = owned(&running);
    let unmarked = machine.temp().join("njutest-fixture-unmarked");
    std::fs::create_dir_all(unmarked.join("fixture-baseline")).expect("a leftover");
    let foreign = machine.temp().join("somebody-else");
    std::fs::create_dir_all(&foreign).expect("another program's directory");

    let swept = machine.sweep();
    drop(holding);
    let said = format!(
        "{}{}",
        String::from_utf8(swept.stdout.clone()).expect("the sweep writes text"),
        String::from_utf8(swept.stderr.clone()).expect("the sweep writes text")
    );
    assert!(swept.status.success(), "{said}");
    assert!(
        !present(&landed.join("target")) && present(&landed.join("README")),
        "a worktree whose head main already holds, with nothing of its own, has landed: its build \
         output is garbage and its source stays: {said}"
    );
    assert!(
        present(&open.join("target")),
        "a worktree with a commit main does not hold is work in progress, and its build is its \
         cache: {said}"
    );
    assert!(
        present(&dirty.join("target")),
        "and one with a change nobody committed has not landed either: {said}"
    );
    assert!(
        present(&machine.main().join("target")),
        "the primary checkout's build is the one every worktree's work comes back to: {said}"
    );
    assert!(
        !present(&gate) && !listed(&machine).contains("njutest-pre-push-abc"),
        "a gate tree nothing uses goes, and git forgets the worktree it was: {said}"
    );
    assert!(
        !present(&abandoned),
        "a directory whose owner's lock nobody holds lost its owner, however it went: {said}"
    );
    assert!(
        present(&running),
        "one whose owner still holds its lock is in use, whenever it was last written: {said}"
    );
    assert!(
        !present(&unmarked),
        "an unmarked leftover nothing on the machine uses is garbage: {said}"
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
fn what_a_process_is_using_stays_however_landed_or_unowned_it_is() {
    let machine = Machine::new();
    let running = machine.root().join("running");
    machine.worktree(&running);
    built(&running.join("target"));
    let sitting = Sitting::at(&running.join("target").join("debug"));
    let held = machine.temp().join("njutest-commands-held");
    std::fs::create_dir_all(held.join("fixture-baseline")).expect("a test's directory");
    let holding = Sitting::at(&held.join("fixture-baseline"));
    let across = machine.root().join("across");
    machine.worktree(&across);
    built(&across.join("target"));
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
    let later = machine.temp().join("njutest-fixture-later");
    std::fs::create_dir_all(&later).expect("a leftover");
    let idle = machine.root().join("idle");
    machine.worktree(&idle);
    built(&idle.join("target"));
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
    let provider = machine.temp().join("njutest-provider-output-abc");
    std::fs::create_dir_all(&provider).expect("a user's run directory");
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
