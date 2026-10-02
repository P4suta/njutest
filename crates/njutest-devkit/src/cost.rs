// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Work performed directly by toolchain tests, alongside the engine's optional cost records.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Cargo command classes the engine knows of that never run under a run's watch, so no record can count them.
pub const UNOBSERVED_CARGO: [&str; 1] =
    ["toolchain banners located outside a costed run (standalone commands and test support)"];

/// The one cost-record schema every producer in this workspace publishes.
pub const SCHEMA: &str = "njutest-test-cost-v3";

/// The multiplicity of one bound build key: every request, process, hit, miss and concrete cause.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyWork {
    /// How many times this complete input was asked for.
    pub requests: u64,
    /// How many of those asks a verified record answered.
    pub hits: u64,
    /// How many of those asks no verified record answered.
    pub misses: u64,
    /// How many actual Cargo processes served those misses.
    pub processes: u64,
    /// How many of those misses never started a child.
    pub failed_launches: u64,
    /// Each miss's concrete cause, by its stable class.
    pub reasons: BTreeMap<String, u64>,
    /// Each record write the facade refused, by cause.
    pub refused_writes: BTreeMap<String, u64>,
    /// Each failed launch's concrete cause.
    pub launch_causes: BTreeMap<String, u64>,
}

/// The multiplicity of one unbound identity, with the reason it never had a key.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnboundWork {
    /// How many times this identity was asked for.
    pub requests: u64,
    /// How many of those asks no verified record answered.
    pub misses: u64,
    /// How many actual Cargo processes served it.
    pub processes: u64,
    /// How many of its launches never started a child.
    pub failed_launches: u64,
    /// Each failed launch's concrete cause.
    pub launch_causes: BTreeMap<String, u64>,
}

/// The complete measured work one record claims, owning every required field.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Work {
    /// Actual Cargo build processes that started.
    pub builds: u64,
    /// Their measured wall time in milliseconds.
    pub build_ms: u64,
    /// Compiler artifacts Cargo reported fresh.
    pub units: u64,
    /// Build requests of every identity.
    pub build_requests: u64,
    /// Requests a verified cache record answered.
    pub build_hits: u64,
    /// Requests no verified record answered.
    pub build_misses: u64,
    /// Per bound input key multiplicity.
    pub build_keys: BTreeMap<String, KeyWork>,
    /// Per unbound identity multiplicity.
    pub unbound: BTreeMap<String, UnboundWork>,
    /// Launches that never started a child.
    pub launch_failures: u64,
    /// Cargo starts this recorder observed, by filename.
    pub observed_cargo_starts: u64,
    /// Cargo toolchain probes observed.
    pub cargo_probes: u64,
    /// Their measured wall time in milliseconds.
    pub cargo_probe_ms: u64,
    /// Cargo metadata commands observed.
    pub cargo_metadata: u64,
    /// Their measured wall time in milliseconds.
    pub cargo_metadata_ms: u64,
    /// Rustc toolchain probes observed.
    pub rustc_probes: u64,
    /// Their measured wall time in milliseconds.
    pub rustc_probe_ms: u64,
    /// Cargo command classes this recorder knows it cannot observe.
    pub unobserved_cargo: Vec<String>,
    /// Platform-probe work, as the engine records it.
    pub platform: Vec<serde_json::Value>,
    /// Platform-probe requests.
    pub platform_requests: u64,
    /// Why the accounting failed, where it did.
    pub error: Option<String>,
}

impl Work {
    /// The work of a record with no Cargo work of its own: its typed operation proves none happened.
    #[must_use]
    pub fn none() -> Self {
        Self {
            builds: 0,
            build_ms: 0,
            units: 0,
            build_requests: 0,
            build_hits: 0,
            build_misses: 0,
            build_keys: BTreeMap::new(),
            unbound: BTreeMap::new(),
            launch_failures: 0,
            observed_cargo_starts: 0,
            cargo_probes: 0,
            cargo_probe_ms: 0,
            cargo_metadata: 0,
            cargo_metadata_ms: 0,
            rustc_probes: 0,
            rustc_probe_ms: 0,
            unobserved_cargo: UNOBSERVED_CARGO
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            platform: Vec::new(),
            platform_requests: 0,
            error: None,
        }
    }

    /// The work of one direct Cargo launch this helper owned: one request, started or failed, outside the cache protocol.
    #[must_use]
    pub fn direct(identity: &str, launch: DirectLaunch) -> Self {
        let mut work = Self::none();
        work.build_requests = 1;
        let held = match launch {
            DirectLaunch::Started { millis, units } => {
                work.builds = 1;
                work.build_ms = millis;
                work.units = units;
                work.observed_cargo_starts = 1;
                UnboundWork {
                    requests: 1,
                    misses: 0,
                    processes: 1,
                    failed_launches: 0,
                    launch_causes: BTreeMap::new(),
                }
            }
            DirectLaunch::Failed { cause } => {
                work.launch_failures = 1;
                let mut launch_causes = BTreeMap::new();
                launch_causes.insert(cause, 1);
                UnboundWork {
                    requests: 1,
                    misses: 0,
                    processes: 0,
                    failed_launches: 1,
                    launch_causes,
                }
            }
        };
        work.unbound.insert(identity.to_owned(), held);
        work
    }

