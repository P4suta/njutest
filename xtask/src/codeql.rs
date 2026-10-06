// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Each local security analysis owns a fresh retained generation and checks its actual inputs again.

use std::collections::BTreeSet;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest as _, Sha256};
use thiserror::Error;

/// The complete pinned bundle and its unchanged native invocation selected by the setup adapter.
#[derive(Debug, clap::Args)]
pub struct Options {
    /// The pinned `CodeQL` executable.
    #[arg(long)]
    pub program: PathBuf,
    /// The complete bundle directory.
    #[arg(long)]
    pub bundle: PathBuf,
    /// The original Rust security-extended query suite.
    #[arg(long)]
    pub query: PathBuf,
    /// The actual user configuration path, including when it is absent.
    #[arg(long)]
    pub user_config: PathBuf,
    /// Run the pinned macOS `x86_64` bundle through the supported Rosetta adapter.
    #[arg(long)]
    pub rosetta: bool,
}

/// A security invocation or its retained input evidence could not establish a verdict.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CodeqlError {
    /// An actual input or retained output could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The affected input or output.
        path: PathBuf,
        /// The actual filesystem refusal.
        source: std::io::Error,
    },
    /// The invocation failed or its complete input or result contract changed.
    #[error("{detail}")]
    Contract {
        /// The actual refusal, including the retained generation where available.
        detail: String,
    },
    /// The original process owner could not settle the complete invocation.
    #[error(transparent)]
    Work {
        /// The original process or observation refusal.
        #[from]
        source: crate::work::WorkError,
    },
}

impl crate::error::Coded for CodeqlError {
    fn code(&self) -> crate::error::XtCode {
        crate::error::XtCode::GateRefused
    }
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct Inputs {
    source: String,
    program: String,
    bundle: String,
    query: String,
    user_config: Option<String>,
    rosetta: bool,
    native_adapter: Option<[String; 2]>,
}

struct Generation {
    directory: PathBuf,
}

impl Generation {
    fn create(root: &Path) -> Result<Self, CodeqlError> {
        let parent = root.join("target/codeql");
        std::fs::create_dir_all(&parent).map_err(|source| io(&parent, source))?;
        let instant = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|source| contract(format!("generation clock: {source}")))?;
        let directory = parent.join(format!(
            "generation-{}-{}",
            std::process::id(),
            instant.as_nanos()
        ));
        Self::at(directory)
    }

    fn at(directory: PathBuf) -> Result<Self, CodeqlError> {
        std::fs::DirBuilder::new()
            .create(&directory)
            .map_err(|source| io(&directory, source))?;
        let generation = Self { directory };
        generation.write("owner.json", &std::process::id())?;
        Ok(generation)
    }

    fn write<T: serde::Serialize>(&self, name: &str, value: &T) -> Result<(), CodeqlError> {
        let path = self.directory.join(name);
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|source| contract(format!("{}: {source}", path.display())))?;
        let mut file = std::fs::File::create_new(&path).map_err(|source| io(&path, source))?;
        file.write_all(&bytes).map_err(|source| io(&path, source))
    }
}

/// Runs the original full query analysis once, preserving every successful or failed generation.
///
/// # Errors
/// Every input, process, analysis, changed-input and security-finding refusal remains a failed gate.
pub fn run(
    root: &Path,
    options: &Options,
    environment: &crate::environment::Environment,
) -> Result<PathBuf, CodeqlError> {
    let generation = Generation::create(root)?;
    let before = inputs(root, options)?;
    generation.write("inputs-before.json", &before)?;
    let stops = crate::work::Stops::arm()?;
    let version = generation.directory.join("version.json");
    let version_output =
        std::fs::File::create_new(&version).map_err(|source| io(&version, source))?;
    let mut identify = command(options, environment, root);
    identify
        .args(["version", "--format=json"])
        .stdout(version_output);
    execute(&mut identify, &stops, &generation)?;
    let version_bytes = std::fs::read(&version).map_err(|source| io(&version, source))?;
    let identified = crate::strictjson::from_slice(&version_bytes)
        .map_err(|source| contract(format!("CodeQL version: {source}")))?;
    if identified
        .get("version")
        .and_then(serde_json::Value::as_str)
        != Some("2.27.1")
    {
        return Err(contract(format!(
            "CodeQL is not pinned2.27.1: {identified}"
        )));
    }
    let database = generation.directory.join("database");
    let sarif = generation.directory.join("results.sarif");
    let mut create = command(options, environment, root);
    create.args(["database", "create"]).arg(&database).args([
        "--language=rust",
        "--build-mode=none",
        "--source-root=.",
        "--threads=6",
    ]);
    let created = execute(&mut create, &stops, &generation);
    let analyzed = created.and_then(|()| {
        let mut analyze = command(options, environment, root);
        analyze
            .args(["database", "analyze"])
            .arg(&database)
            .arg(&options.query)
            .arg(format!(
                "--search-path={}",
                options.bundle.join("qlpacks").display()
            ))
            .args(["--threads=6", "--format=sarifv2.1.0", "--output"])
            .arg(&sarif);
        execute(&mut analyze, &stops, &generation)
    });
    generation.write("execution.json", &format!("{analyzed:?}"))?;
    let after = inputs(root, options)?;
    generation.write("inputs-after.json", &after)?;
    analyzed?;
    if before != after {
        return Err(contract(format!(
            "CodeQL inputs changed during analysis; retained generation: {}",
            generation.directory.display()
        )));
    }
    let bytes = std::fs::read(&sarif).map_err(|source| io(&sarif, source))?;
    let findings = findings(&bytes)?;
    println!(
        "security:codeql: {findings} findings; SARIF: {}",
        sarif.display()
    );
    if findings != 0 {
        return Err(contract(format!(
            "CodeQL reported {findings} security findings"
        )));
    }
    Ok(generation.directory)
}

