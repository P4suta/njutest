// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reaching the parts of a workspace a test drives directly.
//!
//! A workspace hands the toolchain to its own phases and to nothing else,
//! which is what keeps the seam ledger empty. A test about one command — what
//! it does when the toolchain is slow, or when nobody waits for it — needs the
//! same handle those phases get, and this is the one place that lends it.

use crate::cargo::Driver;
use crate::runner::Cancel;
use crate::workspace::Workspace;

/// The handle a workspace's own phases run cargo through.
#[must_use]
pub fn driver<'a>(workspace: &'a Workspace, cancel: &'a Cancel) -> Driver<'a> {
    workspace.driver(cancel)
}
