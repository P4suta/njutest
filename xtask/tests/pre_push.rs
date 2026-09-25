// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The pre-push gate proves that the tree it checks is the exact object Git is pushing.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "a fixture that cannot be constructed leaves no pre-push protocol to test"
)]

use std::ffi::OsString;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use njutest_devkit::process::SupervisedChild;
use njutest_devkit::result::{ResultState, result_state};

const SAYS_A_GREAT_DEAL: &str = "printf '%s\\n' \"$*\" >> \"$CALLS\"; case \"$*\" in 'run check') head -c 1048576 /dev/zero | tr '\\0' x; echo ;; *) exit 99 ;; esac";

const ACCEPTS_THE_CHECK: &str =
    "printf '%s\\n' \"$*\" >> \"$CALLS\"; case \"$*\" in 'run check') ;; *) exit 99 ;; esac";

struct Repository {
    directory: tempfile::TempDir,
    _commands: tempfile::TempDir,
    scratch: tempfile::TempDir,
    head: String,
    path: OsString,
    slots: PathBuf,
    turns: PathBuf,
    base: Option<String>,
}

impl Repository {
    fn new(check: &str) -> Self {
        let directory = tempfile::tempdir().expect("a temporary repository");
        command(directory.path(), "git", &["init", "--quiet"]);
        command(
            directory.path(),
            "git",
            &["config", "user.name", "pre-push-test"],
        );
        command(
            directory.path(),
            "git",
            &["config", "user.email", "pre-push@example.invalid"],
        );
        command(
            directory.path(),
            "git",
            &["config", "commit.gpgsign", "false"],
        );
        std::fs::write(
            directory.path().join(".gitignore"),
            "/target/\nlocal-only\n",
        )
        .expect("an ignored local-input name");
        std::fs::write(directory.path().join("tracked"), "before\n").expect("a tracked file");
        command(directory.path(), "git", &["add", ".gitignore", "tracked"]);
        command(
            directory.path(),
            "git",
            &["commit", "--quiet", "-m", "fixture"],
        );
        let head = String::from_utf8(
            isolated("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(directory.path())
                .output()
                .expect("git rev-parse")
                .stdout,
        )
        .expect("an ASCII object id")
        .trim()
        .to_owned();

        let commands = tempfile::tempdir().expect("a private bin directory");
        let bin = commands.path().to_path_buf();
        let mise = bin.join("mise");
        std::fs::write(
            &mise,
            format!("#!/usr/bin/env bash\nset -euo pipefail\n{check}\n"),
        )
        .expect("a scripted check");
        executable(&mise);
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let path = std::env::join_paths(paths).expect("a PATH for the scripted check");
        let scratch =
            tempfile::tempdir().expect("a temporary directory the gate keeps its tree in");
        let slots = scratch.path().join("slots");
        let turns = scratch.path().join("turns");
        std::fs::create_dir_all(&turns).expect("a directory the scripted checks take turns in");

        Self {
            directory,
            _commands: commands,
            scratch,
            head,
            path,
            slots,
            turns,
            base: None,
        }
    }

    fn sharing_the_machine_with(mut self, other: &Self) -> Self {
        self.slots.clone_from(&other.slots);
        self.turns.clone_from(&other.turns);
        self
    }

    fn calls(&self) -> usize {
        let log = self.scratch.path().join("calls");
        if !log.try_exists().expect("the call log can be looked for") {
            return 0;
        }
        std::fs::read_to_string(&log)
            .expect("the scripted check's call log")
            .lines()
            .count()
    }

    fn link(&self, name: &str) -> (PathBuf, String) {
        let linked = self.scratch.path().join(name);
        command(
            self.directory.path(),
            "git",
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                name,
                linked.to_str().expect("a UTF-8 scratch path"),
            ],
        );
        std::fs::write(linked.join("tracked"), format!("{name}\n")).expect("a linked revision");
        command(&linked, "git", &["commit", "--quiet", "-am", name]);
        let head = object_id(&linked, "HEAD");
        (linked, head)
    }

