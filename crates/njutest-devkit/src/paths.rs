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

/// A directory beside `root` for what a run puts in the temporary directory.
///
/// Snapshots, build caches, and worker scratch all land there. Pointing
/// `TMPDIR` at it is how a test bounds its own mess: what the run leaves
/// behind goes away with the tree the test owns, rather than accumulating in
/// the machine's shared temporary directory.
///
/// # Errors
/// The directory could not be created, which means the test has no place to work.
pub fn temp_beside(root: &Path) -> std::io::Result<PathBuf> {
    beside(root, "njutest-devkit-temp")
}

/// A directory beside `root` for the caches a run keeps between runs.
///
/// It has to be outside the tree: a cache inside the tree under verification
/// would change the tree's own digest every time a run wrote to it, and no
/// second run of the same work would ever look like one.
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
/// A suite that writes the document a real `cargo metadata` would print has to
/// write it the way a real one does. A Windows path separator is the escape
/// character inside a JSON string, so a path written plainly between quotes
/// makes a document that does not parse:
///
/// ```text
/// cargo metadata did not print its document: invalid escape at line 1 column 86
/// ```
///
/// The quotes are left to the caller because a path is not always the whole of
/// a string: a package identity holds one after `path+file://`, and a document
/// that quoted it there would say something else entirely.
#[must_use]
pub fn in_json(path: &Path) -> String {
    let quoted = serde_json::to_string(&path.to_string_lossy()).unwrap_or_default();
    quoted
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_default()
        .to_owned()
}

/// The parent's environment for a new in-process run, with the variables that run must compose for itself taken out.
///
/// A nested run must not inherit `RUST_MUTANTS_ACTIVE`,
/// `RUST_MUTANTS_CATALOG`, or `RUST_MUTANTS_TOUCH`, because an inherited one
/// would decide what somebody else's run measured. A subprocess of the code
/// under test is different and uses [`command`] to retain the outer activation
/// or touch mode beside its catalog.
///
/// So a test composes the environment it gives a run rather than passing its
/// own along. `LLVM_PROFILE_FILE` goes for the same reason from the other end:
/// a measurement sets it, and a child that inherited it writes over the
/// measurement that started it. When the suite itself is under
/// `cargo-llvm-cov`, that harness's private Cargo variables and wrapper go as
/// well. They describe the outer workspace and would otherwise override the
/// inner run's isolated target directory. An ordinary caller's
/// `RUSTC_WRAPPER` is retained.
///
/// The compiler flags go as well. `RUSTFLAGS`, `RUSTDOCFLAGS`, and
/// `CARGO_ENCODED_RUSTFLAGS` say how this workspace is compiled — under
/// continuous integration, with warnings denied — and a fixture built under
/// them is built with a posture nobody wrote it for. An ordinary warning in a
/// sample project becomes the error that stops the run, and a suite that only
/// failed on the machines setting them is a suite about the machine rather
/// than about the product.
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
///
/// The engine answers the same question in `rust_mutants::vars`, and the
/// dependency direction `cargo xtask deps` holds has this crate below every
/// other rather than above the engine, so the rule is stated again here for
/// the suites rather than borrowed.
#[must_use]
pub fn same_name(one: &std::ffi::OsStr, other: &std::ffi::OsStr) -> bool {
    if cfg!(windows) {
        one.eq_ignore_ascii_case(other)
    } else {
        one == other
    }
}

/// The names a nested toolchain run needs from the parent beyond the four every suite asks for.
///
/// A Windows process that is handed an environment without `SystemRoot` does
/// not start: the loader reads it to find the system libraries every program
/// links. The rest are what cargo and rustc look up on that platform for the
/// same reasons `HOME` and `TMPDIR` serve on a unix one, and a child given
/// none of them fails in ways that read as anything but a missing variable.
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
///
/// A suite that drives a real toolchain wants to prove the product works with
/// what a machine actually has to provide and nothing else, so this is an
/// allowed list rather than a refused one. Which names those are is not the
/// same question on every platform, and a suite that spelled one list would be
/// asking a unix question on a Windows machine: the answer there is a run that
/// cannot find cargo, reported as `RM1012`.
#[must_use]
pub fn environment_for_a_toolchain_run(
    also: &[&str],
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let wanted: [&str; 4] = ["PATH", "HOME", "RUSTUP_HOME", "CARGO_HOME"];
    environment_for_a_run()
        .into_iter()
        .filter(|(name, _value)| {
            wanted
                .iter()
                .chain(ALSO_ON_THIS_PLATFORM.iter())
                .chain(also.iter())
                .any(|wanted| same_name(name, std::ffi::OsStr::new(wanted)))
        })
        .collect()
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
/// A subprocess built from an instrumented tree must inherit the outer
/// activation or touch mode and its catalog so its guards still measure it.
/// Its composition root verifies the catalog embedded by Cargo before
/// accepting that pair.
/// Coverage output is never inherited because the child would overwrite the
/// measurement that started it.
#[must_use]
pub fn command(program: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(program);
    let _configured = command.env_remove("LLVM_PROFILE_FILE");
    command
}
