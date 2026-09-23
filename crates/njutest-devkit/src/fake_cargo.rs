// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A cargo, a rustc, a coverage tool, or a test binary that says what a script told it to say.

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
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Files it writes after the delay and before it answers, so their absence is the process having been stopped rather than a clock a test read.
    #[serde(default)]
    pub writes_after: Vec<WriteFile>,
    /// How many times it may answer.
    /// `None` is every time.
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
            writes_after: Vec::new(),
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

    /// Writes `contents` at `path` once the delay has passed, so a test reads whether the command was allowed to finish rather than how long it waited.
    #[must_use]
    pub fn writing_after(mut self, path: &str, contents: &str) -> Self {
        self.writes_after.push(WriteFile {
            path: path.to_owned(),
            contents: contents.to_owned(),
        });
        self
    }
}

/// A file the fake writes before it answers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
            .expect("the fake cargo answer log")
            .lines()
            .map(|line| {
                line.trim()
                    .parse::<usize>()
                    .expect("the fake writes numeric script indexes")
            })
            .collect()
    }
}

/// The fake, as `cargo build --examples` leaves it beside the test binaries.
///
/// # Panics
/// When the example is not there and cannot be built, with the command that builds it.
#[must_use]
pub fn locate() -> PathBuf {
    example("fake_cargo")
}

/// The example named `name`, as `cargo build --examples` leaves it beside the test binaries.
///
/// # Panics
/// When the example is not there and cannot be built, with the command that builds it.
#[must_use]
pub fn example(name: &str) -> PathBuf {
    let current = std::env::current_exe().expect("the test binary's own path");
    let deps = current.parent().expect("the deps directory");
    let profile = deps.parent().expect("the profile directory");
    let fake = profile.join("examples").join(exe(name));
    if !regular_file(&fake).expect("reading the fake cargo example's metadata") {
        static BUILT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        BUILT.get_or_init(|| build_the_example(profile));
    }
    assert!(
        regular_file(&fake).expect("reading the built fake cargo example's metadata"),
        "{name} is not built at {}: run `cargo build --examples -p rust-mutants` first, \
         or drive this suite with a task that does",
        fake.display()
    );
    fake
}

/// Builds the example into the target directory the test binary itself lives in.
fn build_the_example(profile: &Path) {
    let target = profile.parent().expect("the target directory");
    let profile_name = profile.file_name().expect("the cargo profile name");
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
    match said {
        Ok(output) if !output.status.success() => {
            let stderr = String::from_utf8(output.stderr)
                .expect("cargo writes UTF-8 diagnostics for its own failed build");
            eprintln!("njutest-devkit: building the fake cargo failed:\n{stderr}");
        }
        Ok(_) => {}
        Err(error) => {
            eprintln!("njutest-devkit: starting the fake cargo build failed: {error}");
        }
    }
}

/// Writes `script` and puts the fake at every program name it answers to, under a directory of this test's own.
///
/// # Panics
/// When the script cannot be written or the fake cannot be placed.
#[must_use]
pub fn install(script: &Script) -> Installed {
    let dir = tempfile::Builder::new()
        .prefix("njutest-fake-cargo-")
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
        .replace("{{bin_json}}", &nested(&bin))
        .replace("{{script_dir_json}}", &nested(dir.path()))
        .replace("{{bin}}", &crate::paths::in_json(&bin))
        .replace("{{script_dir}}", &crate::paths::in_json(dir.path()));
    std::fs::write(&path, rendered).expect("the script");
    Installed {
        bin,
        script: path,
        log: dir.path().join("script.json.answered"),
        _dir: dir,
    }
}

/// `path` escaped for a JSON string that is itself inside a JSON string.
fn nested(path: &Path) -> String {
    crate::paths::in_json(Path::new(&crate::paths::in_json(path)))
}

#[cfg(unix)]
fn place(fake: &Path, at: &Path) {
    std::os::unix::fs::symlink(fake, at).expect("the fake in place");
}

#[cfg(not(unix))]
fn place(fake: &Path, at: &Path) {
    let copied = std::fs::copy(fake, at).expect("the fake in place");
    let expected = std::fs::metadata(fake).expect("the fake's metadata").len();
    assert_eq!(copied, expected, "the complete fake was copied into place");
}

fn regular_file(path: &Path) -> std::io::Result<bool> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}