    fn push(&self, local: &str) -> Output {
        self.push_over(local, &"0".repeat(40))
    }

    fn push_over(&self, local: &str, remote: &str) -> Output {
        self.push_terminated(local, remote, "\n")
    }

    fn push_terminated(&self, local: &str, remote: &str, end: &str) -> Output {
        self.push_with(self.directory.path(), &update(local, remote, end), &[])
    }

    fn push_from(&self, directory: &Path, local: &str) -> Output {
        self.push_with(directory, &update(local, &"0".repeat(40), "\n"), &[])
    }

    fn push_as_a_hook(&self, local: &str) -> Output {
        let git = self.directory.path().join(".git");
        self.push_with(
            self.directory.path(),
            &update(local, &"0".repeat(40), "\n"),
            &[("GIT_DIR", git.as_os_str()), ("GIT_PREFIX", "".as_ref())],
        )
    }

    fn push_with(
        &self,
        directory: &Path,
        line: &str,
        handed: &[(&str, &std::ffi::OsStr)],
    ) -> Output {
        self.launch(directory, line, handed)
            .wait_with_output()
            .expect("the gate's answer")
    }

    fn launch(
        &self,
        directory: &Path,
        line: &str,
        handed: &[(&str, &std::ffi::OsStr)],
    ) -> SupervisedChild {
        Self::start(self.command(directory, handed, Stdio::piped()), line)
    }

    fn launch_to(&self, directory: &Path, line: &str, told: Stdio) -> SupervisedChild {
        Self::start(self.command(directory, &[], told), line)
    }

    fn command(
        &self,
        directory: &Path,
        handed: &[(&str, &std::ffi::OsStr)],
        told: Stdio,
    ) -> Command {
        let mut command = isolated(env!("CARGO_BIN_EXE_xtask"));
        for (name, _value) in std::env::vars_os() {
            if xtask::prepush::shapes_the_build(&name) {
                command.env_remove(name);
            }
        }
        command
            .arg("pre-push")
            .current_dir(directory)
            .env("PATH", &self.path)
            .env("TMPDIR", self.scratch.path())
            .env("NJUTEST_PRE_PUSH_CACHE", self.scratch.path().join("cache"))
            .env("NJUTEST_SLOT_DIR", &self.slots)
            .env_remove("NJUTEST_SLOT_HELD")
            .env("CALLS", self.scratch.path().join("calls"))
            .env("TURNS", &self.turns)
            .env("EXPECTED_HEAD", &self.head)
            .envs(handed.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(told);
        if let Some(base) = &self.base {
            command.env("NJUTEST_COMMITTED_BASE_REF", base);
        }
        command
    }

    fn start(mut command: Command, line: &str) -> SupervisedChild {
        let mut child = SupervisedChild::launch(&mut command).expect("the pre-push gate");
        child
            .take_stdin()
            .expect("the gate's stdin")
            .write_all(line.as_bytes())
            .expect("a ref update");
        child
    }
}

/// Every directory under `root` that holds a checkout the gate made for itself.
fn gate_trees(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0_u8)];
    while let Some((directory, depth)) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("a readable scratch directory") {
            let entry = entry.expect("a readable scratch entry");
            if !entry.file_type().expect("an entry's type").is_dir() {
                continue;
            }
            let path = entry.path();
            let checkout = path
                .join(".git")
                .try_exists()
                .expect("a checkout marker can be looked for");
            if entry.file_name() == "tree" && checkout {
                found.push(path);
            } else if depth < 4 {
                pending.push((path, depth.saturating_add(1)));
            }
        }
    }
    found
}

/// Waits until `ready` says so, or `limit` passes, and says which.
fn until(limit: Duration, mut ready: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        if ready() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    ready()
}

fn present(path: &Path) -> bool {
    path.try_exists().expect("a marker can be looked for")
}

