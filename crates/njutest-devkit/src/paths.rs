// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where things are: the workspace root, the fixture projects, the cargo that built the test binary.

use std::collections::BTreeSet;
use std::fs;
use std::io as std_io;
use std::path::{Path, PathBuf};

/// The root of this workspace, resolved from this crate's manifest directory at compile time, so it does not depend on the working directory of the test process.
#[must_use]
pub fn workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map_or_else(|| manifest_dir.to_path_buf(), Path::to_path_buf)
}

/// The directory holding the independent fixture projects.
#[must_use]
pub fn fixtures_dir() -> PathBuf {
    workspace_root().join("fixtures")
}

/// The `cargo` a test drives a fixture with.
#[must_use]
pub fn cargo_binary() -> PathBuf {
    std::env::var_os("CARGO").map_or_else(|| PathBuf::from("cargo"), PathBuf::from)
}

/// The POSIX `sh` a test that writes its child as a shell script runs, found on the search path, or on Windows beside the Git that is.
///
/// A missing shell is a missing precondition of the test, never its subject failing, so it is refused here in words that say what to install rather than read later as an empty output.
///
/// # Panics
/// No `sh` is on the search path, and on Windows none is where Git for Windows keeps one beside the `git` that is.
#[must_use]
#[track_caller]
#[expect(
    clippy::panic,
    reason = "a test without its shell cannot say anything about its subject, and saying so is its only honest answer"
)]
pub fn posix_sh() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let directories: Vec<PathBuf> = std::env::split_paths(&path).collect();
    match find_posix_sh(&directories, |candidate| fs::metadata(candidate)) {
        Ok(found) => found,
        Err(skipped) => panic!("{}", missing_shell(&skipped)),
    }
}

/// One search-path entry that could not be inspected, with the candidate that proved it.
#[derive(Debug)]
struct SkippedPath {
    entry: PathBuf,
    candidate: PathBuf,
    source: std_io::Error,
}

/// Finds a shell among `directories`, remembering an entry whose candidates cannot be inspected and continuing after it.
fn find_posix_sh(
    directories: &[PathBuf],
    inspect: impl FnMut(&Path) -> std_io::Result<fs::Metadata>,
) -> Result<PathBuf, Vec<SkippedPath>> {
    let mut search = Search {
        inspect,
        uninspectable: BTreeSet::new(),
        skipped: Vec::new(),
    };
    let named = ["sh", "sh.exe"];
    for directory in directories {
        for name in named {
            let candidate = directory.join(name);
            if search.is_file(directory, &candidate) {
                return Ok(candidate);
            }
        }
    }
    for directory in directories {
        if !search.is_file(directory, &directory.join("git.exe")) {
            continue;
        }
        let Some(git) = directory.parent() else {
            continue;
        };
        for candidate in [
            git.join("usr").join("bin").join("sh.exe"),
            git.join("bin").join("sh.exe"),
        ] {
            if search.is_file(directory, &candidate) {
                return Ok(candidate);
            }
        }
    }
    Err(search.skipped)
}

/// One search that stops asking about an entry after the first candidate the operating system refuses to inspect.
struct Search<F> {
    inspect: F,
    uninspectable: BTreeSet<PathBuf>,
    skipped: Vec<SkippedPath>,
}

impl<F: FnMut(&Path) -> std_io::Result<fs::Metadata>> Search<F> {
    /// Whether `candidate` is a file, recording and skipping `entry` when that cannot be answered.
    fn is_file(&mut self, entry: &Path, candidate: &Path) -> bool {
        if self.uninspectable.contains(entry) {
            return false;
        }
        match (self.inspect)(candidate) {
            Ok(metadata) => metadata.is_file(),
            Err(error)
                if matches!(
                    error.kind(),
                    std_io::ErrorKind::NotFound | std_io::ErrorKind::NotADirectory
                ) =>
            {
                false
            }
            Err(source) => {
                self.uninspectable.insert(entry.to_path_buf());
                self.skipped.push(SkippedPath {
                    entry: entry.to_path_buf(),
                    candidate: candidate.to_path_buf(),
                    source,
                });
                false
            }
        }
    }
}

/// The missing-shell diagnostic, including every search-path entry that could not be inspected.
fn missing_shell(skipped: &[SkippedPath]) -> String {
    let mut message = String::from(
        "this test writes its child as a POSIX shell script and needs `sh`: none is on PATH, and \
         none is beside a Git for Windows on it; on Windows put Git's usr\\bin on PATH",
    );
    if !skipped.is_empty() {
        message.push_str("; these PATH entries could not be inspected");
        for one in skipped {
            message.push_str("; ");
            message.push_str(&one.entry.display().to_string());
            message.push_str(" while checking ");
            message.push_str(&one.candidate.display().to_string());
            message.push_str(": ");
            message.push_str(&one.source.to_string());
        }
    }
    message
}

