// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One answer, which one test asserts and another waits on.

/// Whether `count` is enough to go on.
#[must_use]
pub fn ready(count: i32) -> bool {
    count > 0
}
