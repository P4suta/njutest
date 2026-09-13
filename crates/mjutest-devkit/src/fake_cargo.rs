// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A cargo, a rustc, a coverage tool, or a test binary that says what a script told it to say.
//!
//! The engine's process boundary is a handful of command lines and what comes
//! back from them. A test that drives a real toolchain across it measures
//! cargo; a test that drives this measures the engine.

#![expect(
    clippy::expect_used,
    reason = "support for tests reports a setup failure by panicking: a script that cannot be \
              written or a fake that was never built leaves a test with nothing to assert"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The one variable the fake reads: where its script is.
pub const SCRIPT_ENV: &str = "RUST_MUTANTS_FAKE_CARGO_SCRIPT";

/// What the fake exits with when no entry of its script matches the command it was started as, so an unscripted invocation fails loudly rather than looking like a tool that ran.
pub const UNMATCHED_EXIT: u8 = 99;

/// The document a fake reads.
pub const SCHEMA: &str = "rust-mutants-fake-cargo-v1";

/// Everything a fake will answer to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Script {
    /// [`SCHEMA`].
    pub schema: String,
    /// The invocations it knows, in the order they are matched.
    pub invocations: Vec<Invocation>,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            schema: SCHEMA.to_owned(),
            invocations: Vec::new(),
        }
    }
}

impl Script {
    /// A script that answers nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one invocation to the end, which is where it is matched from.
    #[must_use]
    pub fn answering(mut self, invocation: Invocation) -> Self {
        self.invocations.push(invocation);
        self
    }
}

/// One command the fake answers, and what it answers with.
///
/// A command matches when the program it was started as is `program`, every
/// element of `args_prefix` is in front of its arguments, and the environment
/// conditions hold. The first matching entry that has not run out of `times`
/// answers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invocation {
    /// The file stem of the program: `cargo`, `rustc`, `llvm-profdata`, or the name of a test binary.
    pub program: String,
    /// What has to be in front of the arguments.
    #[serde(default)]
    pub args_prefix: Vec<String>,
    /// Variables that have to be set, whatever their value.
    #[serde(default)]
    pub env_has: Vec<String>,
    /// Variables that have to be unset.
    #[serde(default)]
    pub env_lacks: Vec<String>,
    /// Variables that have to hold exactly this value, which is how one mutant's execution is told from another's.
    #[serde(default)]
    pub env_is: Vec<(String, String)>,
    /// What it prints on stdout.
    #[serde(default)]
    pub stdout: String,
    /// A file whose bytes it prints on stdout instead, for output larger than a script should hold.
    #[serde(default)]
    pub stdout_file: Option<PathBuf>,
    /// What it prints on stderr.
    #[serde(default)]
    pub stderr: String,
    /// What it exits with.
    #[serde(default)]
    pub exit: u8,
    /// How long it takes before it answers, which is how a timeout is provoked.
    #[serde(default)]
    pub delay_ms: u64,
    /// Files it writes before it answers, which is how an artifact a build would have left behind gets there.
    #[serde(default)]
    pub writes: Vec<WriteFile>,
    /// How many times it may answer. `None` is every time.
    #[serde(default)]
    pub times: Option<u32>,
}

impl Invocation {
    /// An invocation of `program` that answers to arguments beginning with `args`.
    #[must_use]
    pub fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args_prefix: args.iter().map(|arg| (*arg).to_owned()).collect(),
            env_has: Vec::new(),
            env_lacks: Vec::new(),
            env_is: Vec::new(),
            stdout: String::new(),
            stdout_file: None,
            stderr: String::new(),
            exit: 0,
            delay_ms: 0,
            writes: Vec::new(),
            times: None,
        }
    }

    /// Prints `text` on stdout.
    #[must_use]
    pub fn printing(mut self, text: &str) -> Self {
        text.clone_into(&mut self.stdout);
        self
    }

    /// Prints `text` on stderr and exits with `code`.
    #[must_use]
    pub fn failing(mut self, code: u8, text: &str) -> Self {
        self.exit = code;
        text.clone_into(&mut self.stderr);
        self
    }

    /// Takes `millis` before it answers.
    #[must_use]
    pub const fn taking(mut self, millis: u64) -> Self {
        self.delay_ms = millis;
        self
    }

    /// Answers only when every named variable is set.
    #[must_use]
    pub fn when_set(mut self, names: &[&str]) -> Self {
        self.env_has = names.iter().map(|name| (*name).to_owned()).collect();
        self
    }

    /// Answers only when `name` holds `value`.
    #[must_use]
    pub fn when(mut self, name: &str, value: &str) -> Self {
        self.env_is.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Writes `contents` at `path` before answering.
    #[must_use]
    pub fn writing(mut self, path: &str, contents: &str) -> Self {
        self.writes.push(WriteFile {
            path: path.to_owned(),
            contents: contents.to_owned(),
        });
        self
    }
}

/// A file the fake writes before it answers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFile {
    /// Where, with the placeholders of [`install`] expanded.
    pub path: String,
    /// What, with the same placeholders expanded.
    pub contents: String,
}