fn command(
    options: &Options,
    environment: &crate::environment::Environment,
    root: &Path,
) -> Command {
    let mut command = if options.rosetta {
        let mut command = Command::new("/usr/bin/arch");
        command.args(["-x86_64", "/bin/bash"]).arg(&options.program);
        command
    } else {
        Command::new(&options.program)
    };
    command
        .current_dir(root)
        .env_clear()
        .envs(environment.pairs())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command
}

fn execute(
    command: &mut Command,
    stops: &crate::work::Stops,
    generation: &Generation,
) -> Result<(), CodeqlError> {
    let ended = crate::work::run(command, None, stops, |_| Ok(()))?;
    match ended {
        crate::work::Ended::Exited(status) if status.success() => Ok(()),
        crate::work::Ended::Exited(status) => Err(contract(format!(
            "CodeQL failed {status}; retained generation: {}",
            generation.directory.display()
        ))),
        crate::work::Ended::OverBudget { elapsed } => {
            Err(contract(format!("CodeQL stopped after {elapsed:?}")))
        }
        crate::work::Ended::Quiet { silent } => Err(contract(format!(
            "CodeQL stopped after {silent:?} without output"
        ))),
        crate::work::Ended::Interrupted { signal } => {
            Err(contract(format!("CodeQL stopped by signal {signal}")))
        }
    }
}

fn inputs(root: &Path, options: &Options) -> Result<Inputs, CodeqlError> {
    let mut source = Sha256::new();
    for path in crate::repository::files(root)
        .map_err(|source| contract(format!("CodeQL source listing: {source}")))?
    {
        source.update(path.as_bytes());
        source.update([0]);
        source.update(file_digest(&root.join(path))?);
    }
    let mut bundle = Sha256::new();
    tree_digest(&options.bundle, &mut bundle, &mut BTreeSet::new())?;
    let user_config = match std::fs::metadata(&options.user_config) {
        Ok(_) => Some(hex::encode(file_digest(&options.user_config)?)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => return Err(io(&options.user_config, source)),
    };
    Ok(Inputs {
        source: hex::encode(source.finalize()),
        program: hex::encode(file_digest(&options.program)?),
        bundle: hex::encode(bundle.finalize()),
        query: hex::encode(file_digest(&options.query)?),
        user_config,
        rosetta: options.rosetta,
        native_adapter: if options.rosetta {
            Some([
                hex::encode(file_digest(Path::new("/usr/bin/arch"))?),
                hex::encode(file_digest(Path::new("/bin/bash"))?),
            ])
        } else {
            None
        },
    })
}

fn file_digest(path: &Path) -> Result<[u8; 32], CodeqlError> {
    let mut file = std::fs::File::open(path).map_err(|source| io(path, source))?;
    let mut digest = Sha256::new();
    let mut bytes = [0; 8192];
    loop {
        let count = file.read(&mut bytes).map_err(|source| io(path, source))?;
        if count == 0 {
            break;
        }
        let received = bytes
            .get(..count)
            .ok_or_else(|| contract("file read exceeded its input buffer".to_owned()))?;
        digest.update(received);
    }
    Ok(digest.finalize().into())
}

fn tree_digest(
    path: &Path,
    digest: &mut Sha256,
    visited: &mut BTreeSet<PathBuf>,
) -> Result<(), CodeqlError> {
    let actual = std::fs::canonicalize(path).map_err(|source| io(path, source))?;
    digest.update(path.as_os_str().as_encoded_bytes());
    digest.update([0]);
    if !visited.insert(actual) {
        return Ok(());
    }
    let metadata = std::fs::metadata(path).map_err(|source| io(path, source))?;
    if metadata.is_file() {
        digest.update(file_digest(path)?);
    } else if metadata.is_dir() {
        let mut children = crate::repository::entries(path).map_err(|source| io(path, source))?;
        children.sort();
        for child in children {
            tree_digest(&child, digest, visited)?;
        }
    } else {
        return Err(contract(format!(
            "CodeQL bundle input is not a file or directory: {}",
            path.display()
        )));
    }
    Ok(())
}

fn findings(bytes: &[u8]) -> Result<usize, CodeqlError> {
    let document = crate::strictjson::from_slice(bytes)
        .map_err(|source| contract(format!("CodeQL SARIF: {source}")))?;
    let runs = document
        .get("runs")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| contract("CodeQL SARIF has no runs array".to_owned()))?;
    if runs.is_empty() {
        return Err(contract("CodeQL SARIF has no analysis runs".to_owned()));
    }
    let mut findings = 0_usize;
    for run in runs {
        let results = run
            .get("results")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| contract("CodeQL SARIF run has no results array".to_owned()))?;
        findings = findings
            .checked_add(results.len())
            .ok_or_else(|| contract("CodeQL finding count overflow".to_owned()))?;
    }
    Ok(findings)
}

