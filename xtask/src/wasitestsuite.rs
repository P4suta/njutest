// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! WebAssembly/wasi-testsuite's preview1 tests on the sealed host: the commit the expectations pin, fetched once into a cache and verified by its id every time, and the engine's harness, which runs every test as a sealed test instance runs and holds it to the result the expectations name.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

/// The expectations, from the workspace root: the repository and commit the suite is fetched from, and what the sealed host must give for every one of its preview1 tests.
pub const EXPECTATIONS: &str = "crates/rust-mutants/tests/wasi-testsuite.toml";

/// The package whose test runs the suite.
pub const HARNESS_PACKAGE: &str = "rust-mutants";

/// The test binary of the package that holds the harness, as one of its modules.
pub const HARNESS_TARGET: &str = "suite";

/// The test that runs every preview1 test, as libtest names it: the harness's module, `tests/<module>.rs`, and the test, ignored wherever no checkout is named to it.
pub const HARNESS_TEST: &str = "wasi_testsuite::every_preview1_test_ends_as_the_expectations_say";

/// The variable the harness is told the verified checkout in.
pub const SUITE_VARIABLE: &str = "NJUTEST_WASI_TESTSUITE";

/// Where the suite is fetched to when no cache is named, from the workspace root.
pub const DEFAULT_CACHE: &str = "target/wasi-testsuite";

/// Why the suite could not be run, or did not end as the expectations say.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SuiteError {
    /// The expectations could not be read, or do not name a repository, a commit and a result for each test.
    #[error("{}: {said}", path.display())]
    Expectations {
        /// The expectations.
        path: PathBuf,
        /// What is wrong with them.
        said: String,
    },
    /// The pinned commit could not be fetched into the cache.
    #[error("{commit} of {repository} could not be fetched into {}: {said}", cache.display())]
    Fetch {
        /// The repository.
        repository: String,
        /// The commit.
        commit: String,
        /// The cache it was fetched into.
        cache: PathBuf,
        /// What failed.
        said: String,
    },
    /// The cached checkout is not the pinned commit, whole and unchanged.
    #[error("{} is not {commit} as it was fetched: {said}", checkout.display())]
    Checkout {
        /// The checkout.
        checkout: PathBuf,
        /// The commit it should be.
        commit: String,
        /// How it differs.
        said: String,
    },
    /// cargo could not be started for the harness.
    #[error("cargo test could not be started for the harness: {source}")]
    Unrun {
        /// Why.
        source: std::io::Error,
    },
    /// The harness did not run the test that runs the suite, so it established nothing.
    #[error("cargo test ended {status} and ran no test named {HARNESS_TEST}:\n{said}")]
    Unran {
        /// How cargo ended.
        status: ExitStatus,
        /// What cargo and the harness printed.
        said: String,
    },
    /// A test ended otherwise than the expectations say, or the suite and the expectations name different tests.
    #[error("the harness ended {status}:\n{said}")]
    Departed {
        /// How cargo ended.
        status: ExitStatus,
        /// What cargo and the harness printed, which names every departure.
        said: String,
    },
}

impl crate::error::Coded for SuiteError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::Expectations { .. } => crate::error::XtCode::SuiteExpectations,
            Self::Fetch { .. } => crate::error::XtCode::SuiteFetch,
            Self::Checkout { .. } => crate::error::XtCode::SuiteCheckout,
            Self::Unrun { .. } | Self::Unran { .. } => crate::error::XtCode::SuiteUnrun,
            Self::Departed { .. } => crate::error::XtCode::SuiteDeparted,
        }
    }
}

/// The suite the expectations pin, and how many of its tests they say pass and how many fail on a refusal of the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    /// The repository the suite is fetched from.
    pub repository: String,
    /// The commit of it.
    pub commit: String,
    /// How many tests the expectations say pass.
    pub passing: usize,
    /// How many tests the expectations say fail on a refusal of the host.
    pub refused: usize,
}

