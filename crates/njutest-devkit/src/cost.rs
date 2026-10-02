// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Work performed directly by toolchain tests, alongside the engine's optional cost records.

use std::io;

use serde_json::json;
use std::path::Path;

/// Cargo command classes the engine knows of that never run under a run's watch, so no record can count them.
pub const UNOBSERVED_CARGO: [&str; 1] =
    ["toolchain banners located outside a costed run (standalone commands and test support)"];

/// The one cost-record schema every producer in this workspace publishes.
pub const SCHEMA: &str = "njutest-test-cost-v3";

/// Publishes explicitly measured test work, or does nothing when cost recording was not requested.
///
/// # Errors
/// The nextest labels, output directory or complete record cannot be written.
pub fn record(root: &Path, work: &serde_json::Value, sealed: &serde_json::Value) -> io::Result<()> {
    published(root, work, sealed, "cost-")
}

/// Publishes one complete classified diagnostic with the same nextest identity requirements.
fn published(
    root: &Path,
    work: &serde_json::Value,
    sealed: &serde_json::Value,
    prefix: &str,
) -> io::Result<()> {
    publish(&Record {
        schema: SCHEMA,
        root,
        work,
        sealed,
        prefix,
    })
}

/// One complete record on its way to a file.
struct Record<'a> {
    schema: &'a str,
    root: &'a Path,
    work: &'a serde_json::Value,
    sealed: &'a serde_json::Value,
    prefix: &'a str,
}

/// Writes one complete record under the given schema, with the same nextest identity requirements.
fn publish(what: &Record<'_>) -> io::Result<()> {
    let Record {
        schema,
        root,
        work,
        sealed,
        prefix,
    } = &what;
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
        "schema": schema, "binary": binary, "test": test, "root": root,
        "work": work, "sealed": sealed,
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
    let millis = u64::try_from(duration.as_millis()).map_err(io::Error::other)?;
    let started = DirectBuild {
        context,
        millis,
        units,
    };
    started.publish(root)
}

/// One direct Cargo build this helper launched and observed itself.
struct DirectBuild<'a> {
    context: &'a str,
    millis: u64,
    units: u64,
}

/// One direct Cargo launch that never started, with the cause the operating system gave.
struct DirectFailure<'a> {
    context: &'a str,
    cause: &'a str,
}

impl DirectBuild<'_> {
    /// Publishes the build's complete work record: one owned, observed, started process.
    fn publish(&self, root: &Path) -> io::Result<()> {
        let held = work(
            LaunchCounts {
                identity: self.identity(),
                starts: 1,
                millis: self.millis,
                units: Some(self.units),
                failures: 0,
            },
            json!({
                "requests": 1, "misses": 0, "processes": 1,
                "failed_launches": 0, "launch_causes": {},
            }),
        );
        publish_work(root, "cost-direct-", &held)
    }

    fn identity(&self) -> String {
        format!("direct: {}", self.context)
    }
}

impl DirectFailure<'_> {
    /// Publishes the attempt's complete work record: one request that never started.
    fn publish(&self, root: &Path) -> io::Result<()> {
        let held = work(
            LaunchCounts {
                identity: self.identity(),
                starts: 0,
                millis: 0,
                units: None,
                failures: 1,
            },
            json!({
                "requests": 1, "misses": 1, "processes": 0,
                "failed_launches": 1, "launch_causes": {self.cause: 1},
            }),
        );
        publish_work(root, "cost-direct-", &held)
    }

    fn identity(&self) -> String {
        format!("direct: {}", self.context)
    }
}

/// The counts one launching identity's work block is built from.
struct LaunchCounts {
    identity: String,
    starts: u64,
    millis: u64,
    units: Option<u64>,
    failures: u64,
}

/// Builds one complete work block around the launching identity's counts.
fn work(counts: LaunchCounts, launched: serde_json::Value) -> serde_json::Value {
    let LaunchCounts {
        identity,
        starts,
        millis,
        units,
        failures,
    } = counts;
    let mut unbound = serde_json::Map::new();
    unbound.insert(identity, launched);
    json!({
        "builds": starts, "build_ms": millis, "units": units.unwrap_or(0),
        "build_misses": failures.min(1),
        "build_keys": {},
        "unbound": serde_json::Value::Object(unbound),
        "launch_failures": failures, "observed_cargo_starts": starts,
        "cargo_probes": 0, "cargo_probe_ms": 0, "cargo_metadata": 0, "cargo_metadata_ms": 0,
        "rustc_probes": 0, "rustc_probe_ms": 0,
        "unobserved_cargo": UNOBSERVED_CARGO,
        "platform": [], "platform_requests": 0, "error": null,
    })
}

/// Publishes one complete work block as a record.
fn publish_work(root: &Path, prefix: &str, work: &serde_json::Value) -> io::Result<()> {
    publish(&Record {
        schema: SCHEMA,
        root,
        work,
        sealed: &serde_json::Value::Null,
        prefix,
    })
}

/// Publishes one guest's own Cargo build: a direct identity with one owned, observed start.
///
/// # Errors
/// The complete record cannot be written.
pub fn guest_build(root: &Path, millis: u64) -> io::Result<()> {
    let started = DirectBuild {
        context: "a guest's own cargo test build",
        millis,
        units: 0,
    };
    started.publish(root)
}

/// Publishes one guest runner's module work with no Cargo work at all.
///
/// # Errors
/// The complete record cannot be written.
pub fn guest_modules(root: &Path, sealed: &serde_json::Value) -> io::Result<()> {
    publish(&Record {
        schema: SCHEMA,
        root,
        work: &module_only(),
        sealed,
        prefix: "cost-",
    })
}

/// The complete work block of a record with no Cargo work of its own.
fn module_only() -> serde_json::Value {
    json!({
        "builds": 0, "build_ms": 0, "units": 0,
        "build_requests": 0, "build_hits": 0, "build_misses": 0,
        "build_keys": {}, "unbound": {},
        "launch_failures": 0, "observed_cargo_starts": 0,
        "cargo_probes": 0, "cargo_probe_ms": 0, "cargo_metadata": 0, "cargo_metadata_ms": 0,
        "rustc_probes": 0, "rustc_probe_ms": 0,
        "unobserved_cargo": UNOBSERVED_CARGO,
        "platform": [], "platform_requests": 0, "error": null,
    })
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
    let began = std::time::Instant::now();
    let output = match command.output() {
        Ok(output) => output,
        Err(source) => {
            let failed = DirectFailure {
                context,
                cause: &source.to_string(),
            };
            failed.publish(&root)?;
            return Err(source);
        }
    };
    build(&root, context, began.elapsed(), &output.stdout)?;
    Ok(output)
}
