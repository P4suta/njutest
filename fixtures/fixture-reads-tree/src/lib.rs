// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One function a test calls, and one module a test only reads as text.

pub mod quiet;

/// Twice `n`.
pub fn loud(n: u32) -> u32 {
    n * 2
}
