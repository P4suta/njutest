// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Work performed directly by toolchain tests, alongside the engine's optional cost records.

use std::io;
use std::path::Path;

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
        "schema": "njutest-test-cost-v1", "binary": binary, "test": test, "root": root,
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

/// Records one direct Cargo build, counting only the compiler artifacts Cargo reported fresh.
///
/// # Errors
/// A measured duration, artifact count or diagnostic cannot be represented or written.
pub fn build(root: &Path, duration: std::time::Duration, stdout: &[u8]) -> io::Result<()> {
    if std::env::var_os("NJUTEST_TEST_COST_DIR").is_none() {
        return Ok(());
    }
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
    published(
        root,
        &serde_json::json!({
            "builds": 1, "build_ms": millis, "units": units,
            "platform": [], "platform_requests": 0, "error": null,
        }),
        &serde_json::Value::Null,
        "cost-direct-",
    )
}

/// Runs a direct fixture build with Cargo JSON messages and records its actual work.
///
/// # Errors
/// Cargo cannot be started or its complete diagnostic cannot be written.
pub fn cargo(mut command: std::process::Command) -> io::Result<std::process::Output> {
    if !command.get_args().any(|arg| {
        arg == "--message-format"
            || arg
                .to_str()
                .is_some_and(|arg| arg.starts_with("--message-format="))
    }) {
        command.arg("--message-format=json");
    }
    let began = std::time::Instant::now();
    let output = command.output()?;
    let root = command
        .get_current_dir()
        .ok_or_else(|| io::Error::other("a measured fixture build needs its directory"))?;
    build(root, began.elapsed(), &output.stdout)?;
    Ok(output)
}