fn waiting_in(slots: &Path) -> bool {
    if !slots
        .try_exists()
        .expect("the lane directory can be looked for")
    {
        return false;
    }
    std::fs::read_dir(slots)
        .expect("a readable lane directory")
        .map(|entry| entry.expect("a readable lane entry").file_name())
        .any(|name| {
            name.to_str()
                .expect("a UTF-8 lane entry")
                .starts_with("heavy.waiting.")
        })
}

fn update(local: &str, remote: &str, end: &str) -> String {
    format!("refs/heads/local {local} refs/heads/remote {remote}{end}")
}

fn symbolic_head(directory: &Path) -> String {
    String::from_utf8(
        isolated("git")
            .args(["symbolic-ref", "--quiet", "HEAD"])
            .current_dir(directory)
            .output()
            .expect("git symbolic-ref")
            .stdout,
    )
    .expect("an ASCII ref name")
    .trim()
    .to_owned()
}

fn object_id(directory: &Path, revision: &str) -> String {
    String::from_utf8(
        isolated("git")
            .args(["rev-parse", revision])
            .current_dir(directory)
            .output()
            .expect("git rev-parse")
            .stdout,
    )
    .expect("an ASCII object id")
    .trim()
    .to_owned()
}

/// A command with none of the `GIT_*` variables a hook hands down, so it touches only the repository it runs in.
fn isolated(program: &str) -> Command {
    let mut command = Command::new(program);
    for (name, _) in std::env::vars_os() {
        if name.as_encoded_bytes().starts_with(b"GIT_") {
            command.env_remove(name);
        }
    }
    command.env("GIT_CONFIG_GLOBAL", "/dev/null");
    command
}

fn command(directory: &Path, program: &str, arguments: &[&str]) {
    let status = isolated(program)
        .args(arguments)
        .current_dir(directory)
        .status()
        .expect("the fixture command");
    assert!(status.success(), "{program} {arguments:?}: {status}");
}

fn executable(path: &Path) {
    let mut permissions = std::fs::metadata(path)
        .expect("the scripted check")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("an executable scripted check");
}

fn stderr(output: &Output) -> String {
    let decoded = String::from_utf8(output.stderr.clone());
    if let Err(error) = &decoded {
        assert_eq!(
            result_state(&decoded),
            ResultState::Returned,
            "the hook wrote non-UTF-8 stderr; bytes: {:02x?}",
            error.as_bytes()
        );
    }
    match decoded {
        Ok(stderr) => stderr,
        Err(_already_reported) => String::new(),
    }
}