    /// Reads one complete work block, refusing an absent, unknown or partial observation.
    ///
    /// # Errors
    /// The block is not the complete protocol this schema publishes.
    pub fn parse(value: &serde_json::Value) -> io::Result<Self> {
        crate::strictjson::decode_str(&value.to_string())
            .map_err(|source| io::Error::other(format!("an incomplete work block: {source}")))
    }

    /// Publishes this work as one complete record, or does nothing when cost recording was not requested.
    ///
    /// # Errors
    /// The nextest labels, output directory or complete record cannot be written.
    pub fn publish(&self, root: &Path, sealed: &serde_json::Value, prefix: &str) -> io::Result<()> {
        let Some(directory) = std::env::var_os("NJUTEST_TEST_COST_DIR") else {
            return Ok(());
        };
        let binary = std::env::var("NEXTEST_BINARY_ID").map_err(io::Error::other)?;
        let test = std::env::var("NEXTEST_TEST_NAME").map_err(io::Error::other)?;
        let root = root
            .to_str()
            .ok_or_else(|| io::Error::other("the test cost root is not UTF-8"))?;
        std::fs::create_dir_all(&directory)?;
        let record = serde_json::json!({
            "schema": SCHEMA, "binary": binary, "test": test, "root": root,
            "work": self, "sealed": sealed,
        });
        let mut staged = tempfile::Builder::new()
            .prefix(prefix)
            .suffix(".json")
            .tempfile_in(&directory)?;
        serde_json::to_writer(staged.as_file_mut(), &record).map_err(io::Error::other)?;
        let (file, path) = staged.keep().map_err(|source| source.error)?;
        drop(file);
        drop(path);
        Ok(())
    }
}

/// One direct Cargo launch's actual outcome.
#[derive(Debug)]
pub enum DirectLaunch {
    /// A process started, ran for `millis`, and compiled `units` fresh artifacts.
    Started {
        /// The measured wall time in milliseconds.
        millis: u64,
        /// Compiler artifacts Cargo reported fresh.
        units: u64,
    },
    /// No child started, for this concrete cause.
    Failed {
        /// Why nothing started.
        cause: String,
    },
}

/// Validates and re-publishes an existing complete work block under this schema, or does nothing uncosted.
///
/// # Errors
/// The block is not the complete protocol, or the record cannot be written.
pub fn record(root: &Path, work: &serde_json::Value, sealed: &serde_json::Value) -> io::Result<()> {
    Work::parse(work)?.publish(root, sealed, "cost-")
}

/// Records one direct Cargo build, named by `context`, counting only the compiler artifacts Cargo reported fresh.
///
/// # Errors
/// A measured duration, artifact count or diagnostic cannot be represented or written.
pub fn build(
    root: &Path,
    context: &str,
    duration: std::time::Duration,
    stdout: &[u8],
) -> io::Result<()> {
    let millis = u64::try_from(duration.as_millis()).map_err(io::Error::other)?;
    Work::direct(
        &format!("direct: {context}"),
        DirectLaunch::Started {
            millis,
            units: fresh_units(stdout)?,
        },
    )
    .publish(root, &serde_json::Value::Null, "cost-direct-")
}

/// Publishes one guest runner's module work with no Cargo work of its own.
///
/// # Errors
/// The complete record cannot be written.
pub fn guest_modules(root: &Path, sealed: &serde_json::Value) -> io::Result<()> {
    Work::none().publish(root, sealed, "cost-")
}

/// Counts the compiler artifacts Cargo reported fresh in its JSON message stream.
///
/// # Errors
/// A message is not the stream this version understands, or the count overflows.
pub fn fresh_units(stdout: &[u8]) -> io::Result<u64> {
    let mut units = 0_u64;
    for message in cargo_metadata::Message::parse_stream(io::Cursor::new(stdout)) {
        if let cargo_metadata::Message::CompilerArtifact(artifact) = message?
            && !artifact.fresh
        {
            units = units
                .checked_add(1)
                .ok_or_else(|| io::Error::other("fresh artifact count overflowed"))?;
        }
    }
    Ok(units)
}

/// Runs a direct fixture build, named by `context`, with Cargo JSON messages and records its actual work.
///
/// # Errors
/// Cargo's complete diagnostic cannot be written; the launch failure itself is recorded, then reported.
pub fn cargo(
    mut command: std::process::Command,
    context: &str,
) -> io::Result<std::process::Output> {
    if !command.get_args().any(|arg| {
        arg == "--message-format"
            || arg
                .to_str()
                .is_some_and(|arg| arg.starts_with("--message-format="))
    }) {
        command.arg("--message-format=json");
    }
    let root = command
        .get_current_dir()
        .ok_or_else(|| io::Error::other("a measured fixture build needs its directory"))?
        .to_path_buf();
    let identity = format!("direct: {context}");
    let began = std::time::Instant::now();
    let output = match command.output() {
        Ok(output) => output,
        Err(source) => {
            let failed = DirectLaunch::Failed {
                cause: source.to_string(),
            };
            Work::direct(&identity, failed).publish(
                &root,
                &serde_json::Value::Null,
                "cost-direct-",
            )?;
            return Err(source);
        }
    };
    let millis = u64::try_from(began.elapsed().as_millis()).map_err(io::Error::other)?;
    Work::direct(
        &identity,
        DirectLaunch::Started {
            millis,
            units: fresh_units(&output.stdout)?,
        },
    )
    .publish(&root, &serde_json::Value::Null, "cost-direct-")?;
    Ok(output)
}