/// The pin the expectations `text` write.
///
/// # Errors
/// [`SuiteError::Expectations`] where the text is not TOML, names no repository or commit, or gives a test no result the harness reads.
pub fn pin(text: &str) -> Result<Pin, SuiteError> {
    let refuse = |said: String| SuiteError::Expectations {
        path: PathBuf::from(EXPECTATIONS),
        said,
    };
    let table: toml::Table = toml::from_str(text).map_err(|error| refuse(error.to_string()))?;
    let named = |key: &str| {
        table
            .get(key)
            .and_then(toml::Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| refuse(format!("no `{key}` names the suite")))
    };
    let (repository, commit) = (named("repository")?, named("commit")?);
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(refuse(format!(
            "the suite is pinned by a full commit id, and {commit:?} is not one"
        )));
    }
    let tests = table
        .get("test")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| refuse("no `[[test]]` names what the host must give".to_owned()))?;
    let (mut passing, mut refused) = (0_usize, 0_usize);
    for test in tests {
        let counted = match test.get("result").and_then(toml::Value::as_str) {
            Some("pass") => &mut passing,
            Some("refused") => &mut refused,
            Some(_) | None => {
                return Err(refuse(format!(
                    "a test's result is neither `pass` nor `refused`: {test}"
                )));
            }
        };
        *counted = counted
            .checked_add(1)
            .ok_or_else(|| refuse("more tests than can be counted".to_owned()))?;
    }
    Ok(Pin {
        repository,
        commit,
        passing,
        refused,
    })
}

/// What `git` printed on standard output when it ran `arguments` in `dir` with `options` before them, or the failure `failed` makes of what went wrong.
fn git<F>(
    dir: &Path,
    (options, arguments): (&[String], &[&str]),
    failed: F,
) -> Result<String, SuiteError>
where
    F: Fn(String) -> SuiteError,
{
    let asked = arguments.join(" ");
    let mut command = crate::repository::git(dir);
    for option in options {
        command.arg("-c").arg(option);
    }
    let output = command
        .args(arguments)
        .output()
        .map_err(|error| failed(format!("git {asked}: {error}")))?;
    let text = |bytes: Vec<u8>| {
        String::from_utf8(bytes)
            .map_err(|_not_text| failed(format!("git {asked} printed bytes that are not text")))
    };
    if output.status.success() {
        text(output.stdout)
    } else {
        let said = text(output.stderr)?;
        Err(failed(format!(
            "git {asked} ended {}: {}",
            output.status,
            said.trim()
        )))
    }
}

/// Whether the checkout at `checkout` is `commit`, whole and unchanged: its `HEAD` is the commit, and nothing in its tree differs from it, is added to it, or is ignored beside it.
///
/// # Errors
/// [`SuiteError::Checkout`] naming how it differs.
pub fn verify(checkout: &Path, commit: &str) -> Result<(), SuiteError> {
    let differs = |said: String| SuiteError::Checkout {
        checkout: checkout.to_path_buf(),
        commit: commit.to_owned(),
        said,
    };
    let head = git(
        checkout,
        (&[], &["rev-parse", "--verify", "HEAD^0"]),
        differs,
    )?;
    if head.trim() != commit {
        return Err(differs(format!("its HEAD is {}", head.trim())));
    }
    let changed = git(
        checkout,
        (
            &[],
            &[
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
                "--ignored",
            ],
        ),
        differs,
    )?;
    if changed.is_empty() {
        Ok(())
    } else {
        Err(differs(format!(
            "its tree differs from the commit; remove it, and the next run fetches it afresh:\n{}",
            changed.trim_end()
        )))
    }
}

