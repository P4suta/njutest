// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One function only a child process runs, started by a test that clears its environment first.

/// The answer the child prints.
pub fn answer() -> u32 {
    40 + 2
}
