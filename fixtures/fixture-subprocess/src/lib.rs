// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library only the binary calls, so the only test that reaches it reaches it through another process.

/// What to say about `n`.
pub fn decide(n: i32) -> &'static str {
    if n > 0 { "positive" } else { "not positive" }
}
