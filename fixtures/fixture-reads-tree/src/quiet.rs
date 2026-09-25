// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A function no test calls, whose text a test checks.

/// The greeting.
pub fn quiet() -> &'static str {
    "hush"
}