#[cfg(test)]
mod shell_tests {
    use std::fs;
    use std::io;

    use super::{find_posix_sh, missing_shell};

    #[test]
    fn an_uninspectable_search_path_entry_is_skipped_for_a_later_shell() -> io::Result<()> {
        let scratch = tempfile::tempdir()?;
        let uninspectable = scratch.path().join("current");
        let usable = scratch.path().join("usable");
        fs::create_dir_all(usable.join("sh"))?;
        fs::write(usable.join("sh.exe"), "")?;
        let directories = [uninspectable.clone(), usable.clone()];
        let found = find_posix_sh(&directories, |candidate| {
            if candidate.starts_with(&uninspectable) {
                Err(io::Error::from_raw_os_error(448))
            } else {
                fs::metadata(candidate)
            }
        })
        .map_err(|skipped| io::Error::other(missing_shell(&skipped)))?;

        let expected = usable.join("sh.exe");
        if found == expected {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "the refused entry was not passed over to {expected:?}, or a directory named `sh` was returned as the executable: {found:?}"
            )))
        }
    }

    #[test]
    fn a_missing_shell_names_an_entry_that_could_not_be_inspected() -> io::Result<()> {
        let scratch = tempfile::tempdir()?;
        let uninspectable = scratch.path().join("current");
        let result = find_posix_sh(std::slice::from_ref(&uninspectable), |_candidate| {
            Err(io::Error::from_raw_os_error(448))
        });
        let skipped = match result {
            Ok(found) => {
                return Err(io::Error::other(format!(
                    "an injected inspection failure found a shell at {}",
                    found.display()
                )));
            }
            Err(skipped) => skipped,
        };
        let message = missing_shell(&skipped);

        if message.contains(&uninspectable.display().to_string())
            && message.contains("os error 448")
        {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "the final refusal did not name both the entry and why it was skipped: {message}"
            )))
        }
    }
}

/// Borrows the exact UTF-8 spelling of a path used by a textual test protocol.
///
/// # Panics
/// The path is not UTF-8.
/// A test that passed replacement characters to the subject would exercise a different path and could not support its claim.
#[must_use]
#[track_caller]
#[expect(
    clippy::panic,
    reason = "non-UTF-8 fixture paths are protocol failures at this test-only boundary"
)]
pub fn utf8(path: &Path) -> &str {
    match path.to_str() {
        Some(text) => text,
        None => panic!(
            "test protocol path is not UTF-8; encoded bytes: {}",
            hex::encode(path.as_os_str().as_encoded_bytes())
        ),
    }
}

/// Owns the exact UTF-8 spelling of a filesystem name used by a textual test protocol.
///
/// # Panics
/// The name is not UTF-8.
/// Replacing bytes would let two filesystem entries become one test-oracle value.
#[must_use]
#[track_caller]
#[expect(
    clippy::panic,
    reason = "non-UTF-8 fixture names are protocol failures at this test-only boundary"
)]
pub fn owned_utf8(name: std::ffi::OsString) -> String {
    match name.into_string() {
        Ok(text) => text,
        Err(name) => panic!(
            "test protocol name is not UTF-8; encoded bytes: {}",
            hex::encode(name.as_encoded_bytes())
        ),
    }
}

/// A directory beside `root` for what a run puts in the temporary directory.
///
/// # Errors
/// The directory could not be created, which means the test has no place to work.
pub fn temp_beside(root: &Path) -> std::io::Result<PathBuf> {
    beside(root, "njutest-devkit-temp")
}

/// A directory beside `root` for the caches a run keeps between runs.
///
/// # Errors
/// The directory could not be created, which means the test has no place to work.
pub fn cache_beside(root: &Path) -> std::io::Result<PathBuf> {
    beside(root, "njutest-devkit-cache")
}

fn beside(root: &Path, name: &str) -> std::io::Result<PathBuf> {
    let parent = root.parent().unwrap_or(root);
    if same_directory(parent, &std::env::temp_dir()) {
        return Err(std::io::Error::other(format!(
            "{} sits directly in the shared temporary directory, so what is put beside it would \
             be shared by every test and outlive them all; give the tree a directory of its own \
             and root it inside that",
            root.display()
        )));
    }
    let dir = parent.join(name);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Every variable a platform's standard library or a POSIX tool reads its temporary directory from.
pub const TEMPORARY_VARIABLES: [&str; 3] = ["TMPDIR", "TMP", "TEMP"];

/// `dir` as the temporary directory under each of [`TEMPORARY_VARIABLES`], for [`std::process::Command::envs`].
#[must_use]
pub fn temporary_directory(dir: &Path) -> [(&'static str, &Path); 3] {
    TEMPORARY_VARIABLES.map(|name| (name, dir))
}

/// A project tree a test owns, rooted inside a temporary directory of its own so what a run puts beside the tree goes with it.
#[derive(Debug)]
pub struct Project {
    #[expect(
        dead_code,
        reason = "the directory is held for what dropping it does: the project and what sits beside it go"
    )]
    directory: tempfile::TempDir,
    root: PathBuf,
}