#[test]
fn a_clean_exact_head_is_checked() {
    let repository =
        Repository::new("case \"$*\" in 'run check'|'run check:cold') ;; *) exit 99 ;; esac");
    let output = repository.push(&repository.head);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn the_check_is_handed_none_of_the_hooks_git_environment() {
    let repository = Repository::new(
        "if compgen -e | grep -q '^GIT_'; then compgen -e | grep '^GIT_' >&2; exit 98; fi",
    );
    let output = repository.push_as_a_hook(&repository.head);
    assert!(
        output.status.success(),
        "a check that inherits GIT_DIR answers about the repository being pushed, and a test \
         inside it that runs `git init` or `git config` rewrites that repository: {}",
        stderr(&output)
    );
}

#[test]
fn a_push_leaves_the_pushers_head_on_its_branch() {
    let repository =
        Repository::new("case \"$*\" in 'run check'|'run check:cold') ;; *) exit 99 ;; esac");
    let before = symbolic_head(repository.directory.path());
    let first = repository.push_as_a_hook(&repository.head);
    assert!(
        first.status.success(),
        "the one that makes the gate's tree: {}",
        stderr(&first)
    );
    std::fs::write(repository.directory.path().join("tracked"), "after\n")
        .expect("a second revision");
    command(
        repository.directory.path(),
        "git",
        &["commit", "--quiet", "-am", "second"],
    );
    let second = object_id(repository.directory.path(), "HEAD");
    let reused = repository.push_as_a_hook(&second);
    assert!(
        reused.status.success(),
        "the one that reuses it: {}",
        stderr(&reused)
    );
    assert_eq!(
        symbolic_head(repository.directory.path()),
        before,
        "the gate's own checkout ran with the hook's GIT_DIR and moved the pusher's HEAD instead \
         of the isolated tree's"
    );
}

#[test]
fn a_different_object_is_refused_before_the_check() {
    let repository = Repository::new("exit 99");
    let output = repository.push(&"1".repeat(40));
    assert!(!output.status.success());
    assert!(stderr(&output).contains("but the checked-out commit is"));
}

#[test]
fn an_existing_remote_ancestor_is_accepted() {
    let repository =
        Repository::new("case \"$*\" in 'run check'|'run check:cold') ;; *) exit 99 ;; esac");
    std::fs::write(repository.directory.path().join("tracked"), "after\n")
        .expect("a second revision");
    command(repository.directory.path(), "git", &["add", "tracked"]);
    command(
        repository.directory.path(),
        "git",
        &["commit", "--quiet", "-m", "fast-forward"],
    );
    let local = object_id(repository.directory.path(), "HEAD");

    let output = repository.push_over(&local, &repository.head);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn a_ref_update_nothing_terminates_is_the_same_ref_update() {
    let repository =
        Repository::new("case \"$*\" in 'run check'|'run check:cold') ;; *) exit 99 ;; esac");
    std::fs::write(repository.directory.path().join("tracked"), "after\n")
        .expect("a second revision");
    command(repository.directory.path(), "git", &["add", "tracked"]);
    command(
        repository.directory.path(),
        "git",
        &["commit", "--quiet", "-m", "fast-forward"],
    );
    let local = object_id(repository.directory.path(), "HEAD");

    let output = repository.push_terminated(&local, &repository.head, "");
    assert!(
        output.status.success(),
        "the dispatcher that feeds this gate captures the ref list with `$(cat)` and \
         replays it with `printf '%s'`, so the last line arrives with nothing after it. \
         A gate that drops it sees an empty push and refuses every one of them, which is \
         what it did: {}",
        stderr(&output)
    );
}

#[test]
fn a_non_fast_forward_update_is_refused_before_the_check() {
    let repository = Repository::new("exit 99");
    command(
        repository.directory.path(),
        "git",
        &["branch", "remote", "HEAD"],
    );
    std::fs::write(repository.directory.path().join("tracked"), "local\n")
        .expect("the local revision");
    command(repository.directory.path(), "git", &["add", "tracked"]);
    command(
        repository.directory.path(),
        "git",
        &["commit", "--quiet", "-m", "local"],
    );
    let local = object_id(repository.directory.path(), "HEAD");
    command(
        repository.directory.path(),
        "git",
        &["checkout", "--quiet", "remote"],
    );
    std::fs::write(repository.directory.path().join("tracked"), "remote\n")
        .expect("the remote revision");
    command(repository.directory.path(), "git", &["add", "tracked"]);
    command(
        repository.directory.path(),
        "git",
        &["commit", "--quiet", "-m", "remote"],
    );
    let remote = object_id(repository.directory.path(), "HEAD");
    command(
        repository.directory.path(),
        "git",
        &["checkout", "--quiet", "--detach", &local],
    );

    let output = repository.push_over(&local, &remote);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("non-fast-forward updates are forbidden"));
}

#[test]
fn an_unknown_remote_commit_is_refused_before_the_check() {
    let repository = Repository::new("exit 99");
    let output = repository.push_over(&repository.head, &"1".repeat(40));
    assert!(!output.status.success());
    assert!(stderr(&output).contains("which is not present locally"));
}

#[test]
fn tracked_adjacent_and_ignored_edits_cannot_contaminate_the_isolated_check() {
    let repository = Repository::new(
        "test ! -e local-only && test ! -e adjacent && test \"$(cat tracked)\" = before",
    );
    std::fs::write(repository.directory.path().join("tracked"), "after\n")
        .expect("an uncommitted tracked edit");
    std::fs::write(
        repository.directory.path().join("local-only"),
        "ignored input\n",
    )
    .expect("an ignored file");
    std::fs::write(
        repository.directory.path().join("adjacent"),
        "untracked input\n",
    )
    .expect("an adjacent file");
    let output = repository.push(&repository.head);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn a_tree_changed_by_the_check_is_refused_afterwards() {
    let repository = Repository::new("printf 'after\\n' >> tracked");
    let output = repository.push(&repository.head);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("isolated check changed the tree"));
}

#[test]
fn a_test_push_keeps_its_gate_tree_inside_what_the_test_handed_it() {
    let repository = Repository::new(ACCEPTS_THE_CHECK);
    let output = repository.push(&repository.head);
    assert!(output.status.success(), "{}", stderr(&output));
    let trees = gate_trees(repository.scratch.path());
    assert_eq!(
        trees.len(),
        1,
        "the gate keeps one tree per repository on purpose, as the cache the next push reuses, \
         and a test's repository is new every run, so a tree anywhere but the directories the \
         test handed it is one more per run that nothing ever removes: {trees:?}"
    );
}

#[test]
fn every_worktree_of_a_repository_shares_one_gate_tree() {
    let repository = Repository::new(ACCEPTS_THE_CHECK);
    let (linked, linked_head) = repository.link("linked");
    let main = repository.push(&repository.head);
    assert!(main.status.success(), "{}", stderr(&main));
    let other = repository.push_from(&linked, &linked_head);
    assert!(other.status.success(), "{}", stderr(&other));
    let trees = gate_trees(repository.scratch.path());
    assert_eq!(
        trees.len(),
        1,
        "each worktree of one repository compiled the workspace from nothing in a gate tree of \
         its own, which is what made every push from a fresh worktree a cold build: {trees:?}"
    );
}

#[test]
fn a_commit_that_passed_is_not_checked_again() {
    let repository = Repository::new(ACCEPTS_THE_CHECK);
    let first = repository.push(&repository.head);
    assert!(first.status.success(), "{}", stderr(&first));
    let checked = repository.calls();
    assert!(checked > 0, "the first push never ran the check");
    let again = repository.push(&repository.head);
    assert!(again.status.success(), "{}", stderr(&again));
    assert_eq!(
        repository.calls(),
        checked,
        "the same commit against the same base ran the whole gate a second time, which is what \
         a push to the hub and then to origin paid for every commit: {}",
        stderr(&again)
    );
    assert!(
        stderr(&again).contains("already passed"),
        "a gate that did not run says so rather than looking like one that did: {}",
        stderr(&again)
    );
}

#[test]
fn a_commit_is_checked_again_once_its_base_has_moved() {
    let mut repository = Repository::new(ACCEPTS_THE_CHECK);
    command(
        repository.directory.path(),
        "git",
        &["branch", "base", "HEAD"],
    );
    repository.base = Some("refs/heads/base".to_owned());
    let first = repository.push(&repository.head);
    assert!(first.status.success(), "{}", stderr(&first));
    let checked = repository.calls();
    command(
        repository.directory.path(),
        "git",
        &["commit", "--quiet", "--allow-empty", "-m", "moved"],
    );
    let moved = object_id(repository.directory.path(), "HEAD");
    command(
        repository.directory.path(),
        "git",
        &["branch", "-f", "base", &moved],
    );
    command(
        repository.directory.path(),
        "git",
        &["checkout", "--quiet", "--detach", &repository.head],
    );
    let again = repository.push(&repository.head);
    assert!(again.status.success(), "{}", stderr(&again));
    assert!(
        repository.calls() > checked,
        "a pass is an answer about a commit against one base, and the commit-range check reads \
         the base, so a moved base is a question nobody has answered yet: {}",
        stderr(&again)
    );
}

#[test]
fn a_check_is_told_which_commit_it_answers_for() {
    let repository = Repository::new(
        "case \"$*\" in 'run check') ;; *) exit 99 ;; esac; \
         test \"$NJUTEST_COMMITTED_HEAD\" = \"$EXPECTED_HEAD\" || exit 96",
    );
    let output = repository.push(&repository.head);
    assert!(
        output.status.success(),
        "the commit-message check answers for whatever NJUTEST_COMMITTED_HEAD names, so the \
         gate has to name the exact object being pushed: {}",
        stderr(&output)
    );
}

#[test]
fn two_gates_take_turns_on_one_machine() {
    let taking_turns = "mkdir \"$TURNS/inside\" 2>/dev/null || { : > \"$TURNS/overlapped\"; exit 97; }; \
         while [ ! -e \"$TURNS/go\" ]; do sleep 0.05; done; \
         rmdir \"$TURNS/inside\"";
    let first = Repository::new(taking_turns);
    let second = Repository::new(taking_turns).sharing_the_machine_with(&first);
    let line = |repository: &Repository| update(&repository.head, &"0".repeat(40), "\n");

    let running = first.launch(first.directory.path(), &line(&first), &[]);
    assert!(
        until(Duration::from_secs(120), || present(
            &first.turns.join("inside")
        )),
        "the first gate never started its check"
    );
    let queued = second.launch(second.directory.path(), &line(&second), &[]);
    assert!(
        until(Duration::from_secs(120), || waiting_in(&second.slots)
            || present(&second.turns.join("overlapped"))),
        "the second gate neither waited nor ran"
    );
    std::fs::write(first.turns.join("go"), "").expect("the first check's release");

    let first_answer = running.wait_with_output().expect("the first gate's answer");
    let second_answer = queued.wait_with_output().expect("the second gate's answer");
    assert!(
        !present(&second.turns.join("overlapped")),
        "the second gate ran its check while the first was still inside it, and two whole-workspace \
         runs at once is what made every test with a bound fail for reasons about the machine: {}",
        stderr(&second_answer)
    );
    assert!(first_answer.status.success(), "{}", stderr(&first_answer));
    assert!(second_answer.status.success(), "{}", stderr(&second_answer));
    let waited = stderr(&second_answer);
    let holder = std::fs::canonicalize(first.directory.path()).expect("the first checkout");
    assert!(
        waited.contains("waiting for the heavy lane")
            && waited.contains(holder.to_str().expect("a UTF-8 checkout path")),
        "a gate that waits says what it is waiting for: {waited}"
    );
}

#[test]
fn a_check_that_outlives_its_budget_is_stopped_with_everything_it_started() {
    let repository = Repository::new(
        "printf '%s\\n' \"$*\" >> \"$CALLS\"; \
         if [ \"$(wc -l < \"$CALLS\")\" -gt 0 ]; then \
           ( trap '' TERM; exec sleep 600 ) & printf '%s\\n' $! > \"$TURNS/sleeper\"; wait; \
         fi",
    );
    let mut command = isolated(env!("CARGO_BIN_EXE_xtask"));
    command
        .arg("pre-push")
        .current_dir(repository.directory.path())
        .env("PATH", &repository.path)
        .env("TMPDIR", repository.scratch.path())
        .env(
            "NJUTEST_PRE_PUSH_CACHE",
            repository.scratch.path().join("cache"),
        )
        .env("NJUTEST_SLOT_DIR", &repository.slots)
        .env_remove("NJUTEST_SLOT_HELD")
        .env("CALLS", repository.scratch.path().join("calls"))
        .env("TURNS", &repository.turns)
        .env("NJUTEST_PUSH_BUDGET_SECONDS", "2")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = SupervisedChild::launch(&mut command).expect("the pre-push gate");
    child
        .take_stdin()
        .expect("the gate's stdin")
        .write_all(update(&repository.head, &"0".repeat(40), "\n").as_bytes())
        .expect("a ref update");
    let output = child.wait_with_output().expect("the gate's answer");
    assert_eq!(
        output.status.code(),
        Some(124),
        "a check past its budget is a report about the gate: {}",
        stderr(&output)
    );
    assert!(stderr(&output).contains("budget"), "{}", stderr(&output));
    let sleeper = std::fs::read_to_string(repository.turns.join("sleeper"))
        .expect("the budgeted check started what it was to be stopped with");
    let alive = isolated("kill")
        .args(["-0", sleeper.trim()])
        .status()
        .expect("kill -0");
    assert!(
        !alive.success(),
        "the budget stopped the check but left what the check started running, so the next push \
         shares the machine with a build nobody is waiting for"
    );
}

const TAKES_TURNS: &str = "printf '%s\\n' \"$*\" >> \"$CALLS\"; \
     mkdir \"$TURNS/inside\" 2>/dev/null || { : > \"$TURNS/overlapped\"; exit 97; }; \
     while [ ! -e \"$TURNS/go\" ]; do sleep 0.05; done; \
     rmdir \"$TURNS/inside\"";

/// Whether any file under `root` is a run waiting for the lane `lane`.
fn waiting_under(root: &Path, lane: &str) -> bool {
    let prefix = format!("{lane}.waiting.");
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        if !present(&directory) {
            continue;
        }
        for entry in std::fs::read_dir(&directory).expect("a readable scratch directory") {
            let entry = entry.expect("a readable scratch entry");
            let name = entry.file_name();
            let name = name.to_str().expect("a UTF-8 scratch name");
            if name.starts_with(&prefix) {
                return true;
            }
            if entry.file_type().expect("an entry's type").is_dir() && name != "tree" {
                pending.push(entry.path());
            }
        }
    }
    false
}