/// Fetches `pin` into a directory of `cache` of its own, verifies it, and moves it to `kept`, so a fetch that stopped halfway never looks like a checkout.
fn fetch(cache: &Path, kept: &Path, pin: &Pin) -> Result<(), SuiteError> {
    let failed = |said: String| SuiteError::Fetch {
        repository: pin.repository.clone(),
        commit: pin.commit.clone(),
        cache: cache.to_path_buf(),
        said,
    };
    std::fs::create_dir_all(cache).map_err(|error| failed(error.to_string()))?;
    let staging = tempfile::Builder::new()
        .prefix(".fetching-")
        .tempdir_in(cache)
        .map_err(|error| failed(error.to_string()))?;
    let options = [
        "fetch.fsckObjects=true".to_owned(),
        "advice.detachedHead=false".to_owned(),
    ];
    let (repository, commit) = (pin.repository.as_str(), pin.commit.as_str());
    for arguments in [
        &["init", "--quiet"][..],
        &[
            "fetch",
            "--quiet",
            "--depth",
            "1",
            "--no-tags",
            repository,
            commit,
        ][..],
        &["checkout", "--quiet", "--detach", commit][..],
    ] {
        git(staging.path(), (&options, arguments), failed)?;
    }
    verify(staging.path(), commit)?;
    std::fs::rename(staging.path(), kept).map_err(|error| {
        failed(format!(
            "{} could not become {}: {error}",
            staging.path().display(),
            kept.display()
        ))
    })
}

/// The checkout of `pin` in `cache`, fetched there the first time it is asked for, and verified by its commit id every time.
///
/// # Errors
/// [`SuiteError::Fetch`] where it could not be fetched, and [`SuiteError::Checkout`] where the checkout is not the commit, whole and unchanged.
pub fn checkout(cache: &Path, pin: &Pin) -> Result<PathBuf, SuiteError> {
    let kept = cache.join(&pin.commit);
    match std::fs::symlink_metadata(&kept) {
        Ok(_held) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fetch(cache, &kept, pin)?,
        Err(error) => {
            return Err(SuiteError::Checkout {
                checkout: kept,
                commit: pin.commit.clone(),
                said: error.to_string(),
            });
        }
    }
    verify(&kept, &pin.commit)?;
    Ok(kept)
}

/// Whether what libtest printed says it ran [`HARNESS_TEST`] and it passed.
#[must_use]
pub fn ran(printed: &str) -> bool {
    let passed = format!("test {HARNESS_TEST} ... ok");
    printed.lines().any(|line| line.trim_end() == passed)
}

/// Runs the harness over the checkout at `checkout` with `cargo` in the workspace at `root`.
fn harness(root: &Path, cargo: &OsStr, checkout: &Path) -> Result<(), SuiteError> {
    let output = Command::new(cargo)
        .args([
            "test",
            "--locked",
            "--package",
            HARNESS_PACKAGE,
            "--test",
            HARNESS_TARGET,
            "--",
            "--ignored",
            "--exact",
            HARNESS_TEST,
        ])
        .env(SUITE_VARIABLE, checkout)
        .current_dir(root)
        .output()
        .map_err(|source| SuiteError::Unrun { source })?;
    let printed = |bytes: &[u8]| match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_not_text) => bytes.escape_ascii().to_string(),
    };
    let said = format!("{}{}", printed(&output.stderr), printed(&output.stdout));
    if !output.status.success() {
        return Err(SuiteError::Departed {
            status: output.status,
            said,
        });
    }
    if ran(&printed(&output.stdout)) {
        Ok(())
    } else {
        Err(SuiteError::Unran {
            status: output.status,
            said,
        })
    }
}

/// Runs every preview1 test of the suite the expectations of the workspace at `root` pin on the sealed host, fetching it into `cache` the first time, with `cargo`.
///
/// # Errors
/// Every [`SuiteError`]: expectations it cannot read, a suite it cannot fetch or verify, a harness it cannot run, and a test that ends otherwise than the expectations say.
pub fn run(root: &Path, cargo: &OsStr, cache: &Path) -> Result<String, SuiteError> {
    let path = root.join(EXPECTATIONS);
    let text = std::fs::read_to_string(&path).map_err(|error| SuiteError::Expectations {
        path: path.clone(),
        said: error.to_string(),
    })?;
    let pin = pin(&text)?;
    let checkout = checkout(cache, &pin)?;
    harness(root, cargo, &checkout)?;
    Ok(format!(
        "wasi-testsuite: every preview1 test of {} at {} ended on the sealed host as {EXPECTATIONS} \
         says: {} passed, and {} failed on a refusal a row of the import table documents",
        pin.repository, pin.commit, pin.passing, pin.refused
    ))
}
