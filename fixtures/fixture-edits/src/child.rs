// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the binary decides, which a test only sees through a process it starts.

/// Which side of zero `n` is on.
pub fn decide(n: i32) -> &'static str {
    if n > 0 { "positive" } else { "not positive" }
}