#[test]
fn a_second_push_of_a_commit_that_passed_while_it_waited_is_not_checked_again() {
    let repository = Repository::new(TAKES_TURNS);
    let line = update(&repository.head, &"0".repeat(40), "\n");
    let first = repository.launch(repository.directory.path(), &line, &[]);
    assert!(
        until(Duration::from_secs(120), || present(
            &repository.turns.join("inside")
        )),
        "the first gate never started its check"
    );
    let second = repository.launch(repository.directory.path(), &line, &[]);
    assert!(
        until(Duration::from_secs(120), || waiting_in(&repository.slots)),
        "the second gate did not queue behind the first"
    );
    std::fs::write(repository.turns.join("go"), "").expect("the first check's release");
    let first = first.wait_with_output().expect("the first gate's answer");
    let calls = repository.calls();
    let second = second.wait_with_output().expect("the second gate's answer");
    assert!(first.status.success(), "{}", stderr(&first));
    assert!(second.status.success(), "{}", stderr(&second));
    assert_eq!(
        repository.calls(),
        calls,
        "the second push waited for the first and then checked the same commit again: {}",
        stderr(&second)
    );
    assert!(
        stderr(&second).contains("already passed"),
        "{}",
        stderr(&second)
    );
}

#[test]
fn two_gates_of_one_repository_take_turns_even_inside_a_held_lane() {
    let repository = Repository::new(TAKES_TURNS);
    let (linked, linked_head) = repository.link("linked");
    let held: [(&str, &std::ffi::OsStr); 1] = [("NJUTEST_SLOT_HELD", "heavy".as_ref())];
    let first = repository.launch(
        repository.directory.path(),
        &update(&repository.head, &"0".repeat(40), "\n"),
        &held,
    );
    assert!(
        until(Duration::from_secs(120), || present(
            &repository.turns.join("inside")
        )),
        "the first gate never started its check"
    );
    let second = repository.launch(&linked, &update(&linked_head, &"0".repeat(40), "\n"), &held);
    assert!(
        until(Duration::from_secs(120), || {
            waiting_under(&repository.scratch.path().join("cache"), "tree")
                || present(&repository.turns.join("overlapped"))
        }),
        "the second gate neither waited nor ran"
    );
    std::fs::write(repository.turns.join("go"), "").expect("the first check's release");
    let first = first.wait_with_output().expect("the first gate's answer");
    let second = second.wait_with_output().expect("the second gate's answer");
    assert!(
        !present(&repository.turns.join("overlapped")),
        "two gates wrote one repository's tree at once because the environment said the heavy \
         lane was already held: {}",
        stderr(&second)
    );
    assert!(first.status.success(), "{}", stderr(&first));
    assert!(second.status.success(), "{}", stderr(&second));
}