fn io(path: &Path, source: std::io::Error) -> CodeqlError {
    CodeqlError::Io {
        path: path.to_owned(),
        source,
    }
}

const fn contract(detail: String) -> CodeqlError {
    CodeqlError::Contract { detail }
}

#[cfg(test)]
mod tests {
    use super::{Generation, file_digest, findings};
    use njutest_devkit::result::{ResultState, result_state};

    #[test]
    fn generations_preserve_prior_outputs_and_refuse_existing_ownership() {
        let root = tempfile::tempdir().expect("generation parent");
        let old = root.path().join("target/codeql/database");
        std::fs::create_dir_all(&old).expect("retained old database");
        std::fs::write(old.join("original"), b"retained failed bytes").expect("old evidence");
        let first = Generation::create(root.path()).expect("first generation");
        let second = Generation::create(root.path()).expect("second generation");
        assert_ne!(first.directory, second.directory);
        assert!(Generation::at(first.directory.clone()).is_err());
        drop(first);
        let owner = std::fs::read(second.directory.join("owner.json")).expect("retained owner");
        assert_eq!(
            crate::strictjson::from_slice(&owner)
                .expect("owner identity")
                .as_u64(),
            Some(u64::from(std::process::id()))
        );
        assert_eq!(
            std::fs::read(old.join("original")).expect("original evidence"),
            b"retained failed bytes"
        );
    }

    #[test]
    fn input_identity_changes_when_actual_bytes_change() {
        let root = tempfile::tempdir().expect("actual file input");
        let file = root.path().join("query");
        std::fs::write(&file, b"original query").expect("original input");
        let before = file_digest(&file).expect("original identity");
        std::fs::write(&file, b"added query").expect("changed input");
        assert_ne!(before, file_digest(&file).expect("changed identity"));
        std::fs::remove_file(&file).expect("remove actual input");
        assert_eq!(result_state(&file_digest(&file)), ResultState::Refused);
    }

    #[test]
    fn analysis_counts_every_run_and_refuses_incomplete_results() {
        assert_eq!(
            result_state(&findings(br#"{"runs":[]}"#)),
            ResultState::Refused
        );
        assert_eq!(
            findings(br#"{"runs":[{"results":[]}]}"#).expect("complete zero-finding analysis"),
            0
        );
        assert_eq!(
            findings(br#"{"runs":[{"results":[{},{}]},{"results":[{}]}]}"#).expect("actual shape"),
            3
        );
        assert_eq!(
            result_state(&findings(br#"{"runs":[{}]}"#)),
            ResultState::Refused
        );
        assert_eq!(
            result_state(&findings(br#"{"runs":[] ,"runs":[{"results":[] }]}"#)),
            ResultState::Refused
        );
    }
}