/// A script written to disk, and the directory of programs that read it.
#[derive(Debug)]
pub struct Installed {
    bin: PathBuf,
    script: PathBuf,
    log: PathBuf,
    _dir: tempfile::TempDir,
}

impl Installed {
    /// The directory holding every program the fake answers as, which is what a test puts on the search path.
    #[must_use]
    pub fn bin(&self) -> &Path {
        &self.bin
    }

    /// The fake as `cargo`, which is what [`crate::paths::cargo_binary`] would otherwise be.
    #[must_use]
    pub fn cargo(&self) -> PathBuf {
        self.bin.join(exe("cargo"))
    }

    /// The environment a run has to start with for the fake to find its script.
    #[must_use]
    pub fn env(&self) -> Vec<(OsString, OsString)> {
        vec![(
            OsString::from(SCRIPT_ENV),
            OsString::from(self.script.as_os_str()),
        )]
    }

    /// Which entries answered, in order, so a test can say what the engine asked for.
    ///
    /// # Panics
    /// When the log cannot be read.
    #[must_use]
    pub fn answered(&self) -> Vec<usize> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.trim().parse().ok())
            .collect()
    }
}

/// The fake, as `cargo build --examples` leaves it beside the test binaries.
///
/// No test build produces it. `cargo test --all-targets` builds an example as
/// a libtest harness — a binary that prints "running 0 tests" and exits —
/// rather than as the program it is, and cargo guarantees a plainly named
/// binary to an integration test only for a `[[bin]]` of the same package.
/// Every task and job that runs this suite therefore builds it first, and
/// where one did not, this builds it once rather than failing a suite for the
/// want of a link step. A tree the engine copied and built is such a place:
/// its build produced the test binaries and no example.
///
/// # Panics
/// When the example is not there and cannot be built, with the command that
/// builds it.
#[must_use]
pub fn locate() -> PathBuf {
    let current = std::env::current_exe().expect("the test binary's own path");
    let deps = current.parent().expect("the deps directory");
    let profile = deps.parent().expect("the profile directory");
    let fake = profile.join("examples").join(exe("fake_cargo"));
    if !fake.is_file() {
        static BUILT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        BUILT.get_or_init(|| build_the_example(profile));
    }
    assert!(
        fake.is_file(),
        "the fake cargo is not built at {}: run `cargo build --examples -p rust-mutants` first, \
         or drive this suite with a task that does",
        fake.display()
    );
    fake
}

/// Builds the example into the target directory the test binary itself lives in.
fn build_the_example(profile: &Path) {
    let target = profile.parent().unwrap_or(profile);
    let profile_name = profile.file_name().unwrap_or_default();
    let cargo_profile = if profile_name == "debug" {
        OsString::from("dev")
    } else {
        profile_name.to_owned()
    };
    let manifest = crate::paths::workspace_root().join("Cargo.toml");
    let said = std::process::Command::new(crate::paths::cargo_binary())
        .args(["build", "--offline", "--examples", "-p", "rust-mutants"])
        .arg("--profile")
        .arg(cargo_profile)
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(target)
        .output();
    if let Ok(output) = said
        && !output.status.success()
    {
        eprintln!(
            "mjutest-devkit: building the fake cargo failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Writes `script` and puts the fake at every program name it answers to, under a directory of this test's own.
///
/// Keep the [`Installed`] alive for as long as the run it scripts: dropping it
/// removes the programs, and the next command finds no cargo.
///
/// Placeholders expanded in `stdout`, `stderr`, and every write: `{{bin}}` for
/// the program directory, `{{script_dir}}` for the directory the script is in,
/// and, at run time, `{{cwd}}`, `{{target_dir}}`, `{{pid}}`, and `{{now_ms}}`.
///
/// # Panics
/// When the script cannot be written or the fake cannot be placed.
#[must_use]
pub fn install(script: &Script) -> Installed {
    let dir = tempfile::Builder::new()
        .prefix("mjutest-fake-cargo-")
        .tempdir()
        .expect("a temporary directory");
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).expect("the program directory");
    let fake = locate();
    let mut programs: Vec<String> = script
        .invocations
        .iter()
        .map(|invocation| invocation.program.clone())
        .collect();
    programs.push("cargo".to_owned());
    programs.push("rustc".to_owned());
    programs.sort();
    programs.dedup();
    for program in &programs {
        place(&fake, &bin.join(exe(program)));
    }
    let path = dir.path().join("script.json");
    let rendered = serde_json::to_string_pretty(script)
        .expect("the script renders")
        .replace("{{bin}}", &bin.to_string_lossy())
        .replace("{{script_dir}}", &dir.path().to_string_lossy());
    std::fs::write(&path, rendered).expect("the script");
    Installed {
        bin,
        script: path,
        log: dir.path().join("script.json.answered"),
        _dir: dir,
    }
}

#[cfg(unix)]
fn place(fake: &Path, at: &Path) {
    std::os::unix::fs::symlink(fake, at).expect("the fake in place");
}

#[cfg(not(unix))]
fn place(fake: &Path, at: &Path) {
    let _copied = std::fs::copy(fake, at).expect("the fake in place");
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}