#[test]
fn a_commit_is_checked_again_under_different_build_settings() {
    let repository = Repository::new(ACCEPTS_THE_CHECK);
    let first = repository.push(&repository.head);
    assert!(first.status.success(), "{}", stderr(&first));
    let checked = repository.calls();
    let flags: [(&str, &std::ffi::OsStr); 1] = [("RUSTFLAGS", "-D warnings".as_ref())];
    let again = repository.push_with(
        repository.directory.path(),
        &update(&repository.head, &"0".repeat(40), "\n"),
        &flags,
    );
    assert!(again.status.success(), "{}", stderr(&again));
    assert!(
        repository.calls() > checked,
        "a pass under one set of build flags answered for another: {}",
        stderr(&again)
    );
}

#[test]
fn a_push_runs_the_check_once() {
    let repository = Repository::new(ACCEPTS_THE_CHECK);
    let output = repository.push(&repository.head);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        repository.calls(),
        1,
        "the gate ran the whole check twice, once to warm and once to measure, which is the \
         suite, the doctests, Kani, deny and audit twice for every push: {}",
        stderr(&output)
    );
}

#[test]
fn a_check_that_goes_quiet_is_stopped_and_one_that_talks_is_not() {
    let quiet: [(&str, &std::ffi::OsStr); 1] = [("NJUTEST_PUSH_QUIET_SECONDS", "2".as_ref())];
    let silent = Repository::new("sleep 30");
    let stopped = silent.push_with(
        silent.directory.path(),
        &update(&silent.head, &"0".repeat(40), "\n"),
        &quiet,
    );
    assert_eq!(
        stopped.status.code(),
        Some(124),
        "a check that said nothing past its quiet ran on: {}",
        stderr(&stopped)
    );
    assert!(
        stderr(&stopped).contains("said nothing"),
        "{}",
        stderr(&stopped)
    );

    let talking =
        Repository::new("for i in 1 2 3 4 5 6 7 8; do echo \"still working $i\"; sleep 0.5; done");
    let finished = talking.push_with(
        talking.directory.path(),
        &update(&talking.head, &"0".repeat(40), "\n"),
        &quiet,
    );
    assert!(
        finished.status.success(),
        "a check that kept saying something was stopped for how long it took (ADR 0026): {}",
        stderr(&finished)
    );
    let heard = format!(
        "{}{}",
        String::from_utf8(finished.stdout.clone()).expect("UTF-8 output"),
        stderr(&finished)
    );
    assert!(
        heard.contains("still working 8"),
        "what the check said did not reach the person pushing: {heard}"
    );
}

#[test]
fn a_hook_whose_stderr_takes_no_more_still_answers_for_the_check() {
    let repository = Repository::new(SAYS_A_GREAT_DEAL);
    let (mut reader, writer) = std::io::pipe().expect("a pipe for the hook's stderr");
    let flags = rustix::fs::fcntl_getfl(&writer).expect("the pipe's flags");
    rustix::fs::fcntl_setfl(&writer, flags | rustix::fs::OFlags::NONBLOCK)
        .expect("a pipe that refuses rather than waits, as git's can");
    let child = repository.launch_to(
        repository.directory.path(),
        &update(&repository.head, &"0".repeat(40), "\n"),
        Stdio::from(writer),
    );
    let answered = child.wait_with_output().expect("the gate's answer");
    let mut shown = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut shown).expect("what the hook showed");
    let shown = String::from_utf8(shown).expect("the hook writes text");
    assert!(
        answered.status.success(),
        "a check that passed passed, however much of its output a full pipe refused: showing the \
         output is not the check. {}: {}",
        answered.status,
        shown
            .lines()
            .filter(|line| line.starts_with("pre-push:"))
            .collect::<Vec<_>>()
            .join(" / ")
    );
}
