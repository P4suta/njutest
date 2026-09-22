// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where things are: the workspace root, the fixture projects, the cargo that built the test binary.

use std::fs;
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
    let dir = root.parent().unwrap_or(root).join(name);
    fs::create_dir_all(&dir)?;
    Ok(dir)
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
    vars.into_iter()
        .filter(|(name, _value)| {
            let reserved = composed
                .iter()
                .any(|reserved| same_name(name, std::ffi::OsStr::new(reserved)));
            !(reserved || (outer_coverage && cargo_llvm_cov_owns(name)))
        })
        .collect()
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
    command
}

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