impl Project {
    /// A new, empty project tree.
    ///
    /// # Panics
    /// When no temporary directory can be made, which leaves the test nowhere to work.
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "a test with no directory has nothing to test"
    )]
    pub fn fresh() -> Self {
        let directory = tempfile::tempdir().expect("a directory for a project");
        let root = directory.path().join("project");
        fs::create_dir_all(&root).expect("a project tree");
        Self { directory, root }
    }

    /// The project's root.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.root
    }
}

/// Whether two paths name one directory once each is resolved.
fn same_directory(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        (Err(_), _) | (_, Err(_)) => left == right,
    }
}

/// `path`, escaped the way a JSON string escapes its contents, without the quotes.
///
/// # Panics
/// A test fixture path is not UTF-8.
/// Fixture paths enter textual Cargo and JSON protocols, so accepting a lossy spelling would test a different path.
#[must_use]
pub fn in_json(path: &Path) -> String {
    text_in_json(utf8(path))
}

/// `text`, escaped the way a JSON string escapes its contents, without the quotes.
///
/// A package identity spells a path inside itself, and on Windows that path holds backslashes a JSON string reads as escapes, so a document built by pasting one in is not the document cargo prints.
///
/// # Panics
/// Never: JSON string serialization of a `str` cannot fail.
#[must_use]
#[expect(
    clippy::expect_used,
    reason = "JSON string serialization of a str cannot fail, and a test that lost the spelling would assert about a different package"
)]
pub fn text_in_json(text: &str) -> String {
    let quoted = serde_json::to_string(text).expect("a string serializes to JSON");
    quoted
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .expect("a serialized JSON string has quotes")
        .to_owned()
}

/// How many toolchain tests nextest runs at once, whoever starts it; `.config/nextest.toml` holds the same number.
pub const TOOLCHAIN_TESTS_AT_ONCE: usize = 4;

/// The jobs a cargo started by a test may use: this machine's share for one of [`TOOLCHAIN_TESTS_AT_ONCE`] tests, never fewer than one.
#[must_use]
pub fn nested_build_jobs() -> usize {
    let cores = match std::thread::available_parallelism() {
        Ok(cores) => cores.get(),
        Err(_unknown) => 1,
    };
    cores
        .checked_div(TOOLCHAIN_TESTS_AT_ONCE)
        .unwrap_or(1)
        .max(1)
}

/// The parent's environment for a new in-process run, with the variables that run must compose for itself taken out.
#[must_use]
pub fn environment_for_a_run() -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let composed: [&str; 7] = [
        "RUST_MUTANTS_ACTIVE",
        "RUST_MUTANTS_CATALOG",
        "RUST_MUTANTS_TOUCH",
        "LLVM_PROFILE_FILE",
        "RUSTFLAGS",
        "RUSTDOCFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
    ];
    let vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os().collect();
    let outer_coverage = vars
        .iter()
        .any(|(name, _value)| same_name(name, std::ffi::OsStr::new("CARGO_LLVM_COV")));
    let mut kept: Vec<(std::ffi::OsString, std::ffi::OsString)> = vars
        .into_iter()
        .filter(|(name, _value)| {
            let reserved = composed
                .iter()
                .chain(std::iter::once(&JOBS))
                .any(|reserved| same_name(name, std::ffi::OsStr::new(reserved)));
            !(reserved || (outer_coverage && cargo_llvm_cov_owns(name)))
        })
        .collect();
    kept.push(jobs());
    kept
}

/// The variable a nested cargo reads its job count from.
const JOBS: &str = "CARGO_BUILD_JOBS";

/// This machine's share of cores for one nested cargo, as the variable that says it.
fn jobs() -> (std::ffi::OsString, std::ffi::OsString) {
    (
        std::ffi::OsString::from(JOBS),
        std::ffi::OsString::from(nested_build_jobs().to_string()),
    )
}

/// Whether two environment variable names are one name on this platform.
#[must_use]
pub fn same_name(one: &std::ffi::OsStr, other: &std::ffi::OsStr) -> bool {
    if cfg!(windows) {
        one.eq_ignore_ascii_case(other)
    } else {
        one == other
    }
}

