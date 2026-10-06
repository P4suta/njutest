// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Explicitly measured native Cargo builds that do not pass through the engine's compiler facade.

use std::path::Path;
use std::time::Duration;

use super::{CargoError, CargoErrorKind, Message};
use crate::trace::Recorder;
use crate::vars::Variables;

/// One explicitly measured build outside the engine's compiler facade: what it was, how long it took, and what Cargo said.
#[derive(Debug, Clone, Copy)]
pub struct DirectBuild<'a> {
    /// What this build was, published as its `direct: ` identity.
    pub context: &'a str,
    /// How long the build took.
    pub duration: Duration,
    /// The Cargo message stream the build printed.
    pub messages: &'a [Message],
}

/// Records one actual Cargo build, named by its context, when the caller requests diagnostics.
///
/// # Errors
/// The diagnostic cannot be created or a measured count exceeds its width.
pub fn record_build(
    vars: Option<&Variables>,
    root: &Path,
    direct: DirectBuild<'_>,
) -> Result<(), CargoError> {
    let Some(vars) = vars else {
        return Ok(());
    };
    if !vars.holds("NJUTEST_TEST_COST_DIR") {
        return Ok(());
    }
    let trace = Recorder::disabled()
        .costed_as(vars, root, "cost-direct-")
        .map_err(|source| CargoError::new(CargoErrorKind::CommandFailed, source.to_string()))?;
    let identity = format!("direct: {}", direct.context);
    let millis = u64::try_from(direct.duration.as_millis())
        .map_err(|source| CargoError::new(CargoErrorKind::CommandFailed, source.to_string()))?;
    trace.note("fixture-build-request", &identity);
    trace.note("fixture-build-uncacheable", &identity);
    trace.note("fixture-build-process", &identity);
    trace.note("fixture-cargo-build", &millis.to_string());
    trace.note(
        "cargo-built-units",
        &super::compile::fresh_units(direct.messages)?.to_string(),
    );
    Ok(())
}
