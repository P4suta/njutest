// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Explicitly measured native Cargo builds that do not pass through the engine's compiler facade.

use std::path::Path;
use std::time::Duration;

use super::{CargoError, CargoErrorKind, Message};
use crate::trace::Recorder;
use crate::vars::Variables;

/// Records one actual Cargo build and its reported fresh artifacts when the caller requests diagnostics.
///
/// # Errors
/// The diagnostic cannot be created or a measured count exceeds its width.
pub fn record_build(
    vars: Option<&Variables>,
    root: &Path,
    duration: Duration,
    messages: &[Message],
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
    let millis = u64::try_from(duration.as_millis())
        .map_err(|source| CargoError::new(CargoErrorKind::CommandFailed, source.to_string()))?;
    trace.note("fixture-build-request", "direct");
    trace.note("fixture-build-uncacheable", "direct");
    trace.note("fixture-cargo-build", &millis.to_string());
    trace.note(
        "cargo-built-units",
        &super::compile::fresh_units(messages)?.to_string(),
    );
    Ok(())
}
