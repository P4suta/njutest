// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reaching the parts of a workspace a test drives directly.

use crate::cargo::Driver;
use crate::runner::Cancel;
use crate::workspace::Workspace;

/// The handle a workspace's own phases run cargo through.
#[must_use]
pub fn driver<'a>(workspace: &'a Workspace, cancel: &'a Cancel) -> Driver<'a> {
    workspace.driver(cancel)
}
