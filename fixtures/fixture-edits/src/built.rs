// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the build script said.

/// The word the build script gave.
pub fn built() -> &'static str {
    env!("EDITS_BUILT")
}