/// The names a nested toolchain run needs from the parent beyond the four every suite asks for.
#[cfg(windows)]
pub const ALSO_ON_THIS_PLATFORM: [&str; 13] = [
    "SystemRoot",
    "SystemDrive",
    "windir",
    "ComSpec",
    "PATHEXT",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "LOCALAPPDATA",
    "APPDATA",
    "ProgramData",
    "PROCESSOR_ARCHITECTURE",
    "NUMBER_OF_PROCESSORS",
];

/// The names a nested toolchain run needs from the parent beyond the four every suite asks for.
#[cfg(not(windows))]
pub const ALSO_ON_THIS_PLATFORM: [&str; 0] = [];

/// The least of the parent's environment a nested run of either product needs, and the names `also` adds.
#[must_use]
pub fn environment_for_a_toolchain_run(
    also: &[&str],
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let wanted: [&str; 4] = ["PATH", "HOME", "RUSTUP_HOME", "CARGO_HOME"];
    let mut kept: Vec<(std::ffi::OsString, std::ffi::OsString)> = environment_for_a_run()
        .into_iter()
        .filter(|(name, _value)| {
            wanted
                .iter()
                .chain(ALSO_ON_THIS_PLATFORM.iter())
                .chain(also.iter())
                .any(|wanted| same_name(name, std::ffi::OsStr::new(wanted)))
        })
        .collect();
    if let Some(cache) = compilation_cache() {
        kept.push((std::ffi::OsString::from(WRAPPER), cache));
    }
    kept.push(jobs());
    kept
}

/// The variable a compiler wrapper is named in, which two different things use.
const WRAPPER: &str = "RUSTC_WRAPPER";

/// What names a compilation cache, rather than anything else a wrapper can be.
const CACHE: &str = "sccache";

/// The parent's compiler wrapper, when it is a compilation cache and nothing else.
///
/// A nested run builds a fixture from scratch, three hundred times over thirty-eight fixtures, each under its own target directory because sharing one would let a test pick up another's instrumented artifact (ADR 0019).
/// The isolation is the point and it stays; what it costs is recompiling identical units, and a cache keyed on content removes that without touching it.
///
/// Read by value rather than forwarded, because this variable is where `cargo-llvm-cov` puts a shim that instruments whatever it wraps — and a coverage run of this suite that let that reach a fixture would be measuring its own instrumentation.
/// Two different things under one name (ADR 0023); only one of them is wanted here.
fn compilation_cache() -> Option<std::ffi::OsString> {
    environment_for_a_run()
        .into_iter()
        .find(|(name, _value)| same_name(name, std::ffi::OsStr::new(WRAPPER)))
        .map(|(_name, value)| value)
        .filter(|value| names_a_cache(value))
}

/// Whether a compiler wrapper is the compilation cache rather than something else wearing the variable.
#[must_use]
pub fn names_a_cache(wrapper: &std::ffi::OsStr) -> bool {
    Path::new(wrapper)
        .file_stem()
        .is_some_and(|stem| stem.eq_ignore_ascii_case(CACHE))
}

fn cargo_llvm_cov_owns(name: &std::ffi::OsStr) -> bool {
    same_name(name, std::ffi::OsStr::new("RUSTC_WRAPPER"))
        || name.to_str().is_some_and(|name| {
            name == "CARGO_LLVM_COV"
                || name.starts_with("CARGO_LLVM_COV_")
                || name.starts_with("__CARGO_LLVM_COV_")
        })
}

/// A command for `program`, preserving mutation identity while isolating coverage output.
///
/// The compiler flags go with it: a run that inherits them refuses to reach into the code it is measuring, and reports the mutants it could not probe as refused rather than killed.
/// That turns a suite into a report about whoever set the variable — `RUSTFLAGS: -D warnings` in CI is enough — so a test that drives the engine states the environment it wants instead of inheriting one.
#[must_use]
pub fn command(program: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    for inherited in NOT_INHERITED {
        remove_environment(&mut command, inherited);
    }
    discard_profile(&mut command);
    command
}

#[expect(
    unused_results,
    reason = "Command's infallible builder API returns self; this unit helper is the explicit boundary"
)]
fn discard_profile(command: &mut std::process::Command) {
    command.env("LLVM_PROFILE_FILE", NULL_DEVICE);
}

/// The file this platform discards everything written to.
pub const NULL_DEVICE: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

/// What a fixture run is insulated from, spelled here because this crate depends on nothing.
///
/// `crates/rust-mutants/tests/devkit_environment.rs` holds these against the engine's own constants, which is the only place that can see both.
pub const NOT_INHERITED: [&str; 3] = ["LLVM_PROFILE_FILE", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"];

#[expect(
    unused_results,
    reason = "Command's infallible builder API returns self; this unit helper is the explicit boundary"
)]
fn remove_environment(command: &mut std::process::Command, name: &str) {
    command.env_remove(name);
}
